use std::path::PathBuf;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::notice::{Notice, mark_seen};
use crate::tmux::client::TmuxClient;
use crate::tmux::keys::launcher_for;
use crate::update_check::{CheckState, read_state, write_state};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BannerAction {
    None,
    DismissUpdate,
    ShowWhatsNew,
    RunUpdate,
    Later,
    SkipVersion(String),
}

#[derive(Debug, Clone)]
pub struct UpdateBanner {
    pub notice: Notice,
    pub workspace: PathBuf,
    pub tmux: Option<TmuxClient>,
}

impl UpdateBanner {
    pub fn new(notice: Notice, workspace: PathBuf, tmux: Option<TmuxClient>) -> Self {
        Self {
            notice,
            workspace,
            tmux,
        }
    }

    pub fn click_at(&self, col: u16, row: u16, area: Rect) -> BannerAction {
        if row != area.y || col < area.x || col >= area.x + area.width {
            return BannerAction::None;
        }
        let dismiss_x = area.x + area.width.saturating_sub(3);
        if col >= dismiss_x {
            let _ = mark_seen(&self.workspace);
            BannerAction::DismissUpdate
        } else {
            if let Some(ref client) = self.tmux {
                let ws_str = self.workspace.to_string_lossy();
                let launcher = launcher_for(None, Some(&ws_str), None);
                let cmd = format!("{launcher} whats-new");
                let _ =
                    client.run_checked(&["display-popup", "-E", "-w", "70%", "-h", "70%", &cmd]);
            }
            BannerAction::ShowWhatsNew
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let text = format!(
            " ⬆ Updated to tandem {} — click for what's new",
            self.notice.to_version
        );
        let text_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        buf.set_stringn(area.x, area.y, &text, area.width as usize, text_style);

        if area.width >= 4 {
            let dismiss_x = area.x + area.width - 3;
            let dismiss_style = Style::default().fg(Color::DarkGray);
            buf.set_string(dismiss_x, area.y, "[×]", dismiss_style);
        }
    }
}

#[derive(Debug, Clone)]
pub struct OfferBanner {
    pub installed: String,
    pub latest: String,
    pub workspace: PathBuf,
    pub tmux: Option<TmuxClient>,
}

impl OfferBanner {
    pub fn new(
        installed: impl Into<String>,
        latest: impl Into<String>,
        workspace: PathBuf,
        tmux: Option<TmuxClient>,
    ) -> Self {
        Self {
            installed: installed.into(),
            latest: latest.into(),
            workspace,
            tmux,
        }
    }

    pub fn run_update_popup(&self) {
        if let Some(ref client) = self.tmux {
            let ws_str = self.workspace.to_string_lossy();
            let launcher = launcher_for(None, Some(&ws_str), None);
            let cmd = format!("{launcher} update --yes");
            let _ = client.run_checked(&["display-popup", "-E", "-w", "80%", "-h", "70%", &cmd]);
        }
    }

    pub fn click_at(&self, col: u16, row: u16, area: Rect) -> BannerAction {
        if row != area.y || col < area.x || col >= area.x + area.width {
            return BannerAction::None;
        }

        let buttons_width: u16 = 12 + 2 + 7 + 2 + 19; // [Update now]  [Later]  [Skip this version]
        if area.width > buttons_width {
            let start_x = area.x + area.width - buttons_width;
            let u_end = start_x + 12;
            let l_start = u_end + 2;
            let l_end = l_start + 7;
            let s_start = l_end + 2;
            let s_end = s_start + 19;

            if col >= start_x && col < u_end {
                self.run_update_popup();
                return BannerAction::RunUpdate;
            } else if col >= l_start && col < l_end {
                return BannerAction::Later;
            } else if col >= s_start && col < s_end {
                let state = read_state(&self.workspace);
                let _ = write_state(
                    &self.workspace,
                    &CheckState {
                        last: state.last,
                        latest: state.latest,
                        skip: self.latest.clone(),
                    },
                );
                return BannerAction::SkipVersion(self.latest.clone());
            }
        }
        BannerAction::None
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let text = format!(
            " ⬆ agent-workspace {} → {} available",
            self.installed, self.latest
        );
        let text_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        buf.set_stringn(area.x, area.y, &text, area.width as usize, text_style);

        let buttons_str = "[Update now]  [Later]  [Skip this version]";
        let b_len = buttons_str.chars().count() as u16;
        if area.width >= b_len + 10 {
            let b_x = area.x + area.width - b_len - 1;
            buf.set_string(
                b_x,
                area.y,
                "[Update now]",
                Style::default().fg(Color::Green),
            );
            buf.set_string(
                b_x + 14,
                area.y,
                "[Later]",
                Style::default().fg(Color::DarkGray),
            );
            buf.set_string(
                b_x + 23,
                area.y,
                "[Skip this version]",
                Style::default().fg(Color::DarkGray),
            );
        }
    }
}
