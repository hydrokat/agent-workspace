use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use unicode_width::UnicodeWidthStr;

use crate::factory::model::Risk;
use crate::sanitize::clean;
use crate::snapshot::{FactorySummary, IdeaCard, StatusSnapshot};
use crate::status::render::age;
use crate::ui::text::truncate_to_width;

#[derive(Debug, Clone, Default)]
pub struct FactoryTab {
    pub selected: Option<usize>,
    pub selected_id: Option<String>,
    pub scroll_offset: usize,
}

impl FactoryTab {
    pub fn sync(&mut self, summary: Option<&FactorySummary>) {
        let Some(summary) = summary else {
            self.selected = None;
            self.selected_id = None;
            self.scroll_offset = 0;
            return;
        };
        if summary.error.is_some() || summary.ideas.is_empty() {
            self.selected = None;
            self.selected_id = None;
            self.scroll_offset = 0;
            return;
        }
        if let Some(ref target_id) = self.selected_id
            && let Some(pos) = summary.ideas.iter().position(|idea| &idea.id == target_id)
        {
            self.selected = Some(pos);
            return;
        }
        self.selected = Some(0);
        self.selected_id = Some(summary.ideas[0].id.clone());
    }

    pub fn select_next(&mut self, ideas: &[IdeaCard]) {
        if ideas.is_empty() {
            return;
        }
        let next = match self.selected {
            Some(idx) => (idx + 1).min(ideas.len() - 1),
            None => 0,
        };
        self.selected = Some(next);
        self.selected_id = Some(ideas[next].id.clone());
    }

    pub fn select_prev(&mut self, ideas: &[IdeaCard]) {
        if ideas.is_empty() {
            return;
        }
        let prev = match self.selected {
            Some(idx) => idx.saturating_sub(1),
            None => 0,
        };
        self.selected = Some(prev);
        self.selected_id = Some(ideas[prev].id.clone());
    }

