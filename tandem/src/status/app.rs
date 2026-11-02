use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout as RLayout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{StatefulWidget, Widget};

use crate::config::Config;
use crate::coordination::store::CoordStore;
use crate::notice::read_notice;
use crate::snapshot::{StatusSnapshot, collect};
use crate::status::banners::{BannerAction, OfferBanner, UpdateBanner};
use crate::status::notify::Notifier;
use crate::status::queries_tab::QueriesTab;
use crate::status::render;
use crate::status::spec_tab;
use crate::status::sprint_tab;
use crate::status::task_tab;
use crate::tmux::client::TmuxClient;
use crate::tmux::keys::{GO_TO_AGENT, STATUS_REFRESH, crossterm_key, display_shortcut};
use crate::ui::collapsible::Collapsible;
use crate::ui::footer::{Footer, FooterItem};
use crate::ui::option_list::{OptionItem, OptionList, OptionListState};
use crate::ui::scroll::ScrollView;
use crate::ui::tabs::TabBar;
use crate::update_check::{
    Runner as UpdateRunner, due, installed_version_from_manifest, latest_version, read_state,
    record, should_offer,
};

pub const COMPACT_BELOW_ROWS: u16 = 8;
pub const AUTOSAVE_SECONDS: f64 = 15.0;
pub const CHECK_INTERVAL_SECONDS: f64 = 600.0;
pub const UPDATE_NOW_KEY: &str = "Alt+u";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusTab {
    Status,
    Sprint,
    Spec,
    Task,
    Queries,
}

impl StatusTab {
    pub const ALL: [StatusTab; 5] = [
        StatusTab::Status,
        StatusTab::Sprint,
        StatusTab::Spec,
        StatusTab::Task,
        StatusTab::Queries,
    ];

    pub fn index(&self) -> usize {
        match self {
            StatusTab::Status => 0,
            StatusTab::Sprint => 1,
            StatusTab::Spec => 2,
            StatusTab::Task => 3,
            StatusTab::Queries => 4,
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(StatusTab::Status),
            1 => Some(StatusTab::Sprint),
            2 => Some(StatusTab::Spec),
            3 => Some(StatusTab::Task),
            4 => Some(StatusTab::Queries),
            _ => None,
        }
    }

    pub fn next(&self) -> Self {
        match self {
            StatusTab::Status => StatusTab::Sprint,
            StatusTab::Sprint => StatusTab::Spec,
            StatusTab::Spec => StatusTab::Task,
            StatusTab::Task => StatusTab::Queries,
            StatusTab::Queries => StatusTab::Status,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            StatusTab::Status => StatusTab::Queries,
            StatusTab::Sprint => StatusTab::Status,
            StatusTab::Spec => StatusTab::Sprint,
            StatusTab::Task => StatusTab::Spec,
            StatusTab::Queries => StatusTab::Task,
        }
    }

    pub fn title(&self, open_queries: usize) -> String {
        match self {
            StatusTab::Status => "Status".to_string(),
            StatusTab::Sprint => "Sprint".to_string(),
            StatusTab::Spec => "Spec".to_string(),
            StatusTab::Task => "Task".to_string(),
            StatusTab::Queries => {
                if open_queries > 0 {
                    format!("Queries ({open_queries})")
                } else {
                    "Queries".to_string()
                }
            }
        }
    }
}

pub struct StatusApp {
    pub workspace: PathBuf,
    pub store: Arc<CoordStore>,
    pub tmux: Option<TmuxClient>,
    pub refresh_seconds: f64,
    pub autosave: Option<Arc<dyn Fn() + Send + Sync>>,
    pub autosave_seconds: f64,
    pub snapshot: StatusSnapshot,
    pub notifier: Notifier,
    pub config: Option<Config>,
    pub update_runner: Option<UpdateRunner>,
    pub now_fn: Option<Arc<dyn Fn() -> f64 + Send + Sync>>,
    pub notified_versions: HashSet<String>,
    pub offered_versions: HashSet<String>,

    pub update_banner: Option<UpdateBanner>,
    pub offer_banner: Option<OfferBanner>,

    pub tab: StatusTab,
    pub tab_bar: TabBar,
    pub sessions_state: OptionListState,
    pub session_items: Vec<OptionItem>,

    pub sprint_scroll: ScrollView,
    pub spec_scroll: ScrollView,
    pub task_scroll: ScrollView,

    pub collapsible_repos: Collapsible,
    pub collapsible_conflicts: Collapsible,

