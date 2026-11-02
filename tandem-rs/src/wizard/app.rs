use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout as RLayout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, StatefulWidget, Widget};

use crate::adapters::base::{AgentSpec, Model};
use crate::tmux::keys::{
    WIZARD_ADD, WIZARD_DELETE, WIZARD_FILTER, WIZARD_HELP, crossterm_key, display_shortcut,
    wizard_help_lines,
};
use crate::ui::footer::{Footer, FooterItem};
use crate::ui::input::TextInput;
use crate::ui::option_list::{OptionItem, OptionList, OptionListState};
use crate::wizard::flow::{CLIS, Step, WizardFlow};

pub type ModelLoader = Arc<dyn Fn(&str) -> Result<Vec<Model>, String> + Send + Sync>;

pub struct WizardApp {
    pub flow: WizardFlow,
    pub single: bool,
    loader: ModelLoader,
    pub models: Vec<Model>,
    pub loading_models: bool,
    model_rx: Option<Receiver<Result<Vec<Model>, String>>>,
    pub input: TextInput,
    pub input_focused: bool,
    pub choices_state: OptionListState,
    pub choice_items: Vec<OptionItem>,
    pub message: String,
    pub question: String,
    pub show_help: bool,
    pub exit_value: Option<Option<Vec<AgentSpec>>>,
    pub platform: String,
    pub choices_area: Rect,
    pub input_area: Rect,
}

impl WizardApp {
    pub fn new(installed: HashMap<String, bool>, model_loader: ModelLoader, single: bool) -> Self {
        let flow = WizardFlow::new(installed);
        let mut app = Self {
            flow,
            single,
            loader: model_loader,
            models: Vec::new(),
            loading_models: false,
            model_rx: None,
            input: TextInput::new(),
            input_focused: false,
            choices_state: OptionListState::default(),
            choice_items: Vec::new(),
            message: String::new(),
            question: String::new(),
            show_help: false,
            exit_value: None,
            platform: "linux".to_string(),
            choices_area: Rect::default(),
            input_area: Rect::default(),
        };
        app.render_step();
        app
    }

    pub fn is_running(&self) -> bool {
        self.exit_value.is_none()
    }

    pub fn poll_background(&mut self) {
        if let Some(rx) = &self.model_rx
            && let Ok(res) = rx.try_recv()
        {
            self.loading_models = false;
            match res {
                Ok(m) => {
                    self.models = m;
                    self.message.clear();
                }
                Err(err) => {
                    self.models.clear();
                    self.message = format!(
                        "Warning: could not list models ({err}). Type a model ID and press Enter."
                    );
                }
            }
            self.model_rx = None;
            if self.flow.step() == Step::Model {
                self.fill_models("");
            }
        }
    }

    pub fn start_model_load(&mut self) {
        self.loading_models = true;
        self.message = "Loading models…".to_string();
        let (tx, rx) = channel();
        self.model_rx = Some(rx);
        let cli = self.flow.draft_cli().to_string();
        let loader = self.loader.clone();
        thread::spawn(move || {
            let res = loader(&cli);
            let _ = tx.send(res);
        });
    }

    pub fn render_step(&mut self) {
        let step = self.flow.step();
        self.message.clear();
        self.choice_items.clear();
        self.input.clear();
        self.input_focused = false;

        match step {
            Step::Cli => {
                self.question = "Which AI CLI?".to_string();
                for cli in CLIS {
                    let ok = self.flow.installed.get(cli).copied().unwrap_or(false);
                    if ok {
                        self.choice_items.push(OptionItem::new(cli, cli));
                    } else {
                        self.choice_items
                            .push(OptionItem::disabled(cli, format!("{cli}  (not installed)")));
                    }
                }
                self.choices_state
                    .select_first_selectable(&self.choice_items);
            }
            Step::Role => {
                self.question = format!("Role for this {} agent?", self.flow.draft_cli());
                self.choice_items.push(OptionItem::new(
                    "planner",
                    "Planner      — left column; plans and breaks down specs",
                ));
                self.choice_items.push(OptionItem::new(
                    "implementer",
                    "Implementer  — right column; claims tasks, codes in worktrees",
                ));
                self.choices_state
                    .select_first_selectable(&self.choice_items);
            }
            Step::Model => {
                self.question = format!(
                    "Model for {} ({})",
                    self.flow.draft_cli(),
                    self.flow.draft_role()
                );
                self.start_model_load();
            }
            Step::Perms => {
                self.question =
                    "Warning: run agy with --dangerously-skip-permissions? agy will run tools WITHOUT asking you."
                        .to_string();
                self.choice_items
                    .push(OptionItem::new("no", "No (default)"));
                self.choice_items.push(OptionItem::new("yes", "Yes"));
                self.choices_state
                    .select_first_selectable(&self.choice_items);
            }
            Step::Cwd => {
                self.question =
                    "Working directory for this implementer (blank = workspace root)".to_string();
                self.input.placeholder = "/path/to/worktree".to_string();
                self.input_focused = true;
                self.input.focused = true;
            }
            Step::Summary => {
                self.question = "Ready".to_string();
                for (i, a) in self.flow.agents.iter().enumerate() {
                    self.choice_items.push(OptionItem::new(
                        format!("agent-{i}"),
                        format!("{}. {} · {} · {}", i + 1, a.cli, a.role, a.model),
                    ));
                }
                self.choice_items
                    .push(OptionItem::new("add", "Add another agent"));
                if self.flow.can_launch() {
                    self.choice_items.push(OptionItem::new(
                        "launch",
                        format!("Launch all ({})", self.flow.agents.len()),
                    ));
                }
                self.choices_state
                    .select_first_selectable(&self.choice_items);
            }
        }
    }

