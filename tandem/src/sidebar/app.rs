use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::config::Config;
use crate::coordination::store::CoordStore;
use crate::sidebar::model::{
    SidebarRow, first_selectable, last_selectable, next_selectable, prev_selectable, rows_from,
};
use crate::tmux::SESSION;

use crate::tmux::client::TmuxClient;
pub use crate::ui::text::truncate_to_width;

pub struct SidebarApp {
    pub workspace: PathBuf,
    pub store: Arc<CoordStore>,
    pub tmux: Option<TmuxClient>,
    pub rows: Vec<SidebarRow>,
    pub selected: usize,
    pub brand_focused: bool,
    pub status_msg: Option<String>,
    pub dirty: bool,
    pub last_refresh: f64,
    pub editing_agent: Option<(String, String)>,
}

impl SidebarApp {
    pub fn new(workspace: PathBuf, store: CoordStore, tmux: Option<TmuxClient>) -> Self {
        let store_arc = Arc::new(store);
        let mut app = Self {
            workspace,
            store: store_arc,
            tmux,
            rows: Vec::new(),
            selected: 0,
            brand_focused: false,
            status_msg: None,
            dirty: true,
            last_refresh: 0.0,
            editing_agent: None,
        };
        app.refresh();
        app
    }

    pub fn refresh(&mut self) {
        let panes = if let Some(ref tmux) = self.tmux {
            crate::tmux::layout::Layout::new(tmux)
                .panes()
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let sessions = self.store.sessions();
        let queries = self.store.queries();
        let active = panes
            .iter()
            .find(|p| p.window == "main" && p.kind == "agent")
            .map(|p| p.id.as_str());

        let selected_str = if let Some(ref tmux) = self.tmux {
            tmux.run(&[
                "display-message",
                "-p",
                "-t",
                SESSION,
                "#{@tandem_selected}",
            ])
            .map(|r| r.out.trim().to_string())
            .unwrap_or_default()
        } else {
            String::new()
        };
        let selected_sessions: Vec<String> = if selected_str.is_empty() {
            Vec::new()
        } else {
            selected_str
                .split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim().to_string())
                .collect()
        };

        self.rows = rows_from(&panes, &sessions, &queries, active, &selected_sessions);
        if !self.rows.is_empty()
            && !self.rows[self.selected.min(self.rows.len() - 1)].is_selectable()
        {
            self.selected = first_selectable(&self.rows);
        } else if self.selected >= self.rows.len() {
            self.selected = last_selectable(&self.rows);
        }
        self.dirty = true;
    }