    pub fn render(&mut self, snapshot: &StatusSnapshot, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let max_w = area.width as usize;
        let header_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        let col_style = Style::default().fg(Color::DarkGray);
        let dim_style = Style::default().fg(Color::DarkGray);
        let err_style = Style::default().fg(Color::Red);

        let Some(summary) = snapshot.factory.as_ref() else {
            write_clipped(
                buf,
                area.x,
                area.y,
                max_w,
                "FACTORY   factory mode is disabled",
                dim_style,
            );
            return;
        };

        if let Some(ref err) = summary.error {
            let msg = format!(
                "FACTORY   could not read .agent-workspace/factory ({})",
                clean(err)
            );
            write_clipped(buf, area.x, area.y, max_w, &msg, err_style);
            return;
        }

        if summary.ideas.is_empty() {
            write_clipped(
                buf,
                area.x,
                area.y,
                max_w,
                "FACTORY   ideas 0",
                header_style,
            );
            if area.height >= 2 {
                write_clipped(buf, area.x, area.y + 1, max_w, "No ideas yet.", dim_style);
            }
            if area.height >= 3 {
                write_clipped(
                    buf,
                    area.x,
                    area.y + 2,
                    max_w,
                    "Create one with: tandem factory idea new --title \"…\" --text \"…\"",
                    dim_style,
                );
            }
            return;
        }

        let header = format!(
            "FACTORY   ideas {}   needs you {}",
            summary.ideas.len(),
            summary.needs_you
        );
        write_clipped(buf, area.x, area.y, max_w, &header, header_style);
        if area.height == 1 {
            return;
        }

        let narrow = area.width < 80;
        let wide = area.width >= 120;
        let show_hint = area.height >= 4;

        let mut y = area.y + 1;
        let bottom = if show_hint {
            area.y + area.height - 1
        } else {
            area.y + area.height
        };

        if !narrow && area.height >= 3 {
            let col_header = if wide {
                format!(
                    " {:<12} {:<22} {:<6} {:<9} {:<6} {:<9} TITLE",
                    "ID", "STATE", "RISK", "BLOCKERS", "OWNER", "UPDATED"
                )
            } else {
                format!(
                    " {:<12} {:<22} {:<5} {:<9} {:<8} TITLE",
                    "ID", "STATE", "RISK", "BLOCKERS", "UPDATED"
                )
            };
            write_clipped(buf, area.x, y, max_w, &col_header, col_style);
            y += 1;
        }

        let rows_avail = bottom.saturating_sub(y) as usize;
        let lines_per_idea = if narrow { 2 } else { 1 };
        let visible_ideas = (rows_avail / lines_per_idea).max(usize::from(rows_avail > 0));

        if let Some(sel) = self.selected {
            if sel < self.scroll_offset {
                self.scroll_offset = sel;
            } else if visible_ideas > 0 && sel >= self.scroll_offset + visible_ideas {
                self.scroll_offset = sel + 1 - visible_ideas;
            }
        }
        if self.scroll_offset >= summary.ideas.len() {
            self.scroll_offset = summary.ideas.len().saturating_sub(1);
        }

        for (idx, idea) in summary.ideas.iter().enumerate().skip(self.scroll_offset) {
            if y >= bottom {
                break;
            }
            let is_sel = self.selected == Some(idx);
            let marker = if is_sel { "▶" } else { " " };
            let style = if is_sel {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let updated = format!("{} ago", age(snapshot.now - idea.updated));
            let risk = match idea.risk {
                None => "-",
                Some(Risk::Low) => "low",
                Some(Risk::Medium) => "medium",
                Some(Risk::High) => "high",
            };
            let state = idea.state.label();
            let clean_title = clean(&idea.title);
            let clean_owner = clean(&idea.owner);

            if narrow {
                let line1 = format!(
                    "{marker}{:<11} {:<22} {:<5} {} blockers",
                    truncate_to_width(&idea.id, 11),
                    truncate_to_width(state, 22),
                    risk,
                    idea.blockers
                );
                write_clipped(buf, area.x, y, max_w, &line1, style);
                y += 1;
                if y < bottom {
                    let suffix = format!(" · {updated}");
                    let title_budget = max_w.saturating_sub(3 + suffix.width());
                    let t = truncate_to_width(&clean_title, title_budget);
                    let line2 = format!("   {t}{suffix}");
                    write_clipped(buf, area.x, y, max_w, &line2, style);
                    y += 1;
                }
            } else {
                let prefix = if wide {
                    format!(
                        "{marker}{:<12} {:<22} {:<6} {:<9} {:<6} {:<9} ",
                        truncate_to_width(&idea.id, 12),
                        truncate_to_width(state, 22),
                        risk,
                        idea.blockers,
                        truncate_to_width(&clean_owner, 6),
                        truncate_to_width(&updated, 9),
                    )
                } else {
                    format!(
                        "{marker}{:<12} {:<22} {:<5} {:<9} {:<8} ",
                        truncate_to_width(&idea.id, 12),
                        truncate_to_width(state, 22),
                        risk,
                        idea.blockers,
                        truncate_to_width(&updated, 8),
                    )
                };
                let title_budget = max_w.saturating_sub(prefix.width());
                let t = truncate_to_width(&clean_title, title_budget);
                let line = format!("{prefix}{t}");
                write_clipped(buf, area.x, y, max_w, &line, style);
                y += 1;
            }
        }

        if show_hint {
            write_clipped(
                buf,
                area.x,
                area.y + area.height - 1,
                max_w,
                " ↑/↓ move",
                dim_style,
            );
        }
    }
}

fn write_clipped(buf: &mut Buffer, x: u16, y: u16, max_w: usize, text: &str, style: Style) {
    let clipped = truncate_to_width(text, max_w);
    buf.set_stringn(x, y, clipped, max_w, style);
}