    fn fill_models(&mut self, needle: &str) {
        self.choice_items.clear();
        let low_needle = needle.to_lowercase();
        for m in &self.models {
            if low_needle.is_empty()
                || m.id.to_lowercase().contains(&low_needle)
                || m.label.to_lowercase().contains(&low_needle)
            {
                self.choice_items
                    .push(OptionItem::new(&m.id, format!("{}  {}", m.id, m.label)));
            }
        }
        if !self.choice_items.is_empty() {
            self.choices_state
                .select_first_selectable(&self.choice_items);
            if !self.input_focused {
                self.input.focused = false;
            }
        } else {
            self.input_focused = true;
            self.input.focused = true;
        }
    }

    pub fn handle_key(&mut self, event: &KeyEvent) {
        self.poll_background();

        if self.show_help {
            if event.code == KeyCode::Esc
                || event.code == KeyCode::Char('?')
                || event.code == KeyCode::Enter
            {
                self.show_help = false;
            }
            return;
        }

        if let Ok(k_help) = crossterm_key(WIZARD_HELP)
            && event.code == k_help.code
            && event.modifiers == k_help.modifiers
        {
            self.show_help = true;
            return;
        }

        if event.code == KeyCode::Char('?') {
            self.show_help = true;
            return;
        }

        let step = self.flow.step();

        if let Ok(k_filter) = crossterm_key(WIZARD_FILTER)
            && event.code == k_filter.code
            && event.modifiers == k_filter.modifiers
        {
            if step == Step::Model {
                self.input_focused = true;
                self.input.focused = true;
            }
            return;
        }

        if let Ok(k_add) = crossterm_key(WIZARD_ADD)
            && event.code == k_add.code
            && event.modifiers == k_add.modifiers
        {
            if step == Step::Summary {
                self.flow.add_another();
                self.render_step();
            }
            return;
        }

        if let Ok(k_del) = crossterm_key(WIZARD_DELETE)
            && event.code == k_del.code
            && event.modifiers == k_del.modifiers
        {
            if step == Step::Summary
                && let Some(id) = self.choices_state.selected_id(&self.choice_items)
                && let Some(idx_str) = id.strip_prefix("agent-")
                && let Ok(idx) = idx_str.parse::<usize>()
            {
                self.flow.delete(idx);
                self.render_step();
            }
            return;
        }

        if event.code == KeyCode::Char('L') && event.modifiers.contains(KeyModifiers::SHIFT) {
            self.action_launch();
            return;
        }

        match event.code {
            KeyCode::Esc => {
                self.action_go_back();
            }
            KeyCode::Up => {
                if step != Step::Cwd {
                    self.choices_state.select_prev(&self.choice_items);
                    self.input_focused = false;
                    self.input.focused = false;
                }
            }
            KeyCode::Down => {
                if step != Step::Cwd {
                    self.choices_state.select_next(&self.choice_items);
                    self.input_focused = false;
                    self.input.focused = false;
                }
            }
            KeyCode::Enter => {
                self.handle_enter();
            }
            _ => {
                if self.input_focused && (step == Step::Model || step == Step::Cwd) {
                    let changed = self.input.handle_key(event);
                    if changed && step == Step::Model && !self.loading_models {
                        let val = self.input.value.clone();
                        self.fill_models(&val);
                    }
                }
            }
        }
    }

