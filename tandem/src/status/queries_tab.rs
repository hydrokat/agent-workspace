use std::collections::HashMap;
use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout as RLayout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, StatefulWidget};

use crate::coordination::schema::{Query, Session};
use crate::coordination::store::{CoordError, CoordStore};
use crate::sanitize::clean;
use crate::status::notify::order_queries;
use crate::status::render::age;
use crate::tmux::client::TmuxClient;
use crate::ui::option_list::{OptionItem, OptionList, OptionListState};
use crate::ui::textarea::TextArea;

pub const EMPTY: &str = "No open questions. Agents keep working while they wait.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryFocus {
    List,
    Options,
    Answer,
    Buttons,
}

pub struct QueriesTab {
    pub store: Arc<CoordStore>,
    pub tmux: Option<TmuxClient>,
    pub queries: Vec<Query>,
    pub sessions: Vec<Session>,
    pub current_id: Option<String>,
    pub drafts: HashMap<String, String>,

    pub list_items: Vec<OptionItem>,
    pub list_state: OptionListState,

    pub options_items: Vec<OptionItem>,
    pub options_state: OptionListState,

    pub answer: TextArea,
    pub notice: String,
    pub focus: QueryFocus,
    pub button_idx: usize,
    pub now_fn: Option<Arc<dyn Fn() -> f64 + Send + Sync>>,

    pub list_area: Rect,
    pub options_area: Rect,
    pub answer_area: Rect,
    pub buttons_area: Rect,
}

fn query_row(q: &Query, now: f64) -> String {
    let who = clean(&q.asked_by);
    let where_task = clean(&q.task);
    if q.status == "open" {
        format!(
            "{}  {}  {}  {} ago  blocks: \"{}\"",
            q.id,
            who,
            where_task,
            age(now - q.created),
            clean(&q.blocking_scope)
        )
    } else if q.status == "answered" {
        let via = if q.answered_via == "cli" {
            "answered in CLI"
        } else {
            "answered via tandem"
        };
        format!("{}  {}  {}  ✓ {}", q.id, who, where_task, via)
    } else {
        format!("{}  {}  {}  acknowledged", q.id, who, where_task)
    }
}

impl QueriesTab {
    pub fn new(store: Arc<CoordStore>, tmux: Option<TmuxClient>) -> Self {
        Self {
            store,
            tmux,
            queries: Vec::new(),
            sessions: Vec::new(),
            current_id: None,
            drafts: HashMap::new(),

            list_items: Vec::new(),
            list_state: OptionListState::default(),

            options_items: Vec::new(),
            options_state: OptionListState::default(),

            answer: TextArea::new(),
            notice: String::new(),
            focus: QueryFocus::List,
            button_idx: 0,
            now_fn: None,

            list_area: Rect::default(),
            options_area: Rect::default(),
            answer_area: Rect::default(),
            buttons_area: Rect::default(),
        }
    }

    pub fn with_now(mut self, now: Arc<dyn Fn() -> f64 + Send + Sync>) -> Self {
        self.now_fn = Some(now);
        self
    }

