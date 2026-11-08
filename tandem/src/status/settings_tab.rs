use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use std::time::Instant;

use crate::tmux::grid::{FillOrder, GridCols, GridPrefs, GridRows, SidebarMode};

pub const COLS_OPTIONS: [GridCols; 5] = [
    GridCols::Auto,
    GridCols::Fixed(1),
    GridCols::Fixed(2),
    GridCols::Fixed(3),
    GridCols::Fixed(4),
];

pub const ROWS_OPTIONS: [GridRows; 4] = [
    GridRows::Auto,
    GridRows::Fixed(1),
    GridRows::Fixed(2),
    GridRows::Fixed(3),
];

pub const RESET_ROW: usize = 4;

pub const FILL_OPTIONS: [FillOrder; 2] = [FillOrder::RowFirst, FillOrder::ColumnFirst];

pub const SIDEBAR_OPTIONS: [SidebarMode; 4] = [
    SidebarMode::Auto,
    SidebarMode::Wide,
    SidebarMode::Compact,
    SidebarMode::Hidden,
];

pub const QUIT_ROW: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsAction {
    Changed,
    ResetUi,
    QuitMenu,
}

pub struct SettingsTab {
    pub prefs: GridPrefs,
    pub sidebar_mode: SidebarMode,
    pub focused_row: usize,
    pub confirm_reset: bool,
    pub modal_focus: usize,
    pub flash_message: Option<(String, Instant)>,
}

impl Default for SettingsTab {
    fn default() -> Self {
        Self {
            prefs: GridPrefs::default(),
            sidebar_mode: SidebarMode::default(),
            focused_row: 0,
            confirm_reset: false,
            modal_focus: 1,
            flash_message: None,
        }
    }
}

impl SettingsTab {
    pub fn new(prefs: GridPrefs, sidebar_mode: SidebarMode) -> Self {
        Self {
            prefs,
            sidebar_mode,
            focused_row: 0,
            confirm_reset: false,
            modal_focus: 1,
            flash_message: None,
        }
    }

