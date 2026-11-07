use std::io::stdout;
use std::path::Path;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, MouseButton, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;

use crate::config::Config;
use crate::tmux::client::TmuxClient;
use crate::tmux::grid::{GridPrefs, plan_grid};
use crate::tmux::layout::SESSION;

pub fn gridbar_launch(
    self_cmd: Option<&[&str]>,
    workspace: Option<&Path>,
) -> crate::tmux::layout::Launch {
    let argv = if let Some(cmd) = self_cmd {
        let mut v: Vec<String> = cmd.iter().map(|s| s.to_string()).collect();
        v.push("gridbar".to_string());
        v
    } else {
        vec!["tandem".to_string(), "gridbar".to_string()]
    };
    let mut launch = crate::tmux::layout::Launch::new(argv, "gridbar", 0, "gridbar");
    launch.kind = "own".to_string();
    if let Some(ws) = workspace {
        launch.env.insert(
            "TANDEM_WORKSPACE".to_string(),
            ws.to_string_lossy().to_string(),
        );
    }
    launch
}

pub struct GridbarApp {
    client: TmuxClient,
    pub offset: usize,
    pub total_rows: usize,
    pub rows_per_page: usize,
}

impl GridbarApp {
    pub fn new() -> Self {
        let client = TmuxClient::new(SESSION, None);
        Self {
            client,
            offset: 0,
            total_rows: 1,
            rows_per_page: 1,
        }
    }

    pub fn refresh(&mut self) {
        let offset_str = self
            .client
            .run(&[
                "display-message",
                "-p",
                "-t",
                SESSION,
                "#{@tandem_grid_offset}",
            ])
            .map(|r| r.out.trim().to_string())
            .unwrap_or_default();
        self.offset = offset_str.parse::<usize>().unwrap_or(0);

        let sel_str = self
            .client
            .run(&[
                "display-message",
                "-p",
                "-t",
                SESSION,
                "#{@tandem_selected}",
            ])
            .map(|r| r.out.trim().to_string())
            .unwrap_or_default();
        let selected_count = if sel_str.is_empty() {
            1
        } else {
            sel_str.split(',').filter(|s| !s.trim().is_empty()).count()
        };

        let prefs = GridPrefs::default();
        let plan = plan_grid(selected_count, 120, 30, &prefs, self.offset);
        self.total_rows = plan.total_rows.max(1);
        self.rows_per_page = plan.rows_per_page.max(1);
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let height = area.height as usize;
        let track_style = Style::default().fg(Color::DarkGray);
        let thumb_style = Style::default().fg(Color::White);

        if height <= 2 {
            for y in 0..area.height {
                buf.set_string(area.x, area.y + y, "█", thumb_style);
            }
            return;
        }

        let track_len = height - 2;
        let thumb_ratio = (self.rows_per_page as f64 / self.total_rows as f64).clamp(0.1, 1.0);
        let thumb_len = ((track_len as f64 * thumb_ratio).round() as usize).clamp(1, track_len);
        let max_offset = self.total_rows.saturating_sub(self.rows_per_page);
        let thumb_start = ((track_len - thumb_len) * self.offset)
            .checked_div(max_offset)
            .unwrap_or(0);

        buf.set_string(area.x, area.y, "▲", track_style);
        for i in 0..track_len {
            let y = area.y + 1 + i as u16;
            if i >= thumb_start && i < thumb_start + thumb_len {
                buf.set_string(area.x, y, "█", thumb_style);
            } else {
                buf.set_string(area.x, y, "░", track_style);
            }
        }
        buf.set_string(area.x, area.y + area.height - 1, "▼", track_style);
    }

    pub fn handle_event(&mut self, event: &Event) -> Option<isize> {
        match event {
            Event::Key(key) => match key.code {
                KeyCode::Up | KeyCode::PageUp => Some(-1),
                KeyCode::Down | KeyCode::PageDown => Some(1),
                _ => None,
            },
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => Some(-1),
                MouseEventKind::ScrollDown => Some(1),
                MouseEventKind::Down(MouseButton::Left) => {
                    let y = mouse.row as usize;
                    if y == 0 { Some(-1) } else { Some(1) }
                }
                _ => None,
            },
            _ => None,
        }
    }
}

impl Default for GridbarApp {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for &GridbarApp {
    fn render(self, area: Rect, buf: &mut Buffer) {
        GridbarApp::render(self, area, buf);
    }
}

pub fn run_gridbar(_workspace: &Path, _config: &Config) -> Result<(), std::io::Error> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = GridbarApp::new();
    loop {
        app.refresh();
        terminal.draw(|f| {
            f.render_widget(&app, f.area());
        })?;

        if event::poll(Duration::from_millis(500))? {
            let ev = event::read()?;
            if let Event::Key(key) = &ev
                && (key.code == KeyCode::Char('q') || key.code == KeyCode::Esc)
            {
                break;
            }
            if let Some(delta) = app.handle_event(&ev) {
                let _ = app
                    .client
                    .run(&["run-shell", &format!("tandem grid-scroll {delta}")]);
            }
        }
    }

    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    Ok(())
}
