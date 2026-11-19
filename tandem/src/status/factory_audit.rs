use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use unicode_width::UnicodeWidthStr;

use crate::factory::audit::{
    self, AuditEntry, AuditFilter, AuditKind, AuditVerifyReport, export_markdown,
    format_time_short, format_verify_summary,
};
use crate::factory::store::FactoryStore;
use crate::sanitize::clean;
use crate::ui::text::truncate_to_width;

const WIDE_BREAKPOINT: u16 = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditKeyOutcome {
    Ignored,
    Consumed,
    BackToDetail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindFilter {
    All,
    JobStart,
    JobEnd,
    Approval,
    Decision,
    Lifecycle,
}

impl KindFilter {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::JobStart,
            Self::JobStart => Self::JobEnd,
            Self::JobEnd => Self::Approval,
            Self::Approval => Self::Decision,
            Self::Decision => Self::Lifecycle,
            Self::Lifecycle => Self::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::JobStart => "Job start",
            Self::JobEnd => "Job end",
            Self::Approval => "Approval",
            Self::Decision => "Decision",
            Self::Lifecycle => "Lifecycle",
        }
    }

    pub fn as_audit_kind(self) -> Option<AuditKind> {
        match self {
            Self::All => None,
            Self::JobStart => Some(AuditKind::JobStart),
            Self::JobEnd => Some(AuditKind::JobEnd),
            Self::Approval => Some(AuditKind::Approval),
            Self::Decision => Some(AuditKind::Decision),
            Self::Lifecycle => Some(AuditKind::Lifecycle),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FactoryAudit {
    pub idea_id: String,
    pub workspace: PathBuf,
    pub all_entries: Vec<AuditEntry>,
    pub filtered_entries: Vec<AuditEntry>,
    pub selected: usize,
    pub list_scroll: usize,
    pub kind_filter: KindFilter,
    pub actor_filter: Option<String>,
    pub verify_report: Option<AuditVerifyReport>,
    pub narrow_detail_open: bool,
    pub notice: String,
    pub error: Option<String>,
}

impl FactoryAudit {
    pub fn load(workspace: &Path, idea_id: &str) -> Self {
        let mut view = Self {
            idea_id: idea_id.to_string(),
            workspace: workspace.to_path_buf(),
            all_entries: Vec::new(),
            filtered_entries: Vec::new(),
            selected: 0,
            list_scroll: 0,
            kind_filter: KindFilter::All,
            actor_filter: None,
            verify_report: None,
            narrow_detail_open: false,
            notice: String::new(),
            error: None,
        };
        view.reload();
        view
    }

    pub fn reload(&mut self) {
        let store = FactoryStore::new(&self.workspace);
        match audit::entries(&store, &self.idea_id, &AuditFilter::default()) {
            Ok(items) => {
                self.all_entries = items;
                self.error = None;
                self.apply_filters();
            }
            Err(err) => {
                self.error = Some(clean(&err.to_string()));
                self.all_entries.clear();
                self.filtered_entries.clear();
            }
        }
        if let Ok(rep) =
            audit::verify(&self.workspace.join(".agent-workspace/factory/events.jsonl"))
        {
            self.verify_report = Some(rep);
        }
    }

    fn apply_filters(&mut self) {
        let kind = self.kind_filter.as_audit_kind();
        let actor = self.actor_filter.as_deref();
        self.filtered_entries = self
            .all_entries
            .iter()
            .filter(|e| kind.is_none_or(|k| e.kind == k))
            .filter(|e| actor.is_none_or(|a| e.actor.eq_ignore_ascii_case(a)))
            .cloned()
            .collect();
        if self.selected >= self.filtered_entries.len() {
            self.selected = self.filtered_entries.len().saturating_sub(1);
        }
        self.list_scroll = self.list_scroll.min(self.selected);
    }

    fn cycle_actor(&mut self) {
        let actors: Vec<String> = self
            .all_entries
            .iter()
            .map(|e| e.actor.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if actors.is_empty() {
            self.actor_filter = None;
            return;
        }
        self.actor_filter = match &self.actor_filter {
            None => Some(actors[0].clone()),
            Some(cur) => {
                let idx = actors.iter().position(|a| a == cur);
                match idx {
                    Some(i) if i + 1 < actors.len() => Some(actors[i + 1].clone()),
                    _ => None,
                }
            }
        };
        self.apply_filters();
    }

    pub fn handle_key(&mut self, event: KeyEvent) -> AuditKeyOutcome {
        match event.code {
            KeyCode::Esc => {
                if self.narrow_detail_open {
                    self.narrow_detail_open = false;
                    AuditKeyOutcome::Consumed
                } else {
                    AuditKeyOutcome::BackToDetail
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                    if self.selected < self.list_scroll {
                        self.list_scroll = self.selected;
                    }
                }
                AuditKeyOutcome::Consumed
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < self.filtered_entries.len() {
                    self.selected += 1;
                }
                AuditKeyOutcome::Consumed
            }
            KeyCode::Enter => {
                if !self.filtered_entries.is_empty() {
                    self.narrow_detail_open = true;
                }
                AuditKeyOutcome::Consumed
            }
            KeyCode::Char('f') => {
                self.kind_filter = self.kind_filter.next();
                self.apply_filters();
                AuditKeyOutcome::Consumed
            }
            KeyCode::Char('a') => {
                self.cycle_actor();
                AuditKeyOutcome::Consumed
            }
            KeyCode::Char('v') => {
                let events_path = self.workspace.join(".agent-workspace/factory/events.jsonl");
                match audit::verify(&events_path) {
                    Ok(rep) => {
                        self.notice = format_verify_summary(&rep);
                        self.verify_report = Some(rep);
                    }
                    Err(err) => {
                        self.notice = clean(&err.to_string());
                    }
                }
                AuditKeyOutcome::Consumed
            }
            KeyCode::Char('x') => {
                let store = FactoryStore::new(&self.workspace);
                let _ = store.ensure();
                let md = export_markdown(
                    &self.idea_id,
                    &self.filtered_entries,
                    self.verify_report.as_ref(),
                );
                let out_path = store.root.join(format!("audit-{}.md", self.idea_id));
                match fs::write(&out_path, md) {
                    Ok(()) => {
                        self.notice = format!("Exported markdown to {}", out_path.display());
                    }
                    Err(err) => {
                        self.notice = format!("Export failed: {}", clean(&err.to_string()));
                    }
                }
                AuditKeyOutcome::Consumed
            }
            _ => AuditKeyOutcome::Ignored,
        }
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let is_wide = area.width >= WIDE_BREAKPOINT;

        let filter_label = format!(
            "filter: {}{}",
            self.kind_filter.label(),
            self.actor_filter
                .as_ref()
                .map(|a| format!(" ({a})"))
                .unwrap_or_default()
        );
        let header = format!(
            "◀ Idea   Audit · {}   {}   v verify   x export",
            self.idea_id, filter_label
        );
        let header_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        put_line(buf, area.x, area.y, area.width, &header, header_style);

        let mut y = area.y + 1;
        let footer_y = area.y + area.height.saturating_sub(1);

        if let Some(ref rep) = self.verify_report
            && !rep.intact
            && y < footer_y
        {
            let warn = format_verify_summary(rep);
            put_line(
                buf,
                area.x,
                y,
                area.width,
                &warn,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
            y += 1;
        }

        if let Some(ref err) = self.error {
            if y < footer_y {
                put_line(
                    buf,
                    area.x,
                    y,
                    area.width,
                    err,
                    Style::default().fg(Color::Red),
                );
            }
            self.render_footer(area, footer_y, buf, is_wide);
            return;
        }

        if is_wide {
            let body_h = footer_y.saturating_sub(y);
            let list_h = (body_h / 2).max(3).min(body_h);
            let detail_y = y + list_h;
            self.render_list(Rect::new(area.x, y, area.width, list_h), buf);
            if detail_y < footer_y {
                let sep = "─".repeat(area.width as usize);
                put_line(
                    buf,
                    area.x,
                    detail_y,
                    area.width,
                    &sep,
                    Style::default().fg(Color::DarkGray),
                );
            }
            if detail_y + 1 < footer_y {
                let detail_h = footer_y.saturating_sub(detail_y + 1);
                self.render_detail(Rect::new(area.x, detail_y + 1, area.width, detail_h), buf);
            }
        } else if self.narrow_detail_open {
            let body_h = footer_y.saturating_sub(y);
            self.render_detail(Rect::new(area.x, y, area.width, body_h), buf);
        } else {
            let body_h = footer_y.saturating_sub(y);
            self.render_list(Rect::new(area.x, y, area.width, body_h), buf);
        }

        self.render_footer(area, footer_y, buf, is_wide);
    }

    fn render_list(&mut self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        if self.filtered_entries.is_empty() {
            put_line(
                buf,
                area.x,
                area.y,
                area.width,
                "No audit entries match the current filter.",
                Style::default().fg(Color::DarkGray),
            );
            return;
        }
        let max_rows = area.height as usize;
        if self.selected >= self.list_scroll + max_rows {
            self.list_scroll = self.selected + 1 - max_rows;
        }
        for (row_idx, entry) in self
            .filtered_entries
            .iter()
            .skip(self.list_scroll)
            .take(max_rows)
            .enumerate()
        {
            let abs_idx = self.list_scroll + row_idx;
            let is_sel = abs_idx == self.selected;
            let marker = if is_sel { "▶" } else { " " };
            let time = format_time_short(entry.ts);
            let line = format!(
                "{marker}{time}  {:<12}  {}",
                truncate_to_width(&clean(&entry.actor), 12),
                clean(&entry.summary)
            );
            let style = if is_sel {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            put_line(
                buf,
                area.x,
                area.y + row_idx as u16,
                area.width,
                &line,
                style,
            );
        }
    }

    fn render_detail(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        let Some(entry) = self.filtered_entries.get(self.selected) else {
            return;
        };
        let mut y = area.y;
        let max_y = area.y + area.height;

        let title = match (&entry.stage, entry.attempt, &entry.correlation_id) {
            (Some(st), Some(att), Some(corr)) => format!("{st} · attempt {att} · {corr}"),
            _ => clean(&entry.summary),
        };
        put_line(
            buf,
            area.x,
            y,
            area.width,
            &title,
            Style::default().add_modifier(Modifier::BOLD),
        );
        y += 1;

        for item in &entry.details {
            if y >= max_y {
                break;
            }
            let row = format!("{:<10} {}", clean(&item.label), clean(&item.value));
            put_line(buf, area.x, y, area.width, &row, Style::default());
            y += 1;
        }
    }

    fn render_footer(&self, area: Rect, footer_y: u16, buf: &mut Buffer, is_wide: bool) {
        let text = if !self.notice.is_empty() {
            clean(&self.notice)
        } else if is_wide {
            "↑/↓ select  f filter kind  a filter actor  v verify  x export  Esc back".to_string()
        } else if self.narrow_detail_open {
            "Esc back to list".to_string()
        } else {
            "↑/↓ select  Enter detail  f filter  a actor  v verify  x export  Esc back".to_string()
        };
        put_line(
            buf,
            area.x,
            footer_y,
            area.width,
            &text,
            Style::default().fg(Color::DarkGray),
        );
    }
}

fn put_line(buf: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    let clipped = truncate_to_width(text, width as usize);
    buf.set_string(x, y, clipped, style);
    let used = clipped.width() as u16;
    if used < width && style.bg.is_some() {
        let pad = " ".repeat((width - used) as usize);
        buf.set_string(x + used, y, &pad, style);
    }
}