    pub fn handle_key(&mut self, key: &KeyEvent) {
        if let Some((target, mut buf)) = self.editing_agent.take() {
            match key.code {
                KeyCode::Esc => {
                    self.editing_agent = None;
                    self.dirty = true;
                }
                KeyCode::Backspace => {
                    buf.pop();
                    self.editing_agent = Some((target, buf));
                    self.dirty = true;
                }
                KeyCode::Char(c) if c.is_ascii_alphanumeric() && buf.len() < 32 => {
                    buf.push(c);
                    self.editing_agent = Some((target, buf));
                    self.dirty = true;
                }
                KeyCode::Enter => {
                    let cfg = crate::config::Config::from_env().unwrap_or_default();
                    crate::commands::rename_agent_cmd(&self.workspace, &cfg, &target, Some(&buf));
                    self.editing_agent = None;
                    self.refresh();
                }
                _ => {
                    self.editing_agent = Some((target, buf));
                }
            }
            return;
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.brand_focused {
                    self.brand_focused = false;
                    self.selected = last_selectable(&self.rows);
                } else if self.selected == first_selectable(&self.rows) {
                    self.brand_focused = true;
                } else {
                    self.selected = prev_selectable(&self.rows, self.selected);
                }
                self.dirty = true;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.brand_focused {
                    self.brand_focused = false;
                    self.selected = first_selectable(&self.rows);
                } else if self.selected == last_selectable(&self.rows) {
                    self.brand_focused = true;
                } else {
                    self.selected = next_selectable(&self.rows, self.selected);
                }
                self.dirty = true;
            }
            KeyCode::Home => {
                self.brand_focused = true;
                self.dirty = true;
            }
            KeyCode::End => {
                self.brand_focused = false;
                self.selected = last_selectable(&self.rows);
                self.dirty = true;
            }
            KeyCode::Enter => {
                if self.brand_focused {
                    self.open_link();
                } else {
                    self.select_selected(false);
                }
            }
            KeyCode::Char(' ') => {
                if self.brand_focused {
                    self.open_link();
                } else {
                    self.select_selected(true);
                }
            }
            KeyCode::Esc => {
                self.return_focus_to_agent();
            }
            KeyCode::Char('r') => {
                if self.selected < self.rows.len() {
                    if let SidebarRow::Agent { pane_id, name, .. } = &self.rows[self.selected] {
                        self.editing_agent = Some((pane_id.clone(), name.clone()));
                        self.dirty = true;
                    }
                }
            }
            _ => {}
        }
    }

    pub fn handle_mouse(&mut self, mouse: &MouseEvent, area: Rect) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let row_offset = mouse.row.saturating_sub(area.y);
                if row_offset == 0 {
                    self.brand_focused = true;
                    self.open_link();
                    self.dirty = true;
                } else {
                    self.brand_focused = false;
                    let idx = (row_offset - 1) as usize;
                    if idx < self.rows.len() && self.rows[idx].is_selectable() {
                        self.selected = idx;
                        self.dirty = true;
                        let is_toggle = mouse.modifiers.contains(KeyModifiers::SHIFT)
                            || mouse.modifiers.contains(KeyModifiers::ALT);
                        self.select_selected(is_toggle);
                    }
                }
            }
            MouseEventKind::ScrollUp => {
                if self.brand_focused {
                    self.brand_focused = false;
                    self.selected = last_selectable(&self.rows);
                } else if self.selected == first_selectable(&self.rows) {
                    self.brand_focused = true;
                } else {
                    self.selected = prev_selectable(&self.rows, self.selected);
                }
                self.dirty = true;
            }
            MouseEventKind::ScrollDown => {
                if self.brand_focused {
                    self.brand_focused = false;
                    self.selected = first_selectable(&self.rows);
                } else if self.selected == last_selectable(&self.rows) {
                    self.brand_focused = true;
                } else {
                    self.selected = next_selectable(&self.rows, self.selected);
                }
                self.dirty = true;
            }
            _ => {}
        }
    }

    pub fn open_link_with(&mut self, platform: &str, spawner: &dyn crate::ui::link::UrlSpawner) {
        match crate::ui::link::open_url(crate::ui::link::REPO_URL, platform, spawner) {
            Ok(()) => {
                self.status_msg = Some("Opened in browser".to_string());
            }
            Err(_) => {
                self.status_msg = Some(format!(
                    "Could not open the link: {}",
                    crate::ui::link::REPO_URL
                ));
            }
        }
        self.dirty = true;
    }

    pub fn open_link(&mut self) {
        let platform = if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        self.open_link_with(platform, &crate::ui::link::ProcessUrlSpawner);
    }

    pub fn select_selected(&mut self, toggle: bool) {
        if self.selected >= self.rows.len() {
            return;
        }
        match &self.rows[self.selected] {
            SidebarRow::Agent { pane_id, .. } => {
                let pid = pane_id.clone();
                if let Some(ref tmux) = self.tmux {
                    let layout = crate::tmux::layout::Layout::new(tmux);
                    let _ = layout.select_agent(&pid, toggle);
                }
            }
            SidebarRow::AddAgent => {
                if let Some(ref tmux) = self.tmux {
                    let launcher = crate::tmux::keys::launcher_for(None, self.workspace.to_str());
                    let _ = tmux.run_checked(&[
                        "display-popup",
                        "-E",
                        "-w",
                        "90%",
                        "-h",
                        "90%",
                        &format!("{launcher} add-agent"),
                    ]);
                }
            }
            _ => {}
        }
    }

    pub fn activate_selected(&mut self) {
        self.select_selected(false);
    }

    pub fn return_focus_to_agent(&self) {
        if let Some(ref tmux) = self.tmux {
            let layout = crate::tmux::layout::Layout::new(tmux);
            let panes = layout.panes().unwrap_or_default();
            if let Some(agent_pane) = panes
                .iter()
                .find(|p| p.window == "main" && p.role != "sidebar" && p.role != "status")
            {
                let _ = layout
                    .client
                    .run_checked(&["select-pane", "-t", agent_pane.id.as_str()]);
            }
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let is_compact = area.width < 20;
        let ver = env!("CARGO_PKG_VERSION");
        let brand_text = if is_compact {
            let major_minor = ver.split('.').take(2).collect::<Vec<_>>().join(".");
            if self.brand_focused {
                format!("▶tdm {major_minor} ↗")
            } else {
                format!(" tdm {major_minor} ↗")
            }
        } else if self.brand_focused {
            format!("▶tandem {ver} ↗ ")
        } else {
            format!(" tandem {ver} ↗ ")
        };

        let mut brand_style = Style::default().add_modifier(Modifier::BOLD);
        if self.brand_focused {
            brand_style = brand_style.add_modifier(Modifier::REVERSED);
        }
        buf.set_string(
            area.x,
            area.y,
            truncate_to_width(&brand_text, area.width as usize),
            brand_style,
        );

        let max_rows_y = if self.status_msg.is_some() && area.height > 2 {
            area.y + area.height - 1
        } else {
            area.y + area.height
        };

        for (idx, row) in self.rows.iter().enumerate() {
            let current_y = area.y + 1 + idx as u16;
            if current_y >= max_rows_y {
                break;
            }

            let is_selected = idx == self.selected;
            match row {
                SidebarRow::Header { title, count } => {
                    let text = format!("{title:<12} [{count}]");
                    let style = Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD);
                    buf.set_string(
                        area.x + 1,
                        current_y,
                        truncate_to_width(&text, area.width as usize),
                        style,
                    );
                }
                SidebarRow::Agent {
                    pane_id,
                    name,
                    label,
                    cli,
                    glyph,
                    is_active,
                    is_in_grid,
                    has_query,
                    ..
                } => {
                    let prefix = if is_selected { "▶ " } else { "  " };
                    let check = if *is_in_grid { "☑ " } else { "  " };
                    let glyph_color = if *glyph == '✕' {
                        Color::Red
                    } else if *has_query {
                        Color::Yellow
                    } else if *is_active {
                        Color::Green
                    } else {
                        Color::DarkGray
                    };

                    let line = if let Some((ref target, ref edit_buf)) = self.editing_agent
                        && target == pane_id
                    {
                        format!("{prefix}{check}{glyph} {edit_buf}_")
                    } else {
                        let display_name = if !name.is_empty() {
                            name.as_str()
                        } else {
                            label.as_str()
                        };
                        format!("{prefix}{check}{glyph} {display_name:<10} {cli}")
                    };
                    let row_style = if is_selected {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default()
                    };
                    buf.set_string(
                        area.x,
                        current_y,
                        truncate_to_width(&line, area.width as usize),
                        row_style,
                    );

                    if !is_selected
                        && area.width > 4
                        && let Some(cell) = buf.cell_mut((area.x + 4, current_y))
                    {
                        cell.set_fg(glyph_color);
                    }
                }
                SidebarRow::AddAgent => {
                    let prefix = if is_selected { "▶ " } else { "  " };
                    let line = format!("{prefix}+ Add agent");
                    let style = if is_selected {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default().fg(Color::Cyan)
                    };
                    buf.set_string(
                        area.x,
                        current_y,
                        truncate_to_width(&line, area.width as usize),
                        style,
                    );
                }
                SidebarRow::Empty => {
                    let line = "  No agents yet.";
                    let style = Style::default().fg(Color::DarkGray);
                    buf.set_string(
                        area.x,
                        current_y,
                        truncate_to_width(line, area.width as usize),
                        style,
                    );
                }
            }
        }

        if self.editing_agent.is_some() && area.height > 1 {
            let hint = "Rename (A-Z, 0-9) · Enter save · Esc cancel";
            let status_y = area.y + area.height - 1;
            let status_style = Style::default().fg(Color::Yellow);
            buf.set_string(
                area.x,
                status_y,
                truncate_to_width(hint, area.width as usize),
                status_style,
            );
        } else if let Some(ref msg) = self.status_msg
            && area.height > 1
        {
            let status_y = area.y + area.height - 1;
            let status_style = Style::default().fg(Color::DarkGray);
            buf.set_string(
                area.x,
                status_y,
                truncate_to_width(msg, area.width as usize),
                status_style,
            );
        }
    }

    pub fn run<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<(), std::io::Error> {
        self.refresh();
        let mut last_tick = std::time::Instant::now();
        loop {
            if self.dirty {
                terminal.draw(|f| self.render(f.area(), f.buffer_mut()))?;
                self.dirty = false;
            }
            let timeout = Duration::from_millis(200);
            if crossterm::event::poll(timeout)? {
                match crossterm::event::read()? {
                    crossterm::event::Event::Key(key) => {
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL)
                            && key.code == KeyCode::Char('c')
                        {
                            break;
                        }
                        self.handle_key(&key);
                        self.dirty = true;
                    }
                    crossterm::event::Event::Mouse(mouse) => {
                        let area = terminal.get_frame().area();
                        self.handle_mouse(&mouse, area);
                        self.dirty = true;
                    }
                    crossterm::event::Event::Resize(..) => {
                        self.dirty = true;
                    }
                    _ => {}
                }
            }
            if last_tick.elapsed() >= Duration::from_secs(2) {
                self.refresh();
                last_tick = std::time::Instant::now();
            }
        }
        Ok(())
    }
}

pub fn run_sidebar(workspace: &Path, config: &Config) -> Result<(), std::io::Error> {
    let socket = crate::tmux::client::socket_for(workspace);
    let tmux = TmuxClient::new(&socket, None);
    let coord_dir = crate::commands::coord_dir_for(workspace, config);
    let store = CoordStore::new(coord_dir);
    let mut app = SidebarApp::new(workspace.to_path_buf(), store, Some(tmux));

    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let res = app.run(&mut terminal);

    let _ = crossterm::terminal::disable_raw_mode();
    let _ = crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    );
    let _ = terminal.show_cursor();

    res
}