    fn handle_enter(&mut self) {
        let step = self.flow.step();
        match step {
            Step::Cli => {
                if let Some(id) = self.choices_state.selected_id(&self.choice_items) {
                    if let Err(err) = self.flow.choose_cli(id) {
                        self.message = format!("Warning: {err}");
                        return;
                    }
                    self.advance();
                }
            }
            Step::Role => {
                if let Some(id) = self.choices_state.selected_id(&self.choice_items) {
                    if let Err(err) = self.flow.choose_role(id) {
                        self.message = format!("Warning: {err}");
                        return;
                    }
                    self.advance();
                }
            }
            Step::Model => {
                let text = self.input.value.trim().to_string();
                let model_to_choose = if text.is_empty() {
                    self.choices_state
                        .selected_id(&self.choice_items)
                        .map(|s| s.to_string())
                        .unwrap_or_default()
                } else {
                    text
                };

                if model_to_choose.is_empty() {
                    return;
                }

                if let Err(err) = self.flow.choose_model(&model_to_choose) {
                    self.message = format!("Warning: {err}");
                    return;
                }
                self.advance();
            }
            Step::Perms => {
                if let Some(id) = self.choices_state.selected_id(&self.choice_items) {
                    self.flow.choose_skip_permissions(id == "yes");
                    self.advance();
                }
            }
            Step::Cwd => {
                let cwd_text = self.input.value.clone();
                if let Err(err) = self.flow.choose_cwd(&cwd_text) {
                    self.message = format!("Warning: {err}");
                    return;
                }
                self.advance();
            }
            Step::Summary => {
                if let Some(id) = self.choices_state.selected_id(&self.choice_items) {
                    if id == "add" {
                        self.flow.add_another();
                        self.render_step();
                    } else if id == "launch" {
                        self.action_launch();
                    } else if let Some(idx_str) = id.strip_prefix("agent-")
                        && let Ok(idx) = idx_str.parse::<usize>()
                    {
                        self.flow.edit(idx);
                        self.render_step();
                    }
                }
            }
        }
    }

    pub fn handle_mouse(&mut self, event: &MouseEvent) {
        self.handle_mouse_with_area(event, self.choices_area);
    }