    pub fn flash(&mut self, msg: &str) {
        self.flash_message = Some((msg.to_string(), Instant::now()));
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> Option<SettingsAction> {
        if self.confirm_reset {
            match key.code {
                KeyCode::Esc => {
                    self.confirm_reset = false;
                    None
                }
                KeyCode::Left
                | KeyCode::Right
                | KeyCode::Tab
                | KeyCode::BackTab
                | KeyCode::Char('h')
                | KeyCode::Char('l') => {
                    self.modal_focus = if self.modal_focus == 0 { 1 } else { 0 };
                    None
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    self.confirm_reset = false;
                    if self.modal_focus == 0 {
                        self.flash("UI reset to defaults");
                        Some(SettingsAction::ResetUi)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.focused_row = if self.focused_row == 0 {
                        QUIT_ROW
                    } else {
                        self.focused_row - 1
                    };
                    None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.focused_row = if self.focused_row == QUIT_ROW {
                        0
                    } else {
                        self.focused_row + 1
                    };
                    None
                }
                KeyCode::Left | KeyCode::Char('h') => self.cycle_option(false),
                KeyCode::Right | KeyCode::Char('l') => self.cycle_option(true),
                KeyCode::Enter | KeyCode::Char(' ') => {
                    if self.focused_row == QUIT_ROW {
                        Some(SettingsAction::QuitMenu)
                    } else if self.focused_row == RESET_ROW {
                        self.confirm_reset = true;
                        self.modal_focus = 1;
                        None
                    } else {
                        self.cycle_option(true)
                    }
                }
                _ => None,
            }
        }
    }

    fn cycle_option(&mut self, next: bool) -> Option<SettingsAction> {
        match self.focused_row {
            0 => {
                let idx = COLS_OPTIONS
                    .iter()
                    .position(|&c| c == self.prefs.cols)
                    .unwrap_or(0);
                let new_idx = if next {
                    (idx + 1) % COLS_OPTIONS.len()
                } else {
                    (idx + COLS_OPTIONS.len() - 1) % COLS_OPTIONS.len()
                };
                self.prefs.cols = COLS_OPTIONS[new_idx];
                self.flash("Settings saved");
                Some(SettingsAction::Changed)
            }
            1 => {
                let idx = ROWS_OPTIONS
                    .iter()
                    .position(|&r| r == self.prefs.rows)
                    .unwrap_or(0);
                let new_idx = if next {
                    (idx + 1) % ROWS_OPTIONS.len()
                } else {
                    (idx + ROWS_OPTIONS.len() - 1) % ROWS_OPTIONS.len()
                };
                self.prefs.rows = ROWS_OPTIONS[new_idx];
                self.flash("Settings saved");
                Some(SettingsAction::Changed)
            }
            2 => {
                let idx = FILL_OPTIONS
                    .iter()
                    .position(|&f| f == self.prefs.fill)
                    .unwrap_or(0);
                let new_idx = if next {
                    (idx + 1) % FILL_OPTIONS.len()
                } else {
                    (idx + FILL_OPTIONS.len() - 1) % FILL_OPTIONS.len()
                };
                self.prefs.fill = FILL_OPTIONS[new_idx];
                self.flash("Settings saved");
                Some(SettingsAction::Changed)
            }
            3 => {
                let idx = SIDEBAR_OPTIONS
                    .iter()
                    .position(|&s| s == self.sidebar_mode)
                    .unwrap_or(0);
                let new_idx = if next {
                    (idx + 1) % SIDEBAR_OPTIONS.len()
                } else {
                    (idx + SIDEBAR_OPTIONS.len() - 1) % SIDEBAR_OPTIONS.len()
                };
                self.sidebar_mode = SIDEBAR_OPTIONS[new_idx];
                self.flash("Settings saved");
                Some(SettingsAction::Changed)
            }
            _ => None,
        }
    }

    pub fn handle_mouse(&mut self, mouse: &MouseEvent, area: Rect) -> Option<SettingsAction> {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return None;
        }

        if self.confirm_reset {
            let modal_w = 48u16.min(area.width);
            let modal_h = 5u16.min(area.height);
            let modal_x = area.x + (area.width.saturating_sub(modal_w)) / 2;
            let modal_y = area.y + (area.height.saturating_sub(modal_h)) / 2;

            if mouse.row == modal_y + 3 {
                if mouse.column >= modal_x + 14 && mouse.column <= modal_x + 24 {
                    self.confirm_reset = false;
                    self.flash("UI reset to defaults");
                    return Some(SettingsAction::ResetUi);
                } else if mouse.column >= modal_x + 26 && mouse.column <= modal_x + 38 {
                    self.confirm_reset = false;
                    return None;
                }
            }
            self.confirm_reset = false;
            return None;
        }

        let rel_y = mouse.row.saturating_sub(area.y);
        match rel_y {
            0 => {
                if mouse.column >= area.x + 14 && mouse.column <= area.x + 22 {
                    self.focused_row = QUIT_ROW;
                    Some(SettingsAction::QuitMenu)
                } else {
                    None
                }
            }
            1 => {
                self.focused_row = 0;
                self.cycle_option(true)
            }
            2 => {
                self.focused_row = 1;
                self.cycle_option(true)
            }
            3 => {
                self.focused_row = 2;
                self.cycle_option(true)
            }
            4 => {
                self.focused_row = 3;
                self.cycle_option(true)
            }
            6 => {
                self.focused_row = RESET_ROW;
                self.confirm_reset = true;
                self.modal_focus = 1;
                None
            }
            _ => None,
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let header_style = Style::default().add_modifier(Modifier::BOLD);
        buf.set_string(area.x, area.y, "Grid layout", header_style);

        if area.width >= 23 {
            let is_focused = self.focused_row == QUIT_ROW && !self.confirm_reset;
            let quit_style = if is_focused {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(Color::Yellow)
            };
            buf.set_string(area.x + 14, area.y, "[ Quit… ]", quit_style);

            if area.width >= 60 {
                let desc_style = Style::default().fg(Color::DarkGray);
                buf.set_string(
                    area.x + 25,
                    area.y,
                    "save, detach, update, discard",
                    desc_style,
                );
            }
        }

        let grid_labels = ["Columns", "Rows per page", "Fill order", "Sidebar"];
        for (idx, label) in grid_labels.iter().enumerate() {
            let row_y = area.y + 1 + idx as u16;
            if row_y >= area.y + area.height {
                break;
            }

            let is_focused = self.focused_row == idx && !self.confirm_reset;
            let prefix = if is_focused { "▶ " } else { "  " };

            let label_str = format!("{prefix}{label:<14} ");
            let label_style = if is_focused {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            buf.set_string(area.x, row_y, &label_str, label_style);

            let mut cur_x = area.x + label_str.len() as u16;

            match idx {
                0 => {
                    for opt in &COLS_OPTIONS {
                        let is_active = *opt == self.prefs.cols;
                        let name = match opt {
                            GridCols::Auto => "Auto".to_string(),
                            GridCols::Fixed(n) => n.to_string(),
                        };
                        cur_x = render_option_badge(buf, cur_x, row_y, &name, is_active);
                    }
                }
                1 => {
                    for opt in &ROWS_OPTIONS {
                        let is_active = *opt == self.prefs.rows;
                        let name = match opt {
                            GridRows::Auto => "Auto".to_string(),
                            GridRows::Fixed(n) => n.to_string(),
                        };
                        cur_x = render_option_badge(buf, cur_x, row_y, &name, is_active);
                    }
                }
                2 => {
                    for opt in &FILL_OPTIONS {
                        let is_active = *opt == self.prefs.fill;
                        let name = match opt {
                            FillOrder::RowFirst => "Row first".to_string(),
                            FillOrder::ColumnFirst => "Column first".to_string(),
                        };
                        cur_x = render_option_badge(buf, cur_x, row_y, &name, is_active);
                    }
                }
                3 => {
                    for opt in &SIDEBAR_OPTIONS {
                        let is_active = *opt == self.sidebar_mode;
                        let name = match opt {
                            SidebarMode::Auto => "Auto".to_string(),
                            SidebarMode::Wide => "Wide".to_string(),
                            SidebarMode::Compact => "Compact".to_string(),
                            SidebarMode::Hidden => "Hidden".to_string(),
                        };
                        cur_x = render_option_badge(buf, cur_x, row_y, &name, is_active);
                    }
                }
                _ => {}
            }
        }

        let reset_y = area.y + 6;
        if reset_y < area.y + area.height {
            let is_focused = self.focused_row == RESET_ROW && !self.confirm_reset;
            let reset_prefix = if is_focused { "▶ " } else { "  " };
            let reset_btn = format!(
                "{reset_prefix}[ Reset UI ]   restores layout, sizes, grid and sidebar to defaults"
            );
            let style = if is_focused {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(Color::Yellow)
            };
            buf.set_string(area.x, reset_y, &reset_btn, style);
        }

        let about_y = area.y + 8;
        if about_y < area.y + area.height {
            let ver = env!("CARGO_PKG_VERSION");
            let about = format!("tandem {ver} · github.com/hydrokat/agent-workspace ↗");
            let style = Style::default().fg(Color::DarkGray);
            buf.set_string(area.x, about_y, &about, style);
        }

        if let Some((ref msg, time)) = self.flash_message
            && time.elapsed().as_secs_f64() < 2.0
            && area.height > 1
        {
            let flash_y = area.y + area.height - 1;
            let flash_style = Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD);
            buf.set_string(area.x, flash_y, msg, flash_style);
        }

        if self.confirm_reset {
            render_modal(area, buf, self.modal_focus);
        }
    }
}

fn render_option_badge(buf: &mut Buffer, x: u16, y: u16, label: &str, is_active: bool) -> u16 {
    let bullet = if is_active { "•" } else { " " };
    let text = format!("({bullet}{label}) ");
    let style = if is_active {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    buf.set_string(x, y, &text, style);
    x + text.len() as u16
}

fn render_modal(area: Rect, buf: &mut Buffer, modal_focus: usize) {
    let width = 50u16.min(area.width);
    let height = 5u16.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;

    for r in y..y + height {
        for c in x..x + width {
            if let Some(cell) = buf.cell_mut((c, r)) {
                cell.set_symbol(" ");
                cell.set_style(Style::default().bg(Color::Black));
            }
        }
    }

    let top = format!(
        "┌ Reset UI {:─<width$}┐",
        "",
        width = (width as usize).saturating_sub(12)
    );
    buf.set_string(
        x,
        y,
        &top[..width as usize],
        Style::default().fg(Color::Yellow),
    );

    let prompt = "  Reset the UI to defaults? Agents keep running.";
    buf.set_string(
        x,
        y + 1,
        prompt,
        Style::default().add_modifier(Modifier::BOLD),
    );

    let reset_style = if modal_focus == 0 {
        Style::default()
            .add_modifier(Modifier::REVERSED)
            .fg(Color::Red)
    } else {
        Style::default().fg(Color::Red)
    };
    let cancel_style = if modal_focus == 1 {
        Style::default()
            .add_modifier(Modifier::REVERSED)
            .fg(Color::White)
    } else {
        Style::default().fg(Color::White)
    };

    buf.set_string(x + 14, y + 3, "[ Reset ]", reset_style);
    buf.set_string(x + 26, y + 3, "[ Cancel ]", cancel_style);

    let bottom = format!(
        "└{:─<width$}┘",
        "",
        width = (width as usize).saturating_sub(2)
    );
    buf.set_string(
        x,
        y + 4,
        &bottom[..width as usize],
        Style::default().fg(Color::Yellow),
    );
}