    pub fn now(&self) -> f64 {
        if let Some(ref f) = self.now_fn {
            f()
        } else {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0)
        }
    }

    pub fn current_query(&self) -> Option<&Query> {
        let qid = self.current_id.as_deref()?;
        self.queries.iter().find(|q| q.id == qid)
    }

    pub fn save_draft(&mut self) {
        if let Some(ref qid) = self.current_id {
            self.drafts.insert(qid.clone(), self.answer.text());
        }
    }

    pub fn update_data(&mut self, queries: Vec<Query>, sessions: Vec<Session>) {
        self.save_draft();
        self.queries = queries;
        self.sessions = sessions;
        let keep_id = self.current_id.clone();
        let now = self.now();

        self.list_items.clear();
        for (name, items) in order_queries(&self.queries) {
            self.list_items
                .push(OptionItem::header(format!("{name} ({})", items.len())));
            for item in items {
                self.list_items
                    .push(OptionItem::new(&item.id, query_row(&item, now)));
            }
        }

        self.reselect(keep_id.as_deref());
        if self.queries.is_empty() {
            self.show(None);
        }
    }

    pub fn reselect(&mut self, keep_id: Option<&str>) {
        let mut target_idx = None;
        if let Some(qid) = keep_id {
            target_idx = self
                .list_items
                .iter()
                .position(|item| item.id == qid && !item.disabled);
        }
        if target_idx.is_none() {
            target_idx = self.list_items.iter().position(|item| !item.disabled);
        }

        if let Some(idx) = target_idx {
            self.list_state.selected = Some(idx);
            let qid = self.list_items[idx].id.clone();
            self.show(Some(&qid));
        } else {
            self.list_state.selected = None;
            self.show(None);
        }
    }

    pub fn show(&mut self, qid: Option<&str>) {
        self.current_id = qid.map(|s| s.to_string());
        let query = self.current_query().cloned();
        if let Some(ref q) = query {
            let draft = self.drafts.get(&q.id).cloned().unwrap_or_default();
            self.answer.set_text(&draft);
            self.answer.read_only = q.status != "open";

            self.options_items.clear();
            for (i, opt) in q.options.iter().enumerate() {
                self.options_items.push(OptionItem::new(
                    format!("{i}"),
                    format!("({}) {}", i + 1, clean(opt)),
                ));
            }
            if !self.options_items.is_empty() {
                self.options_state.selected = Some(0);
            } else {
                self.options_state.selected = None;
            }
        } else {
            self.answer.set_text("");
            self.answer.read_only = true;
            self.options_items.clear();
            self.options_state.selected = None;
        }
    }

    pub fn action_send(&mut self) -> bool {
        let query = match self.current_query() {
            Some(q) => q.clone(),
            None => return false,
        };
        let text = self.answer.text().trim().to_string();
        if text.is_empty() {
            self.notice = "Warning: type an answer first.".to_string();
            return false;
        }
        match self.store.answer(&query.id, &text, "tandem") {
            Ok(_) => {
                self.drafts.remove(&query.id);
                self.notice = format!(
                    "Answer sent to {}. They pick it up at their next checkpoint.",
                    clean(&query.asked_by)
                );
                self.focus = QueryFocus::List;
                true
            }
            Err(CoordError::AnswerConflict(status)) => {
                self.notice =
                    format!("Warning: this question is already {status}; your draft is kept.");
                false
            }
            Err(e) => {
                self.notice =
                    format!("Warning: could not save the answer ({e}); your draft is kept.");
                false
            }
        }
    }

    pub fn action_cancel(&mut self) {
        self.answer.set_text("");
        if let Some(ref qid) = self.current_id {
            self.drafts.remove(qid);
        }
        self.notice.clear();
        self.focus = QueryFocus::List;
    }

    pub fn action_withdraw(&mut self) -> bool {
        if self.focus == QueryFocus::Answer {
            return false;
        }
        let query = match self.current_query() {
            Some(q) => q.clone(),
            None => return false,
        };
        match self.store.withdraw(&query.id) {
            Ok(_) => {
                self.notice = "Marked withdrawn.".to_string();
                true
            }
            Err(CoordError::AnswerConflict(status)) => {
                self.notice = format!("Warning: this question is already {status}.");
                false
            }
            Err(_) => false,
        }
    }

    pub fn action_goto(&mut self) {
        if self.focus == QueryFocus::Answer {
            return;
        }
        let query = match self.current_query() {
            Some(q) => q.clone(),
            None => return,
        };
        let tmux = match self.tmux {
            Some(ref t) => t,
            None => return,
        };
        let session = self.sessions.iter().find(|s| s.id == query.asked_by);
        if let Some(s) = session
            && s.pane.starts_with('%')
        {
            let _ = tmux.run_checked(&["select-pane", "-t", &s.pane]);
        }
    }

    pub fn handle_key(&mut self, event: &KeyEvent) -> bool {
        if event.modifiers.contains(KeyModifiers::CONTROL) && event.code == KeyCode::Char('s') {
            return self.action_send();
        }

        if event.code == KeyCode::Esc {
            self.action_cancel();
            return false;
        }

        if event.modifiers.contains(KeyModifiers::ALT) && event.code == KeyCode::Char('w') {
            return self.action_withdraw();
        }

        if event.modifiers.contains(KeyModifiers::ALT) && event.code == KeyCode::Char('g') {
            self.action_goto();
            return false;
        }

        if (self.focus == QueryFocus::List || self.focus == QueryFocus::Options)
            && let KeyCode::Char(c) = event.code
            && c.is_ascii_digit()
            && c != '0'
        {
            let idx = (c as u8 - b'1') as usize;
            if let Some(q) = self.current_query()
                && q.status == "open"
                && idx < q.options.len()
            {
                let opt = clean(&q.options[idx]);
                self.answer.set_text(&opt);
                self.save_draft();
                return false;
            }
        }

        if event.code == KeyCode::Tab {
            let has_options = !self.options_items.is_empty();
            self.focus = match self.focus {
                QueryFocus::List => {
                    if has_options {
                        QueryFocus::Options
                    } else {
                        QueryFocus::Answer
                    }
                }
                QueryFocus::Options => QueryFocus::Answer,
                QueryFocus::Answer => QueryFocus::Buttons,
                QueryFocus::Buttons => QueryFocus::List,
            };
            self.answer.focused = self.focus == QueryFocus::Answer;
            return false;
        }

        if event.code == KeyCode::BackTab {
            let has_options = !self.options_items.is_empty();
            self.focus = match self.focus {
                QueryFocus::List => QueryFocus::Buttons,
                QueryFocus::Options => QueryFocus::List,
                QueryFocus::Answer => {
                    if has_options {
                        QueryFocus::Options
                    } else {
                        QueryFocus::List
                    }
                }
                QueryFocus::Buttons => QueryFocus::Answer,
            };
            self.answer.focused = self.focus == QueryFocus::Answer;
            return false;
        }

        match self.focus {
            QueryFocus::List => match event.code {
                KeyCode::Up => {
                    self.save_draft();
                    self.list_state.select_prev(&self.list_items);
                    let qid = self
                        .list_state
                        .selected_id(&self.list_items)
                        .map(|s| s.to_string());
                    self.show(qid.as_deref());
                    false
                }
                KeyCode::Down => {
                    self.save_draft();
                    self.list_state.select_next(&self.list_items);
                    let qid = self
                        .list_state
                        .selected_id(&self.list_items)
                        .map(|s| s.to_string());
                    self.show(qid.as_deref());
                    false
                }
                KeyCode::Enter => {
                    if let Some(q) = self.current_query()
                        && q.status == "open"
                    {
                        self.focus = QueryFocus::Answer;
                        self.answer.focused = true;
                    }
                    false
                }
                _ => false,
            },
            QueryFocus::Options => match event.code {
                KeyCode::Up => {
                    self.options_state.select_prev(&self.options_items);
                    false
                }
                KeyCode::Down => {
                    self.options_state.select_next(&self.options_items);
                    false
                }
                KeyCode::Enter => {
                    if let Some(sel) = self.options_state.selected
                        && let Some(q) = self.current_query()
                        && sel < q.options.len()
                    {
                        let opt = clean(&q.options[sel]);
                        self.answer.set_text(&opt);
                        self.save_draft();
                    }
                    false
                }
                _ => false,
            },
            QueryFocus::Answer => {
                let changed = self.answer.handle_key(event);
                if changed {
                    self.save_draft();
                }
                false
            }
            QueryFocus::Buttons => match event.code {
                KeyCode::Left => {
                    if self.button_idx > 0 {
                        self.button_idx -= 1;
                    }
                    false
                }
                KeyCode::Right => {
                    if self.button_idx < 3 {
                        self.button_idx += 1;
                    }
                    false
                }
                KeyCode::Enter => match self.button_idx {
                    0 => self.action_send(),
                    1 => self.action_withdraw(),
                    2 => {
                        self.action_goto();
                        false
                    }
                    3 => {
                        self.action_cancel();
                        false
                    }
                    _ => false,
                },
                _ => false,
            },
        }
    }

    pub fn handle_mouse(&mut self, event: &MouseEvent, _outer: Rect) -> bool {
        if event.kind != MouseEventKind::Down(MouseButton::Left) {
            return false;
        }

        let col = event.column;
        let row = event.row;

        if let Some(idx) = self.list_state.click_at(
            col,
            row,
            self.list_area.y,
            self.list_area.height,
            &self.list_items,
        ) {
            self.save_draft();
            self.list_state.selected = Some(idx);
            let qid = self
                .list_state
                .selected_id(&self.list_items)
                .map(|s| s.to_string());
            self.show(qid.as_deref());
            self.focus = QueryFocus::List;
            self.answer.focused = false;
            return false;
        }

        if !self.options_items.is_empty()
            && row > self.options_area.y
            && let Some(idx) = self.options_state.click_at(
                col,
                row,
                self.options_area.y + 1,
                self.options_area.height.saturating_sub(1),
                &self.options_items,
            )
        {
            self.options_state.selected = Some(idx);
            if let Some(q) = self.current_query()
                && idx < q.options.len()
            {
                let opt = clean(&q.options[idx]);
                self.answer.set_text(&opt);
                self.save_draft();
            }
            self.focus = QueryFocus::Options;
            self.answer.focused = false;
            return false;
        }

        if row >= self.answer_area.y
            && row < self.answer_area.y + self.answer_area.height
            && col >= self.answer_area.x
            && col < self.answer_area.x + self.answer_area.width
        {
            self.focus = QueryFocus::Answer;
            self.answer.focused = true;
            self.answer.click_at(
                col,
                row,
                self.answer_area.x,
                self.answer_area.y,
                self.answer_area.width,
                self.answer_area.height,
            );
            return false;
        }

        if row == self.buttons_area.y
            && col >= self.buttons_area.x
            && col < self.buttons_area.x + self.buttons_area.width
        {
            self.focus = QueryFocus::Buttons;
            let offset = col.saturating_sub(self.buttons_area.x);
            if offset < 8 {
                return self.action_send();
            } else if offset < 20 {
                return self.action_withdraw();
            } else if offset < 35 {
                self.action_goto();
                return false;
            } else if offset < 45 {
                self.action_cancel();
                return false;
            }
        }

        false
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let has_options = !self.options_items.is_empty();
        let options_h = if has_options {
            (self.options_items.len().min(4) as u16) + 1
        } else {
            0
        };

        let chunks = RLayout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(3),
                Constraint::Length(options_h),
                Constraint::Length(4),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(area);

        self.list_area = chunks[0];
        self.options_area = chunks[2];
        self.answer_area = chunks[3];
        self.buttons_area = chunks[5];

        let list_widget = OptionList::new(&self.list_items);
        list_widget.render(chunks[0], buf, &mut self.list_state);

        let detail_area = chunks[1];
        if detail_area.height > 0 {
            if let Some(q) = self.current_query() {
                let line1 = format!(
                    "{} · asked by {} · task {}",
                    q.id,
                    clean(&q.asked_by),
                    clean(&q.task)
                );
                buf.set_stringn(
                    detail_area.x + 1,
                    detail_area.y,
                    &line1,
                    detail_area.width as usize - 2,
                    Style::default().fg(Color::Cyan),
                );

                let line2 = clean(&q.question);
                buf.set_stringn(
                    detail_area.x + 1,
                    detail_area.y + 1,
                    &line2,
                    detail_area.width as usize - 2,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                );

                if q.status != "open" && detail_area.height >= 3 {
                    let how = if q.answered_via == "cli" {
                        "in CLI"
                    } else {
                        "via tandem"
                    };
                    let line3 = if !q.answer.is_empty() {
                        format!("Status: {} {how}. Answer: {}", q.status, clean(&q.answer))
                    } else {
                        format!("Status: {}", q.status)
                    };
                    buf.set_stringn(
                        detail_area.x + 1,
                        detail_area.y + 2,
                        &line3,
                        detail_area.width as usize - 2,
                        Style::default().fg(Color::Green),
                    );
                }
            } else {
                buf.set_stringn(
                    detail_area.x + 1,
                    detail_area.y,
                    EMPTY,
                    detail_area.width as usize - 2,
                    Style::default().fg(Color::DarkGray),
                );
            }
        }

        if has_options && chunks[2].height > 0 {
            buf.set_string(
                chunks[2].x + 1,
                chunks[2].y,
                "Options (1-9):",
                Style::default().fg(Color::Yellow),
            );
            let opt_sub = Rect::new(
                chunks[2].x + 2,
                chunks[2].y + 1,
                chunks[2].width.saturating_sub(2),
                chunks[2].height.saturating_sub(1),
            );
            let opt_widget = OptionList::new(&self.options_items);
            opt_widget.render(opt_sub, buf, &mut self.options_state);
        }

        let answer_block = Block::default()
            .borders(Borders::ALL)
            .title("Answer (Ctrl+S to send):")
            .border_style(if self.focus == QueryFocus::Answer {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            });
        self.answer
            .render_with_block(chunks[3], buf, Some(answer_block));

        if !self.notice.is_empty() {
            let n_style = if self.notice.starts_with("Warning") {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::Green)
            };
            buf.set_stringn(
                chunks[4].x + 1,
                chunks[4].y,
                &self.notice,
                chunks[4].width as usize - 2,
                n_style,
            );
        }

        self.render_buttons(chunks[5], buf);
    }

    fn render_buttons(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let q = self.current_query();
        let is_open = q.map(|x| x.status == "open").unwrap_or(false);
        let has_draft = !self.answer.text().trim().is_empty();
        let has_query = q.is_some();

        let btns = [
            ("Send", is_open && has_draft),
            ("Withdraw", is_open),
            ("Go to agent", has_query),
            ("Cancel", has_query),
        ];

        let mut x = area.x + 1;
        for (i, (label, enabled)) in btns.iter().enumerate() {
            let is_focused = self.focus == QueryFocus::Buttons && self.button_idx == i;
            let text = format!("[{label}]");
            let style = if !*enabled {
                Style::default().fg(Color::DarkGray)
            } else if is_focused {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            buf.set_string(x, area.y, &text, style);
            x += text.chars().count() as u16 + 2;
        }
    }
}
