use tandem::tmux::grid::{Breakpoint, breakpoint};
use tandem::tmux::keys::{default_keymap, mouse_commands, resize_commands};

#[test]
fn test_breakpoint_boundaries() {
    assert_eq!(breakpoint(120, 30), Breakpoint::Wide);
    assert_eq!(breakpoint(200, 60), Breakpoint::Wide);

    assert_eq!(breakpoint(120, 29), Breakpoint::Medium);
    assert_eq!(breakpoint(119, 30), Breakpoint::Medium);
    assert_eq!(breakpoint(80, 20), Breakpoint::Medium);
    assert_eq!(breakpoint(100, 25), Breakpoint::Medium);

    assert_eq!(breakpoint(80, 19), Breakpoint::Narrow);
    assert_eq!(breakpoint(79, 20), Breakpoint::Narrow);
    assert_eq!(breakpoint(79, 19), Breakpoint::Narrow);
    assert_eq!(breakpoint(150, 15), Breakpoint::Narrow);
    assert_eq!(breakpoint(50, 40), Breakpoint::Narrow);
}

#[test]
fn test_breakpoint_properties() {
    assert_eq!(Breakpoint::Wide.default_sidebar_cols(), Some(26));
    assert_eq!(Breakpoint::Wide.max_grid_cols(), 4);
    assert_eq!(Breakpoint::Wide.popup_percent(), (70, 70));

    assert_eq!(Breakpoint::Medium.default_sidebar_cols(), Some(14));
    assert_eq!(Breakpoint::Medium.max_grid_cols(), 2);
    assert_eq!(Breakpoint::Medium.popup_percent(), (90, 90));

    assert_eq!(Breakpoint::Narrow.default_sidebar_cols(), None);
    assert_eq!(Breakpoint::Narrow.max_grid_cols(), 1);
    assert_eq!(Breakpoint::Narrow.popup_percent(), (100, 100));
}

#[test]
fn test_mouse_commands_border_focus_and_double_click_equalize() {
    let km = default_keymap();
    let cmds = mouse_commands(None, None, "linux", &km);

    let click1_border = cmds
        .iter()
        .find(|c| c.len() >= 4 && c[2] == "MouseDown1Border")
        .expect("MouseDown1Border command found");
    assert_eq!(click1_border[3], "select-pane -t =");

    let dclick1_border = cmds
        .iter()
        .find(|c| c.len() >= 4 && c[2] == "DoubleClick1Border")
        .expect("DoubleClick1Border command found");
    assert!(dclick1_border[3].contains("select-pane -t ="));
    assert!(dclick1_border[3].contains("equalize"));
}

#[test]
fn test_resize_commands_include_e_equalize() {
    let cmds = resize_commands(None, None);
    let e_cmd = cmds
        .iter()
        .find(|c| c.len() >= 5 && c[3] == "e")
        .expect("resize command 'e' found");
    assert!(e_cmd[4].contains("equalize"));
}

#[test]
fn test_footer_wrapping_and_truncating() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::widgets::Widget;
    use tandem::ui::footer::{Footer, FooterItem};

    let items = vec![
        FooterItem::new("Opt+h", "Help"),
        FooterItem::new("Ctrl+Opt+n", "New agent"),
        FooterItem::new("Opt+1", "Planner"),
        FooterItem::new("Opt+2", "Implementer"),
        FooterItem::new("Ctrl+Opt+z", "Resize"),
    ];

    let backend = TestBackend::new(30, 2);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let footer = Footer::new(&items);
            footer.render(f.area(), f.buffer_mut());
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let line0: String = (0..30).map(|x| buf[(x, 0)].symbol()).collect();
    let line1: String = (0..30).map(|x| buf[(x, 1)].symbol()).collect();

    assert!(line0.contains("Opt+h Help"));
    assert!(line1.contains("Opt+1 Planner") || line1.contains("Ctrl+Opt+n"));
}