    pub queries_tab: QueriesTab,
    pub platform: String,

    pub last_refresh: f64,
    pub last_autosave: f64,
    pub last_update_check: f64,

    pub banner_area: Rect,
    pub tabs_area: Rect,
    pub body_area: Rect,
    pub sessions_area: Rect,
    pub repos_header_y: u16,
    pub conflicts_header_y: u16,
}

impl StatusApp {
    pub fn new(
        workspace: PathBuf,
        store: CoordStore,
        tmux: Option<TmuxClient>,
        config: Option<Config>,
    ) -> Self {
        let store_arc = Arc::new(store);
        let notice = read_notice(&workspace);
        let update_banner = notice
            .filter(|n| !n.seen)
            .map(|n| UpdateBanner::new(n, workspace.clone(), tmux.clone()));

        let queries_tab = QueriesTab::new(store_arc.clone(), tmux.clone());
        let notifier = Notifier::new(tmux.clone());

        let tab_bar = TabBar::from_tabs(&StatusTab::ALL, 0, StatusTab::Status);

        let mut app = Self {
            workspace,
            store: store_arc,
            tmux,
            refresh_seconds: 2.0,
            autosave: None,
            autosave_seconds: AUTOSAVE_SECONDS,
            snapshot: StatusSnapshot::default(),
            notifier,
            config,
            update_runner: None,
            now_fn: None,
            notified_versions: HashSet::new(),
            offered_versions: HashSet::new(),

            update_banner,
            offer_banner: None,

            tab: StatusTab::Status,
            tab_bar,
            sessions_state: OptionListState::default(),
            session_items: Vec::new(),

            sprint_scroll: ScrollView::new(),
            spec_scroll: ScrollView::new(),
            task_scroll: ScrollView::new(),

            collapsible_repos: Collapsible::new(
                "Repositories · * main checkout · → linked worktree",
                true,
            ),
            collapsible_conflicts: Collapsible::new("Conflicts", false),

            queries_tab,
            platform: "linux".to_string(),

            last_refresh: 0.0,
            last_autosave: 0.0,
            last_update_check: 0.0,

            banner_area: Rect::default(),
            tabs_area: Rect::default(),
            body_area: Rect::default(),
            sessions_area: Rect::default(),
            repos_header_y: 0,
            conflicts_header_y: 0,
        };
        app.action_refresh_now();
        app
    }

    pub fn with_refresh_seconds(mut self, s: f64) -> Self {
        self.refresh_seconds = s;
        self
    }

    pub fn with_autosave(mut self, f: Arc<dyn Fn() + Send + Sync>, seconds: f64) -> Self {
        self.autosave = Some(f);
        self.autosave_seconds = seconds;
        self
    }

    pub fn with_now(mut self, now: Arc<dyn Fn() -> f64 + Send + Sync>) -> Self {
        self.now_fn = Some(now);
        self
    }

    pub fn with_update_runner(mut self, runner: UpdateRunner) -> Self {
        self.update_runner = Some(runner);
        self
    }

