use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use std::path::PathBuf;
use std::time::Instant;

use crate::net_interfaces::IpCandidate;
use crate::status::qr::{render_qr_lines, required_size};
use crate::ui::text::truncate_to_width;
use crate::web_manager::{
    WebConfig, WebStatus, access_url, access_url_for_workspace, generate_auth_token,
};

pub const ROW_ENABLE: usize = 0;
pub const ROW_BIND: usize = 1;
pub const ROW_PORT: usize = 2;
pub const ROW_TERMINAL: usize = 3;
pub const ROW_TOKEN: usize = 4;
pub const ROW_CONNECT: usize = 5;
pub const ROW_REGENERATE: usize = 6;
const LAST_ROW: usize = ROW_REGENERATE;

const MIN_PORT: u16 = 1024;
const CONSENT_MODAL: (u16, u16) = (64, 7);
const REGEN_MODAL: (u16, u16) = (50, 5);
const DEPS_MODAL: (u16, u16) = (68, 7);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebAction {
    Changed,
    Regenerated,
    InstallDeps,
    ManualInstall,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepsPrompt {
    pub platform_label: String,
    pub missing: String,
    pub script_path: Option<PathBuf>,
    pub preview: String,
    pub focus: usize,
}

pub struct WebTab {
    pub focused_row: usize,
    pub web_config: WebConfig,
    pub ip_candidates: Vec<IpCandidate>,
    pub web_status: WebStatus,
    pub confirm_web_modal: bool,
    pub web_modal_focus: usize,
    pub confirm_regenerate: bool,
    pub regen_modal_focus: usize,
    pub token_revealed: bool,
    pub editing_port: Option<String>,
    pub qr_open: bool,
    pub deps_prompt: Option<DepsPrompt>,
    pub deps_help: Option<Vec<String>>,
    pub installing: bool,
    pub flash_message: Option<(String, Instant)>,
    pub workspace_socket: Option<String>,
}

impl Default for WebTab {
    fn default() -> Self {
        Self {
            focused_row: ROW_ENABLE,
            web_config: WebConfig::default(),
            ip_candidates: Vec::new(),
            web_status: WebStatus::Off,
            confirm_web_modal: false,
            web_modal_focus: 0,
            confirm_regenerate: false,
            regen_modal_focus: 1,
            token_revealed: false,
            editing_port: None,
            qr_open: false,
            deps_prompt: None,
            deps_help: None,
            installing: false,
            flash_message: None,
            workspace_socket: None,
        }
    }
}

fn is_loopback(ip: &str) -> bool {
    ip.starts_with("127.")
}

impl WebTab {
    pub fn set_web_config(&mut self, config: WebConfig) {
        self.web_config = config;
    }

    pub fn set_workspace_socket(&mut self, socket: Option<String>) {
        self.workspace_socket = socket;
    }

    pub fn set_ip_candidates(&mut self, candidates: Vec<IpCandidate>) {
        if !candidates.is_empty()
            && (!self.web_config.first_time_acknowledged
                || self.web_config.bind_ip.is_empty()
                || !candidates.iter().any(|c| c.ip == self.web_config.bind_ip))
        {
            let pick = candidates
                .iter()
                .find(|c| c.preferred)
                .or_else(|| candidates.first());
            if let Some(c) = pick {
                self.web_config.bind_ip = c.ip.clone();
                self.web_config.bind_source = c.source;
            }
        }
        self.ip_candidates = candidates;
    }

    pub fn flash(&mut self, msg: &str) {
        self.flash_message = Some((msg.to_string(), Instant::now()));
    }

    pub fn captures_input(&self) -> bool {
        self.editing_port.is_some()
            || self.confirm_web_modal
            || self.confirm_regenerate
            || self.qr_open
            || self.deps_prompt.is_some()
            || self.deps_help.is_some()
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> Option<WebAction> {
        if self.deps_help.is_some() {
            self.deps_help = None;
            return None;
        }
        if self.deps_prompt.is_some() {
            return self.handle_deps_key(key);
        }
        if self.qr_open {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('q')
            ) {
                self.qr_open = false;
            }
            return None;
        }
        if self.confirm_web_modal {
            return self.handle_consent_key(key);
        }
        if self.confirm_regenerate {
            return self.handle_regenerate_key(key);
        }
        if self.editing_port.is_some() {
            return self.handle_port_key(key);
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.focused_row = if self.focused_row == 0 {
                    LAST_ROW
                } else {
                    self.focused_row - 1
                };
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.focused_row = if self.focused_row == LAST_ROW {
                    0
                } else {
                    self.focused_row + 1
                };
                None
            }
            KeyCode::Left | KeyCode::Char('h') => self.cycle_option(false),
            KeyCode::Right | KeyCode::Char('l') => self.cycle_option(true),
            KeyCode::Enter | KeyCode::Char(' ') => self.activate(),
            _ => None,
        }
    }

    fn handle_deps_key(&mut self, key: &KeyEvent) -> Option<WebAction> {
        let prompt = self.deps_prompt.as_mut()?;
        match key.code {
            KeyCode::Esc => {
                self.deps_prompt = None;
                None
            }
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Char('h')
            | KeyCode::Char('l') => {
                prompt.focus = 1 - prompt.focus;
                None
            }
            KeyCode::Enter | KeyCode::Char(' ') => Some(if prompt.focus == 0 {
                WebAction::InstallDeps
            } else {
                WebAction::ManualInstall
            }),
            _ => None,
        }
    }

    fn handle_consent_key(&mut self, key: &KeyEvent) -> Option<WebAction> {
        match key.code {
            KeyCode::Esc => {
                self.confirm_web_modal = false;
                None
            }
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Char('h')
            | KeyCode::Char('l') => {
                self.web_modal_focus = 1 - self.web_modal_focus;
                None
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.confirm_web_modal = false;
                if self.web_modal_focus == 0 {
                    self.accept_consent()
                } else {
                    self.web_config.enabled = false;
                    None
                }
            }
            _ => None,
        }
    }

    fn accept_consent(&mut self) -> Option<WebAction> {
        self.web_config.first_time_acknowledged = true;
        self.web_config.enabled = true;
        self.flash("tandem-web enabled");
        Some(WebAction::Changed)
    }

    fn handle_regenerate_key(&mut self, key: &KeyEvent) -> Option<WebAction> {
        match key.code {
            KeyCode::Esc => {
                self.confirm_regenerate = false;
                None
            }
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Char('h')
            | KeyCode::Char('l') => {
                self.regen_modal_focus = 1 - self.regen_modal_focus;
                None
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.confirm_regenerate = false;
                if self.regen_modal_focus == 0 {
                    self.regenerate()
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn regenerate(&mut self) -> Option<WebAction> {
        self.web_config.auth_token = generate_auth_token();
        self.flash("New token issued; old links no longer work");
        Some(WebAction::Regenerated)
    }

    fn handle_port_key(&mut self, key: &KeyEvent) -> Option<WebAction> {
        match key.code {
            KeyCode::Esc => {
                self.editing_port = None;
                None
            }
            KeyCode::Backspace => {
                if let Some(buf) = self.editing_port.as_mut() {
                    buf.pop();
                }
                None
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                if let Some(buf) = self.editing_port.as_mut()
                    && buf.len() < 5
                {
                    buf.push(c);
                }
                None
            }
            KeyCode::Enter => self.commit_port(),
            _ => None,
        }
    }

    fn commit_port(&mut self) -> Option<WebAction> {
        let raw = self.editing_port.clone().unwrap_or_default();
        match raw.parse::<u16>() {
            Ok(port) if port >= MIN_PORT => {
                self.editing_port = None;
                if port == self.web_config.port {
                    return None;
                }
                self.web_config.port = port;
                self.flash("Port updated");
                Some(WebAction::Changed)
            }
            _ => {
                self.flash("Port must be between 1024 and 65535");
                None
            }
        }
    }

    fn toggle_enabled(&mut self) -> Option<WebAction> {
        if self.installing {
            self.flash("Installing dependencies, please wait");
            return None;
        }
        if self.web_config.enabled {
            self.web_config.enabled = false;
            self.flash("tandem-web disabled");
            return Some(WebAction::Changed);
        }
        if !self.web_config.first_time_acknowledged {
            self.confirm_web_modal = true;
            self.web_modal_focus = 0;
            return None;
        }
        self.web_config.enabled = true;
        self.flash("tandem-web enabled");
        Some(WebAction::Changed)
    }

    fn cycle_bind(&mut self, next: bool) -> Option<WebAction> {
        let len = self.ip_candidates.len();
        if len == 0 {
            return None;
        }
        let idx = self
            .ip_candidates
            .iter()
            .position(|c| c.ip == self.web_config.bind_ip)
            .unwrap_or(0);
        let new_idx = if next {
            (idx + 1) % len
        } else {
            (idx + len - 1) % len
        };
        let candidate = &self.ip_candidates[new_idx];
        self.web_config.bind_ip = candidate.ip.clone();
        self.web_config.bind_source = candidate.source;
        self.flash("Bind IP updated");
        Some(WebAction::Changed)
    }

    fn toggle_terminal(&mut self) -> Option<WebAction> {
        self.web_config.ttyd_enabled = !self.web_config.ttyd_enabled;
        self.flash(if self.web_config.ttyd_enabled {
            "Terminal attach enabled"
        } else {
            "Terminal attach disabled"
        });
        Some(WebAction::Changed)
    }

    fn open_connect(&mut self) -> Option<WebAction> {
        if !matches!(self.web_status, WebStatus::Running { .. }) {
            self.flash("Enable web first, then scan the QR code");
            return None;
        }
        if is_loopback(&self.web_config.bind_ip) {
            self.flash("Bound to localhost: pick a VPN or LAN IP to connect a phone");
            return None;
        }
        self.qr_open = true;
        None
    }

    fn cycle_option(&mut self, next: bool) -> Option<WebAction> {
        match self.focused_row {
            ROW_ENABLE => self.toggle_enabled(),
            ROW_BIND => self.cycle_bind(next),
            ROW_TERMINAL => self.toggle_terminal(),
            _ => None,
        }
    }

    fn activate(&mut self) -> Option<WebAction> {
        match self.focused_row {
            ROW_ENABLE => self.toggle_enabled(),
            ROW_BIND => self.cycle_bind(true),
            ROW_PORT => {
                self.editing_port = Some(self.web_config.port.to_string());
                None
            }
            ROW_TERMINAL => self.toggle_terminal(),
            ROW_TOKEN => {
                self.token_revealed = !self.token_revealed;
                None
            }
            ROW_CONNECT => self.open_connect(),
            ROW_REGENERATE => {
                self.confirm_regenerate = true;
                self.regen_modal_focus = 1;
                None
            }
            _ => None,
        }
    }

    pub fn handle_mouse(&mut self, mouse: &MouseEvent, area: Rect) -> Option<WebAction> {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return None;
        }

        if self.deps_help.is_some() {
            self.deps_help = None;
            return None;
        }

        if let Some(prompt) = self.deps_prompt.as_ref() {
            let _ = prompt;
            let (x, y) = modal_origin(area, DEPS_MODAL);
            if mouse.row == y + 5 {
                if (x + 3..=x + 24).contains(&mouse.column) {
                    return Some(WebAction::InstallDeps);
                } else if (x + 28..=x + 53).contains(&mouse.column) {
                    return Some(WebAction::ManualInstall);
                }
            }
            self.deps_prompt = None;
            return None;
        }

        if self.qr_open {
            self.qr_open = false;
            return None;
        }

        if self.confirm_web_modal {
            let (x, y) = modal_origin(area, CONSENT_MODAL);
            self.confirm_web_modal = false;
            if mouse.row == y + 5 {
                if (x + 12..=x + 34).contains(&mouse.column) {
                    return self.accept_consent();
                } else if (x + 36..=x + 48).contains(&mouse.column) {
                    self.web_config.enabled = false;
                }
            }
            return None;
        }

        if self.confirm_regenerate {
            let (x, y) = modal_origin(area, REGEN_MODAL);
            self.confirm_regenerate = false;
            if mouse.row == y + 3 && (x + 8..=x + 24).contains(&mouse.column) {
                return self.regenerate();
            }
            return None;
        }

        self.editing_port = None;
        let rel_y = mouse.row.checked_sub(area.y)? as usize;
        if rel_y > LAST_ROW {
            return None;
        }
        self.focused_row = rel_y;
        self.activate()
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        if self.qr_open {
            self.render_qr(area, buf);
            return;
        }

        let modal_open = self.confirm_web_modal
            || self.confirm_regenerate
            || self.deps_prompt.is_some()
            || self.deps_help.is_some();
        for row in 0..=LAST_ROW {
            let y = area.y + row as u16;
            if y >= area.y + area.height {
                break;
            }
            self.render_row(row, area, y, buf, !modal_open);
        }

        if let Some((ref msg, time)) = self.flash_message
            && time.elapsed().as_secs_f64() < 2.0
            && area.height > LAST_ROW as u16 + 1
        {
            put(
                buf,
                area.x,
                area.y + area.height - 1,
                area.right(),
                msg,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            );
        }

        if self.deps_help.is_some() {
            self.render_deps_help(area, buf);
        } else if self.deps_prompt.is_some() {
            self.render_deps_prompt(area, buf);
        } else if self.confirm_web_modal {
            self.render_consent_modal(area, buf);
        } else if self.confirm_regenerate {
            self.render_regenerate_modal(area, buf);
        }
    }

    fn render_row(&self, row: usize, area: Rect, y: u16, buf: &mut Buffer, focus_allowed: bool) {
        let right = area.right();
        let focused = focus_allowed && self.focused_row == row;
        let label = match row {
            ROW_ENABLE => "Enable web",
            ROW_BIND => "Bind IP",
            ROW_PORT => "Port",
            ROW_TERMINAL => "Terminal",
            ROW_TOKEN => "Token",
            ROW_CONNECT => "Connect",
            _ => "Regenerate",
        };
        let prefix = if focused { "▶ " } else { "  " };
        let label_style = if focused {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let head = format!("{prefix}{label:<12} ");
        let mut x = put(buf, area.x, y, right, &head, label_style);

        match row {
            ROW_ENABLE => {
                x = badge(buf, x, y, right, "Off", !self.web_config.enabled);
                x = badge(buf, x, y, right, "On", self.web_config.enabled);
                let (text, color) = match &self.web_status {
                    _ if self.installing => ("  ● Installing dependencies…", Color::Yellow),
                    WebStatus::Running { .. } => ("  ● Running", Color::Green),
                    WebStatus::Starting => ("  ● Starting", Color::Yellow),
                    WebStatus::Error(_) => ("  ● Error", Color::Red),
                    WebStatus::Off if self.web_config.enabled => ("  ● Configured", Color::Cyan),
                    WebStatus::Off => ("  ● Off", Color::DarkGray),
                };
                put(buf, x, y, right, text, Style::default().fg(color));
            }
            ROW_BIND => {
                let text = if self.ip_candidates.is_empty() {
                    format!(
                        "{} [{}]  (no network interfaces found)",
                        self.web_config.bind_ip,
                        self.web_config.bind_source.as_str()
                    )
                } else {
                    format!(
                        "← (•{} [{}]) →",
                        self.web_config.bind_ip,
                        self.web_config.bind_source.as_str()
                    )
                };
                put(
                    buf,
                    x,
                    y,
                    right,
                    &text,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                );
            }
            ROW_PORT => match &self.editing_port {
                Some(edit) => {
                    let text = format!("{edit}_   Enter save · Esc cancel · 1024-65535");
                    put(
                        buf,
                        x,
                        y,
                        right,
                        &text,
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    );
                }
                None => {
                    let text = format!("{}   Enter to edit", self.web_config.port);
                    put(buf, x, y, right, &text, Style::default().fg(Color::White));
                }
            },
            ROW_TERMINAL => {
                x = badge(buf, x, y, right, "Off", !self.web_config.ttyd_enabled);
                x = badge(buf, x, y, right, "On", self.web_config.ttyd_enabled);
                put(
                    buf,
                    x,
                    y,
                    right,
                    " ttyd attach from the web UI",
                    Style::default().fg(Color::DarkGray),
                );
            }
            ROW_TOKEN => {
                let (shown, hint) = if self.token_revealed {
                    (self.web_config.auth_token.clone(), "  Enter to hide")
                } else {
                    ("tandem_••••••••••••••••".to_string(), "  Enter to reveal")
                };
                x = put(buf, x, y, right, &shown, Style::default().fg(Color::White));
                put(buf, x, y, right, hint, Style::default().fg(Color::DarkGray));
            }
            ROW_CONNECT => match &self.web_status {
                WebStatus::Error(msg) => {
                    put(buf, x, y, right, msg, Style::default().fg(Color::Red));
                }
                WebStatus::Running { .. } => {
                    let url = access_url(&self.web_config, false);
                    x = put(buf, x, y, right, &url, Style::default().fg(Color::Green));
                    let style = if focused {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default().fg(Color::Cyan)
                    };
                    put(buf, x + 2, y, right, "[ Show QR ]", style);
                }
                _ => {
                    put(
                        buf,
                        x,
                        y,
                        right,
                        "Enable web to connect a phone",
                        Style::default().fg(Color::DarkGray),
                    );
                }
            },
            _ => {
                let style = if focused {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default().fg(Color::Yellow)
                };
                x = put(buf, x, y, right, "[ Regenerate token… ]", style);
                put(
                    buf,
                    x,
                    y,
                    right,
                    "  invalidates the old URL and QR",
                    Style::default().fg(Color::DarkGray),
                );
            }
        }
    }

    fn render_qr(&self, area: Rect, buf: &mut Buffer) {
        let url = access_url_for_workspace(&self.web_config, self.workspace_socket.as_deref());
        let (w, h) = required_size(&url);
        if w == 0 || area.width < w || area.height < h {
            let msg = format!("QR needs {w}x{h}, pane is {}x{}", area.width, area.height);
            put(
                buf,
                area.x,
                area.y,
                area.right(),
                &msg,
                Style::default().fg(Color::Yellow),
            );
            put(
                buf,
                area.x,
                area.y + 1,
                area.right(),
                "Enlarge the status pane. Esc to close.",
                Style::default().fg(Color::DarkGray),
            );
            return;
        }

        let qr_style = Style::default().fg(Color::Black).bg(Color::White);
        let x0 = area.x + (area.width - w) / 2;
        for (i, line) in render_qr_lines(&url).iter().enumerate() {
            let y = area.y + i as u16;
            if y >= area.y + area.height {
                break;
            }
            put(buf, x0, y, area.right(), line, qr_style);
        }

        let caption_y = area.y + h;
        if caption_y < area.y + area.height {
            put(
                buf,
                area.x,
                caption_y,
                area.right(),
                "Scan with your phone camera · Esc to close",
                Style::default().fg(Color::DarkGray),
            );
        }
    }

    fn render_consent_modal(&self, area: Rect, buf: &mut Buffer) {
        let (x, y, w, h) = modal_rect(area, CONSENT_MODAL);
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let bind_line = format!(
            "  Bind IP: {} [{}] · Port: {}",
            self.web_config.bind_ip,
            self.web_config.bind_source.as_str(),
            self.web_config.port
        );
        let lines = [
            (
                "  Turning on tandem-web-core-service on this host may install",
                bold,
            ),
            (
                "  additional dependencies (bun, tandem-web packages, ttyd).",
                bold,
            ),
            (bind_line.as_str(), Style::default().fg(Color::Yellow)),
        ];
        draw_modal(
            buf,
            (x, y, w, h),
            "Enable tandem-web-core-service (First-Time Setup)",
            Color::Cyan,
            &lines,
        );
        modal_button(
            buf,
            x + 14,
            y + 5,
            x + w,
            "[ Install & Enable ]",
            Color::Green,
            self.web_modal_focus == 0,
        );
        modal_button(
            buf,
            x + 38,
            y + 5,
            x + w,
            "[ Cancel ]",
            Color::White,
            self.web_modal_focus == 1,
        );
    }

    fn render_deps_prompt(&self, area: Rect, buf: &mut Buffer) {
        let Some(prompt) = self.deps_prompt.as_ref() else {
            return;
        };
        let (x, y, w, h) = modal_rect(area, DEPS_MODAL);
        let script = prompt
            .script_path
            .as_ref()
            .map_or("(none)".to_string(), |p| {
                shorten_left(&p.display().to_string(), (w as usize).saturating_sub(14))
            });
        let line1 = format!(
            "  tandem-web needs {}, which is not installed for {}.",
            prompt.missing, prompt.platform_label
        );
        let line2 = format!("  Script: {script}");
        let line3 = format!("  Runs:   {}", prompt.preview);
        let lines = [
            (
                line1.as_str(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            (line2.as_str(), Style::default().fg(Color::Yellow)),
            (line3.as_str(), Style::default().fg(Color::DarkGray)),
        ];
        draw_modal(
            buf,
            (x, y, w, h),
            "Dependencies needed",
            Color::Cyan,
            &lines,
        );
        modal_button(
            buf,
            x + 3,
            y + 5,
            x + w,
            "[ Let tandem install ]",
            Color::Green,
            prompt.focus == 0,
        );
        modal_button(
            buf,
            x + 28,
            y + 5,
            x + w,
            "[ I'll install it myself ]",
            Color::White,
            prompt.focus == 1,
        );
    }

    fn render_deps_help(&self, area: Rect, buf: &mut Buffer) {
        let Some(help) = self.deps_help.as_ref() else {
            return;
        };
        let width = 96u16.min(area.width);
        let height = ((help.len() + 3) as u16).min(area.height);
        let x = area.x + area.width.saturating_sub(width) / 2;
        let y = area.y + area.height.saturating_sub(height) / 2;
        let mut lines: Vec<(String, Style)> = help
            .iter()
            .map(|l| (format!("  {l}"), Style::default()))
            .collect();
        lines.push((
            "  Press any key to close".to_string(),
            Style::default().fg(Color::DarkGray),
        ));
        let refs: Vec<(&str, Style)> = lines.iter().map(|(t, s)| (t.as_str(), *s)).collect();
        draw_modal(
            buf,
            (x, y, width, height),
            "Install dependencies yourself",
            Color::Yellow,
            &refs,
        );
    }

    fn render_regenerate_modal(&self, area: Rect, buf: &mut Buffer) {
        let (x, y, w, h) = modal_rect(area, REGEN_MODAL);
        let lines = [(
            "  Issue a new token? Old URL and QR stop working.",
            Style::default().add_modifier(Modifier::BOLD),
        )];
        draw_modal(buf, (x, y, w, h), "Regenerate token", Color::Yellow, &lines);
        modal_button(
            buf,
            x + 10,
            y + 3,
            x + w,
            "[ Regenerate ]",
            Color::Red,
            self.regen_modal_focus == 0,
        );
        modal_button(
            buf,
            x + 26,
            y + 3,
            x + w,
            "[ Cancel ]",
            Color::White,
            self.regen_modal_focus == 1,
        );
    }
}

fn shorten_left(text: &str, max_cols: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_cols || max_cols == 0 {
        return text.to_string();
    }
    let keep = max_cols.saturating_sub(1);
    format!(
        "…{}",
        chars[chars.len() - keep..].iter().collect::<String>()
    )
}

fn modal_origin(area: Rect, size: (u16, u16)) -> (u16, u16) {
    let (x, y, _, _) = modal_rect(area, size);
    (x, y)
}

fn modal_rect(area: Rect, size: (u16, u16)) -> (u16, u16, u16, u16) {
    let w = size.0.min(area.width);
    let h = size.1.min(area.height);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    (x, y, w, h)
}

fn put(buf: &mut Buffer, x: u16, y: u16, right: u16, text: &str, style: Style) -> u16 {
    let right = right.min(buf.area.right());
    if x >= right || y < buf.area.y || y >= buf.area.bottom() {
        return x;
    }
    let clipped = truncate_to_width(text, (right - x) as usize);
    buf.set_string(x, y, clipped, style);
    x + unicode_width::UnicodeWidthStr::width(clipped) as u16
}

fn badge(buf: &mut Buffer, x: u16, y: u16, right: u16, label: &str, active: bool) -> u16 {
    let bullet = if active { "•" } else { " " };
    let style = if active {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    put(buf, x, y, right, &format!("({bullet}{label}) "), style)
}

fn draw_modal(
    buf: &mut Buffer,
    rect: (u16, u16, u16, u16),
    title: &str,
    border: Color,
    lines: &[(&str, Style)],
) {
    let (x, y, w, h) = rect;
    if w < 4 || h < 3 {
        return;
    }
    let right = x + w;
    for r in y..y + h {
        for c in x..right {
            if let Some(cell) = buf.cell_mut((c, r)) {
                cell.set_symbol(" ");
                cell.set_style(Style::default().bg(Color::Black));
            }
        }
    }
    let border_style = Style::default().fg(border);
    let title_part = format!("┌ {title} ");
    let fill = (w as usize).saturating_sub(title_part.chars().count() + 1);
    let top = format!("{title_part}{}┐", "─".repeat(fill));
    put(buf, x, y, right, &top, border_style);
    for (i, (text, style)) in lines.iter().enumerate() {
        let ly = y + 1 + i as u16;
        if ly < y + h - 1 {
            put(buf, x, ly, right, text, *style);
        }
    }
    let bottom = format!("└{}┘", "─".repeat((w as usize).saturating_sub(2)));
    put(buf, x, y + h - 1, right, &bottom, border_style);
}

fn modal_button(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    right: u16,
    label: &str,
    color: Color,
    focused: bool,
) {
    let style = if focused {
        Style::default().add_modifier(Modifier::REVERSED).fg(color)
    } else {
        Style::default().fg(color)
    };
    put(buf, x, y, right, label, style);
}