#[test]
fn test_tab_bar_tiers() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::widgets::Widget;
    use tandem::status::app::StatusTab;
    use tandem::ui::tabs::TabBar;

    let bar = TabBar::from_tabs(&StatusTab::ALL, 2, StatusTab::Status);

    let b_wide = TestBackend::new(140, 1);
    let mut t_wide = Terminal::new(b_wide).unwrap();
    t_wide
        .draw(|f| {
            (&bar).render(f.area(), f.buffer_mut());
        })
        .unwrap();
    let line_wide: String = (0..140)
        .map(|x| t_wide.backend().buffer()[(x, 0)].symbol())
        .collect();
    assert!(line_wide.contains("Status"));
    assert!(line_wide.contains("Queries (2)"));

    let b_med = TestBackend::new(100, 1);
    let mut t_med = Terminal::new(b_med).unwrap();
    t_med
        .draw(|f| {
            (&bar).render(f.area(), f.buffer_mut());
        })
        .unwrap();
    let line_med: String = (0..100)
        .map(|x| t_med.backend().buffer()[(x, 0)].symbol())
        .collect();
    assert!(line_med.contains("Sta"));
    assert!(line_med.contains("Q 2"));

    let b_narrow = TestBackend::new(60, 1);
    let mut t_narrow = Terminal::new(b_narrow).unwrap();
    t_narrow
        .draw(|f| {
            (&bar).render(f.area(), f.buffer_mut());
        })
        .unwrap();
    let line_narrow: String = (0..60)
        .map(|x| t_narrow.backend().buffer()[(x, 0)].symbol())
        .collect();
    assert!(line_narrow.contains("[1] Status"));
    assert!(line_narrow.contains("[5•]"));
}

#[test]
fn test_reflow_idempotent() {
    use std::sync::{Arc, Mutex};
    use tandem::tmux::client::{Result as TmuxResult, TmuxClient, TmuxRunner};
    use tandem::tmux::layout::Layout;

    struct MockRunner {
        calls: Mutex<Vec<Vec<String>>>,
        reflow_size: Mutex<String>,
        breakpoint: Mutex<String>,
    }

    impl TmuxRunner for MockRunner {
        fn run(&self, argv: &[String]) -> TmuxResult {
            self.calls.lock().unwrap().push(argv.to_vec());
            let joined = argv.join(" ");
            if joined.contains("display-message") && joined.contains("#{@tandem_reflow_size}") {
                let sz = self.reflow_size.lock().unwrap().clone();
                let bp = self.breakpoint.lock().unwrap().clone();
                return TmuxResult {
                    code: 0,
                    out: format!("{sz} {bp}").trim().to_string(),
                    err: String::new(),
                };
            }
            if joined.contains("set-option") {
                for i in 0..argv.len() {
                    if argv[i] == "@tandem_reflow_size" && i + 1 < argv.len() {
                        *self.reflow_size.lock().unwrap() = argv[i + 1].clone();
                    } else if argv[i] == "@tandem_breakpoint" && i + 1 < argv.len() {
                        *self.breakpoint.lock().unwrap() = argv[i + 1].clone();
                    }
                }
            }
            if joined.contains("list-panes") {
                let p1 = "%1\tmain\town\tsidebar\t0\tsidebar\t0\t\t\t\t";
                let p2 = "%2\tmain\tagent\tplanner\t1\tclaude·opus\t0\t\t\t\t";
                let p3 = "%3\tmain\town\tstatus\t0\t\t0\t\t\t\t";
                return TmuxResult {
                    code: 0,
                    out: format!("{p1}\n{p2}\n{p3}\n"),
                    err: String::new(),
                };
            }
            TmuxResult {
                code: 0,
                out: String::new(),
                err: String::new(),
            }
        }
    }

    let runner = Arc::new(MockRunner {
        calls: Mutex::new(Vec::new()),
        reflow_size: Mutex::new(String::new()),
        breakpoint: Mutex::new(String::new()),
    });
    let client = TmuxClient::with_runner("tandem-test", None, runner.clone());
    let layout = Layout::new(&client);

    layout.reflow(120, 40).unwrap();
    let first_count = runner.calls.lock().unwrap().len();
    assert!(first_count > 0);

    runner.calls.lock().unwrap().clear();
    layout.reflow(120, 40).unwrap();
    let second_count = runner.calls.lock().unwrap().len();
    assert_eq!(second_count, 1);
}