    pub fn now(&self) -> f64 {
        if let Some(ref f) = self.now_fn {
            f()
        } else {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0)
        }
    }

    pub fn action_refresh_now(&mut self) {
        let snapshot = collect(&self.workspace, &self.store, Duration::from_secs(3));
        self.apply(snapshot);
        self.last_refresh = self.now();
    }

    pub fn apply(&mut self, snapshot: StatusSnapshot) {
        self.snapshot = snapshot;
        let highlighted = self.sessions_state.selected;
        self.session_items.clear();
        for index in 0..self.snapshot.sessions.len() {
            let row_str = render::session_row(&self.snapshot, index);
            let sess_id = &self.snapshot.sessions[index].id;
            self.session_items.push(OptionItem::new(sess_id, row_str));
        }
        if let Some(hl) = highlighted
            && hl < self.session_items.len()
        {
            self.sessions_state.selected = Some(hl);
        } else if !self.session_items.is_empty() {
            self.sessions_state.selected = Some(0);
        } else {
            self.sessions_state.selected = None;
        }

        self.queries_tab.update_data(
            self.snapshot.queries.clone(),
            self.snapshot.sessions.clone(),
        );

        self.tab_bar = TabBar::from_tabs(&StatusTab::ALL, self.snapshot.open_queries(), self.tab);

        self.notifier.notify(&self.snapshot.queries);
    }

    pub fn maybe_check_update(&mut self) {
        if let Some(ref cfg) = self.config {
            if !cfg.update_check {
                return;
            }
            let state = read_state(&self.workspace);
            let now = self.now();
            if due(&state, now, cfg.update_interval as i64) {
                let spec = &cfg.update_spec;
                let latest = if let Some(ref runner) = self.update_runner {
                    latest_version(spec, &self.workspace, |a, p, d| runner(a, p, d))
                } else {
                    latest_version(spec, &self.workspace, |a, p, d| {
                        Ok(crate::updater::default_quiet_runner(a, p, d))
                    })
                };
                let new_state = record(&self.workspace, &state, now, latest.as_deref())
                    .unwrap_or_else(|_| state.clone());
                let installed = installed_version_from_manifest(&self.workspace);
                if should_offer(
                    installed.as_deref(),
                    Some(&new_state.latest),
                    &new_state.skip,
                ) {
                    self.show_offer(&installed.unwrap_or_default(), &new_state.latest);
                }
                self.last_update_check = now;
            }
        }
    }

    pub fn show_offer(&mut self, installed: &str, latest: &str) {
        if self.offered_versions.contains(latest) {
            return;
        }
        self.offered_versions.insert(latest.to_string());
        self.offer_banner = Some(OfferBanner::new(
            installed,
            latest,
            self.workspace.clone(),
            self.tmux.clone(),
        ));
        if let Some(ref tmux) = self.tmux
            && !self.notified_versions.contains(latest)
        {
            self.notified_versions.insert(latest.to_string());
            let _ = tmux.run_checked(&[
                "display-message",
                &format!("tandem: update {latest} available — see the Status window"),
            ]);
        }
    }

    pub fn check_action(&self, action: &str) -> bool {
        if action == "update_now" {
            self.offer_banner.is_some()
        } else {
            true
        }
    }

    pub fn set_tab(&mut self, tab: StatusTab) {
        self.tab = tab;
        self.tab_bar.selected = tab.index();
    }

    pub fn action_show_status(&mut self) {
        self.set_tab(StatusTab::Status);
    }

    pub fn action_show_queries(&mut self) {
        self.set_tab(StatusTab::Queries);
    }

    pub fn action_update_now(&mut self) {
        if let Some(ref offer) = self.offer_banner {
            offer.run_update_popup();
        }
    }

    pub fn action_goto(&mut self) {
        if self.tab == StatusTab::Status
            && let Some(idx) = self.sessions_state.selected
            && let Some(sess) = self.snapshot.sessions.get(idx)
        {
            let pane = &sess.pane;
            if pane.starts_with('%')
                && let Some(ref tmux) = self.tmux
            {
                let _ = tmux.run_checked(&["select-pane", "-t", pane]);
            }
        } else if self.tab == StatusTab::Task
            && let Some(first_task) = self.snapshot.ongoing_tasks.first()
            && let Some(ref pane) = first_task.session_pane
            && pane.starts_with('%')
            && let Some(ref tmux) = self.tmux
        {
            let _ = tmux.run_checked(&["select-pane", "-t", pane]);
        }
    }

    pub fn tick_timers(&mut self) {
        let now = self.now();
        if now - self.last_refresh >= self.refresh_seconds {
            self.action_refresh_now();
        }
        if let Some(ref autosave) = self.autosave
            && now - self.last_autosave >= self.autosave_seconds
        {
            autosave();
            self.last_autosave = now;
        }
        if now - self.last_update_check >= CHECK_INTERVAL_SECONDS {
            self.maybe_check_update();
        }
    }

    pub fn handle_key(&mut self, event: &KeyEvent) {
        if let Ok(k_refresh) = crossterm_key(STATUS_REFRESH)
            && event.code == k_refresh.code
            && event.modifiers == k_refresh.modifiers
        {
            self.action_refresh_now();
            return;
        }

        if let Ok(k_goto) = crossterm_key(GO_TO_AGENT)
            && event.code == k_goto.code
            && event.modifiers == k_goto.modifiers
        {
            self.action_goto();
            return;
        }

        if let Ok(k_status) = crossterm_key("Alt+s")
            && event.code == k_status.code
            && event.modifiers == k_status.modifiers
        {
            self.action_show_status();
            return;
        }

        if let Ok(k_queries) = crossterm_key("Alt+q")
            && event.code == k_queries.code
            && event.modifiers == k_queries.modifiers
        {
            self.action_show_queries();
            return;
        }

        if let Ok(k_update) = crossterm_key(UPDATE_NOW_KEY)
            && event.code == k_update.code
            && event.modifiers == k_update.modifiers
            && self.offer_banner.is_some()
        {
            self.action_update_now();
            return;
        }

        let is_shift_tab = event.code == KeyCode::BackTab
            || (event.code == KeyCode::Tab
                && event
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::SHIFT));

        if is_shift_tab {
            self.set_tab(self.tab.prev());
            return;
        }

        if event.modifiers.is_empty() {
            match event.code {
                KeyCode::Char('1') => {
                    self.set_tab(StatusTab::Status);
                    return;
                }
                KeyCode::Char('2') => {
                    self.set_tab(StatusTab::Sprint);
                    return;
                }
                KeyCode::Char('3') => {
                    self.set_tab(StatusTab::Spec);
                    return;
                }
                KeyCode::Char('4') => {
                    self.set_tab(StatusTab::Task);
                    return;
                }
                KeyCode::Char('5') => {
                    self.set_tab(StatusTab::Queries);
                    return;
                }
                _ => {}
            }
        }

        if self.tab == StatusTab::Queries {
            if self.queries_tab.handle_key(event) {
                self.action_refresh_now();
            }
            return;
        }

        if event.code == KeyCode::Tab {
            self.set_tab(self.tab.next());
            return;
        }

        let view_h = if self.body_area.height > 0 {
            self.body_area.height as usize
        } else {
            20
        };
        let view_w = if self.body_area.width > 0 {
            self.body_area.width
        } else {
            80
        };

        match self.tab {
            StatusTab::Status => match event.code {
                KeyCode::Up => {
                    self.sessions_state.select_prev(&self.session_items);
                }
                KeyCode::Down => {
                    self.sessions_state.select_next(&self.session_items);
                }
                KeyCode::Enter => {
                    self.action_goto();
                }
                _ => {}
            },
            StatusTab::Sprint => {
                let lines = sprint_tab::lines(&self.snapshot, view_w);
                self.sprint_scroll
                    .handle_key(event.code, lines.len(), view_h);
            }
            StatusTab::Spec => {
                let lines = spec_tab::lines(&self.snapshot, view_w);
                self.spec_scroll.handle_key(event.code, lines.len(), view_h);
            }
            StatusTab::Task => {
                let lines = task_tab::lines(&self.snapshot, view_w);
                self.task_scroll.handle_key(event.code, lines.len(), view_h);
            }
            StatusTab::Queries => {}
        }
    }

    pub fn handle_mouse(&mut self, event: &MouseEvent) {
        let col = event.column;
        let row = event.row;

        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(ref banner) = self.update_banner {
                    let action = banner.click_at(col, row, self.banner_area);
                    if action == BannerAction::DismissUpdate {
                        self.update_banner = None;
                        return;
                    }
                }

                if let Some(ref banner) = self.offer_banner {
                    let action = banner.click_at(col, row, self.banner_area);
                    match action {
                        BannerAction::Later | BannerAction::SkipVersion(_) => {
                            self.offer_banner = None;
                            return;
                        }
                        BannerAction::RunUpdate => {
                            return;
                        }
                        _ => {}
                    }
                }

                if row == self.tabs_area.y
                    && let Some(new_idx) =
                        self.tab_bar
                            .click_at(col, self.tabs_area.x, self.tabs_area.width)
                {
                    if let Some(tab) = StatusTab::from_index(new_idx) {
                        self.set_tab(tab);
                    }
                    return;
                }

                match self.tab {
                    StatusTab::Status => {
                        if self.collapsible_repos.click_at(row, self.repos_header_y) {
                            return;
                        }
                        if self
                            .collapsible_conflicts
                            .click_at(row, self.conflicts_header_y)
                        {
                            return;
                        }
                        if let Some(_idx) = self.sessions_state.click_at(
                            col,
                            row,
                            self.sessions_area.y,
                            self.sessions_area.height,
                            &self.session_items,
                        ) {
                            self.action_goto();
                        }
                    }
                    StatusTab::Queries
                        if self.queries_tab.handle_mouse(event, self.sessions_area) =>
                    {
                        self.action_refresh_now();
                    }
                    _ => {}
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let view_h = if self.body_area.height > 0 {
                    self.body_area.height as usize
                } else {
                    20
                };
                let view_w = if self.body_area.width > 0 {
                    self.body_area.width
                } else {
                    80
                };
                match self.tab {
                    StatusTab::Sprint => {
                        let lines = sprint_tab::lines(&self.snapshot, view_w);
                        self.sprint_scroll
                            .handle_mouse(event.kind, lines.len(), view_h);
                    }
                    StatusTab::Spec => {
                        let lines = spec_tab::lines(&self.snapshot, view_w);
                        self.spec_scroll
                            .handle_mouse(event.kind, lines.len(), view_h);
                    }
                    StatusTab::Task => {
                        let lines = task_tab::lines(&self.snapshot, view_w);
                        self.task_scroll
                            .handle_mouse(event.kind, lines.len(), view_h);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let compact = area.height < COMPACT_BELOW_ROWS;

        let has_update_banner = !compact && self.update_banner.is_some();
        let has_offer_banner = !compact && self.offer_banner.is_some();
        let banner_height = if has_update_banner || has_offer_banner {
            1
        } else {
            0
        };

        let chunks = RLayout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(banner_height), // Banner (if any)
                Constraint::Length(1),             // Tab bar
                Constraint::Length(1),             // Summary line
                Constraint::Min(0),                // Body / Queries
                Constraint::Length(1),             // Footer
            ])
            .split(area);

        self.banner_area = chunks[0];
        if has_update_banner {
            if let Some(ref banner) = self.update_banner {
                banner.render(chunks[0], buf);
            }
        } else if has_offer_banner && let Some(ref banner) = self.offer_banner {
            banner.render(chunks[0], buf);
        }

        self.tabs_area = chunks[1];
        (&self.tab_bar).render(chunks[1], buf);

        let summary_text = render::summary_line(&self.snapshot);
        let summary_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        buf.set_stringn(
            chunks[2].x + 1,
            chunks[2].y,
            &summary_text,
            chunks[2].width as usize - 2,
            summary_style,
        );

        self.body_area = chunks[3];
        if !compact {
            match self.tab {
                StatusTab::Status => {
                    self.render_status_tab(chunks[3], buf);
                }
                StatusTab::Sprint => {
                    let lines = sprint_tab::lines(&self.snapshot, chunks[3].width);
                    self.sprint_scroll.render_lines(&lines, chunks[3], buf);
                }
                StatusTab::Spec => {
                    let lines = spec_tab::lines(&self.snapshot, chunks[3].width);
                    self.spec_scroll.render_lines(&lines, chunks[3], buf);
                }
                StatusTab::Task => {
                    let lines = task_tab::lines(&self.snapshot, chunks[3].width);
                    self.task_scroll.render_lines(&lines, chunks[3], buf);
                }
                StatusTab::Queries => {
                    self.queries_tab.render(chunks[3], buf);
                }
            }
        }

        self.render_footer(chunks[4], buf);
    }

    fn render_status_tab(&mut self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let mut y = area.y;
        let bottom = area.y + area.height;

        if let Some(headline) = render::sprint_line(&self.snapshot)
            && y < bottom
        {
            buf.set_stringn(
                area.x + 1,
                y,
                &headline,
                (area.width as usize).saturating_sub(2),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
            y += 1;
        }

        let label_style = Style::default().fg(Color::Yellow);
        buf.set_string(area.x + 1, y, "In progress", label_style);
        y += 1;

        let sess_count = self.session_items.len().min(6) as u16;
        let sess_height = sess_count.min(bottom.saturating_sub(y));
        self.sessions_area = Rect::new(area.x, y, area.width, sess_height);
        if sess_height > 0 {
            let opt_list = OptionList::new(&self.session_items);
            opt_list.render(self.sessions_area, buf, &mut self.sessions_state);
            y += sess_height;
        }

        let in_prog = render::in_progress_text(&self.snapshot);
        for line in in_prog.lines() {
            if y >= bottom {
                break;
            }
            buf.set_stringn(
                area.x + 1,
                y,
                line,
                (area.width as usize).saturating_sub(2),
                Style::default().fg(Color::DarkGray),
            );
            y += 1;
        }

        if y < bottom {
            buf.set_stringn(
                area.x + 1,
                y,
                "Sprint, Spec and Task have their own tabs",
                (area.width as usize).saturating_sub(2),
                Style::default().fg(Color::DarkGray),
            );
            y += 1;
        }

        if y < bottom {
            self.repos_header_y = y;
            self.collapsible_repos.render_header(
                Rect::new(area.x + 1, y, area.width.saturating_sub(2), 1),
                buf,
            );
            y += 1;
            if self.collapsible_repos.expanded && y < bottom {
                let repos_w = (area.width.saturating_sub(6) as usize).max(render::MIN_WIDTH);
                let repos_text = render::repos_text(&self.snapshot, repos_w);
                for line in repos_text.lines() {
                    if y >= bottom {
                        break;
                    }
                    buf.set_stringn(
                        area.x + 3,
                        y,
                        line,
                        area.width as usize - 4,
                        Style::default().fg(Color::White),
                    );
                    y += 1;
                }
            }
        }

        if y < bottom {
            self.conflicts_header_y = y;
            self.collapsible_conflicts.render_header(
                Rect::new(area.x + 1, y, area.width.saturating_sub(2), 1),
                buf,
            );
            y += 1;
            if self.collapsible_conflicts.expanded && y < bottom {
                let conflicts_text = render::conflicts_text(&self.snapshot);
                for line in conflicts_text.lines() {
                    if y >= bottom {
                        break;
                    }
                    buf.set_stringn(
                        area.x + 3,
                        y,
                        line,
                        area.width as usize - 4,
                        Style::default().fg(Color::Red),
                    );
                    y += 1;
                }
            }
        }
    }

    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        let d = |k: &str| display_shortcut(k, &self.platform).unwrap_or_else(|_| k.to_string());
        let mut items = Vec::new();
        match self.tab {
            StatusTab::Status => {
                items.push(FooterItem::new("Tab", "Switch tabs"));
                items.push(FooterItem::new("1-5", "Jump"));
                items.push(FooterItem::new("↑↓", "Move"));
                items.push(FooterItem::new(d(GO_TO_AGENT), "Go to agent"));
                items.push(FooterItem::new(d(STATUS_REFRESH), "Refresh"));
                items.push(FooterItem::new(d("Alt+s"), "Status"));
                items.push(FooterItem::new(d("Alt+q"), "Queries"));
            }
            StatusTab::Sprint | StatusTab::Spec | StatusTab::Task => {
                items.push(FooterItem::new("Tab", "Switch tabs"));
                items.push(FooterItem::new("1-5", "Jump"));
                items.push(FooterItem::new("↑↓", "Scroll"));
                items.push(FooterItem::new(d(STATUS_REFRESH), "Refresh"));
                items.push(FooterItem::new(d("Alt+s"), "Status"));
                items.push(FooterItem::new(d("Alt+q"), "Queries"));
            }
            StatusTab::Queries => {
                items.push(FooterItem::new("Tab", "Next element"));
                items.push(FooterItem::new("Shift+Tab", "Prev tab"));
                items.push(FooterItem::new("1-5", "Jump"));
                items.push(FooterItem::new("↑↓", "Move"));
                items.push(FooterItem::new(d(STATUS_REFRESH), "Refresh"));
                items.push(FooterItem::new(d("Alt+s"), "Status"));
                items.push(FooterItem::new(d("Alt+q"), "Queries"));
            }
        }
        if self.offer_banner.is_some() {
            items.push(FooterItem::new(d(UPDATE_NOW_KEY), "Update now"));
        }
        let footer = Footer::new(&items);
        footer.render(area, buf);
    }

    pub fn run<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<(), std::io::Error> {
        self.action_refresh_now();
        let mut last_tick = std::time::Instant::now();
        loop {
            terminal.draw(|f| self.render(f.area(), f.buffer_mut()))?;
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
                    }
                    crossterm::event::Event::Mouse(mouse) => {
                        self.handle_mouse(&mouse);
                    }
                    _ => {}
                }
            }
            if last_tick.elapsed() >= Duration::from_millis(500) {
                self.tick_timers();
                last_tick = std::time::Instant::now();
            }
        }
        Ok(())
    }
}

pub fn run_status(workspace: &std::path::Path, config: &Config) -> Result<(), std::io::Error> {
    let layout = crate::commands::make_layout(workspace, config);
    let socket = crate::tmux::client::socket_for(workspace);
    let tmux = TmuxClient::new(&socket, None);
    let coord_dir = crate::commands::coord_dir_for(workspace, config);
    let store = CoordStore::new(coord_dir);
    let ws = workspace.to_path_buf();
    let autosave = Arc::new(move || {
        let _ = crate::commands::autosave(&layout, &ws);
    });
    let mut app = StatusApp::new(
        workspace.to_path_buf(),
        store,
        Some(tmux),
        Some(config.clone()),
    )
    .with_autosave(autosave, AUTOSAVE_SECONDS);
    run_app_in_terminal(&mut app)
}

fn run_app_in_terminal(app: &mut StatusApp) -> Result<(), std::io::Error> {
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