    pub fn handle_mouse_with_area(&mut self, event: &MouseEvent, choices_area: Rect) {
        if self.show_help {
            self.show_help = false;
            return;
        }

        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            let col = event.column;
            let row = event.row;

            if self.flow.step() == Step::Model
                && row >= self.input_area.y
                && row < self.input_area.y + self.input_area.height
                && col >= self.input_area.x
                && col < self.input_area.x + self.input_area.width
            {
                self.input_focused = true;
                self.input.focused = true;
                return;
            }

            if let Some(_idx) = self.choices_state.click_at(
                col,
                row,
                choices_area.y,
                choices_area.height,
                &self.choice_items,
            ) {
                self.input_focused = false;
                self.input.focused = false;
                self.handle_enter();
            }
        }
    }

    fn advance(&mut self) {
        if self.single && self.flow.step() == Step::Summary {
            self.exit_value = Some(Some(self.flow.agents.clone()));
            return;
        }
        self.render_step();
    }

    pub fn action_launch(&mut self) {
        if self.flow.step() == Step::Summary && self.flow.can_launch() {
            self.exit_value = Some(Some(self.flow.agents.clone()));
        } else if !self.flow.can_launch() {
            self.message = "Warning: add at least one agent before launching.".to_string();
        }
    }

    pub fn action_go_back(&mut self) {
        if self.flow.step() == Step::Cli && self.flow.agents.is_empty() {
            self.exit_value = Some(None);
            return;
        }
        self.flow.back();
        self.render_step();
    }

    pub fn check_action(&self, action: &str) -> bool {
        let step = self.flow.step();
        match action {
            "go_back" | "help" => true,
            "focus_input" => step == Step::Model,
            "add" | "delete" | "launch" => step == Step::Summary,
            _ => false,
        }
    }

    pub fn run<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut ratatui::Terminal<B>,
    ) -> Result<Option<Vec<AgentSpec>>, std::io::Error> {
        while self.is_running() {
            self.poll_background();
            terminal.draw(|f| self.render(f.area(), f.buffer_mut()))?;
            if let Some(val) = self.exit_value.take() {
                return Ok(val);
            }
            if crossterm::event::poll(std::time::Duration::from_millis(50))? {
                match crossterm::event::read()? {
                    crossterm::event::Event::Key(k) => self.handle_key(&k),
                    crossterm::event::Event::Mouse(m) => self.handle_mouse(&m),
                    _ => {}
                }
            }
        }
        Ok(self.exit_value.take().flatten())
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        self.poll_background();

        let chunks = RLayout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5), // Agents summary
                Constraint::Length(2), // Question
                Constraint::Min(4),    // Choices / Input
                Constraint::Length(1), // Message
                Constraint::Length(1), // Footer
            ])
            .split(area);

        let mut agent_lines = Vec::new();
        if self.flow.agents.is_empty() {
            agent_lines.push("No agents yet".to_string());
        } else {
            agent_lines.push("Agents to launch:".to_string());
            for (i, a) in self.flow.agents.iter().enumerate() {
                agent_lines.push(format!("  {}. {} · {} · {}", i + 1, a.cli, a.role, a.model));
            }
        }
        let agents_para = Paragraph::new(agent_lines.join("\n"))
            .block(Block::default().borders(Borders::BOTTOM).title("Agents"));
        agents_para.render(chunks[0], buf);

        let q_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        buf.set_stringn(
            chunks[1].x + 1,
            chunks[1].y,
            &self.question,
            chunks[1].width as usize - 2,
            q_style,
        );

        let step = self.flow.step();
        if step == Step::Cwd {
            self.choices_area = Rect::default();
            self.input_area = chunks[2];
            self.input.render_with_block(
                chunks[2],
                buf,
                Some(Block::default().borders(Borders::ALL)),
            );
        } else if step == Step::Model {
            let model_chunks = RLayout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Min(2)])
                .split(chunks[2]);
            self.input_area = model_chunks[0];
            self.input.render_with_block(
                model_chunks[0],
                buf,
                Some(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Filter / Type ID"),
                ),
            );
            self.choices_area = model_chunks[1];
            let opt_list = OptionList::new(&self.choice_items);
            opt_list.render(model_chunks[1], buf, &mut self.choices_state);
        } else {
            self.choices_area = chunks[2];
            self.input_area = Rect::default();
            let opt_list = OptionList::new(&self.choice_items);
            opt_list.render(chunks[2], buf, &mut self.choices_state);
        }

        if !self.message.is_empty() {
            let msg_style = if self.message.starts_with("Warning") {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Cyan)
            };
            buf.set_stringn(
                chunks[3].x + 1,
                chunks[3].y,
                &self.message,
                chunks[3].width as usize - 2,
                msg_style,
            );
        }

        let d = |k: &str| display_shortcut(k, &self.platform).unwrap_or_else(|_| k.to_string());
        let footer_items = match step {
            Step::Cli | Step::Role | Step::Perms => vec![
                FooterItem::new("↑↓", "Move"),
                FooterItem::new("Enter", "Pick"),
                FooterItem::new("Esc", "Back"),
                FooterItem::new("?", "Help"),
            ],
            Step::Model => vec![
                FooterItem::new("↑↓", "Move"),
                FooterItem::new("Enter", "Pick"),
                FooterItem::new(d(WIZARD_FILTER), "Filter"),
                FooterItem::new("Esc", "Back"),
                FooterItem::new("?", "Help"),
            ],
            Step::Cwd => vec![
                FooterItem::new("Type", "Directory"),
                FooterItem::new("Enter", "Confirm"),
                FooterItem::new("Esc", "Back"),
            ],
            Step::Summary => vec![
                FooterItem::new("Enter", "Edit row"),
                FooterItem::new(d(WIZARD_ADD), "Add another"),
                FooterItem::new(d(WIZARD_DELETE), "Delete"),
                FooterItem::new("Shift+L", "Launch all"),
                FooterItem::new("Esc", "Back"),
                FooterItem::new("?", "Help"),
            ],
        };
        let footer = Footer::new(&footer_items);
        footer.render(chunks[4], buf);

        if self.show_help {
            let help_area = centered_rect(60, 50, area);
            Clear.render(help_area, buf);
            let block = Block::default()
                .title(" Keys ")
                .borders(Borders::ALL)
                .style(Style::default().bg(Color::Black));
            let inner = block.inner(help_area);
            block.render(help_area, buf);
            let lines = wizard_help_lines(&self.platform);
            let p = Paragraph::new(lines.join("\n"));
            p.render(inner, buf);
        }
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = RLayout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    RLayout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
