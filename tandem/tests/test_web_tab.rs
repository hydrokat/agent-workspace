use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tandem::net_interfaces::{IpCandidate, NetworkSource};
use tandem::status::qr::{render_qr_lines, required_size};
use tandem::status::web_tab::{
    ROW_BIND, ROW_CONNECT, ROW_ENABLE, ROW_PORT, ROW_REGENERATE, ROW_TERMINAL, ROW_TOKEN,
    WebAction, WebTab,
};
use tandem::web_manager::{WebConfig, WebStatus, access_url};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn click(col: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn candidates() -> Vec<IpCandidate> {
    vec![
        IpCandidate {
            ip: "100.101.102.103".into(),
            interface: "tailscale0".into(),
            source: NetworkSource::Tailscale,
            preferred: true,
        },
        IpCandidate {
            ip: "192.168.1.50".into(),
            interface: "en0".into(),
            source: NetworkSource::Lan,
            preferred: false,
        },
        IpCandidate {
            ip: "127.0.0.1".into(),
            interface: "lo".into(),
            source: NetworkSource::Localhost,
            preferred: false,
        },
    ]
}

fn tab() -> WebTab {
    let mut t = WebTab::default();
    t.set_ip_candidates(candidates());
    t
}

fn acknowledged(enabled: bool) -> WebTab {
    let mut t = tab();
    let mut cfg = t.web_config.clone();
    cfg.first_time_acknowledged = true;
    cfg.enabled = enabled;
    t.set_web_config(cfg);
    t
}

fn text(buf: &Buffer) -> String {
    let area = buf.area;
    (area.y..area.y + area.height)
        .map(|y| {
            (area.x..area.x + area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render(t: &WebTab, w: u16, h: u16) -> String {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    t.render(area, &mut buf);
    text(&buf)
}

#[test]
fn row_constants_are_contiguous() {
    assert_eq!(
        [
            ROW_ENABLE,
            ROW_BIND,
            ROW_PORT,
            ROW_TERMINAL,
            ROW_TOKEN,
            ROW_CONNECT,
            ROW_REGENERATE
        ],
        [0, 1, 2, 3, 4, 5, 6]
    );
}

#[test]
fn navigation_wraps_over_seven_rows() {
    let mut t = tab();
    for i in 1..=6 {
        t.handle_key(&key(KeyCode::Down));
        assert_eq!(t.focused_row, i);
    }
    t.handle_key(&key(KeyCode::Down));
    assert_eq!(t.focused_row, 0);
    t.handle_key(&key(KeyCode::Up));
    assert_eq!(t.focused_row, 6);
}

#[test]
fn first_time_enable_opens_consent_and_confirm_enables() {
    let mut t = tab();
    t.focused_row = ROW_ENABLE;
    assert_eq!(t.handle_key(&key(KeyCode::Right)), None);
    assert!(t.confirm_web_modal);
    t.handle_key(&key(KeyCode::Esc));
    assert!(!t.confirm_web_modal && !t.web_config.enabled);

    t.handle_key(&key(KeyCode::Right));
    assert_eq!(t.handle_key(&key(KeyCode::Enter)), Some(WebAction::Changed));
    assert!(t.web_config.enabled && t.web_config.first_time_acknowledged);
}

#[test]
fn consent_decline_leaves_web_off() {
    let mut t = tab();
    t.focused_row = ROW_ENABLE;
    t.handle_key(&key(KeyCode::Right));
    t.handle_key(&key(KeyCode::Tab));
    assert_eq!(t.handle_key(&key(KeyCode::Enter)), None);
    assert!(!t.web_config.enabled);
}

#[test]
fn acknowledged_enable_and_disable_toggle_without_modal() {
    let mut t = acknowledged(false);
    t.focused_row = ROW_ENABLE;
    assert_eq!(t.handle_key(&key(KeyCode::Enter)), Some(WebAction::Changed));
    assert!(t.web_config.enabled && !t.confirm_web_modal);
    assert_eq!(t.handle_key(&key(KeyCode::Enter)), Some(WebAction::Changed));
    assert!(!t.web_config.enabled);
}

#[test]
fn bind_ip_cycles_candidates_and_updates_source() {
    let mut t = tab();
    t.focused_row = ROW_BIND;
    let first = t.web_config.bind_ip.clone();
    assert_eq!(first, "100.101.102.103");
    assert_eq!(t.handle_key(&key(KeyCode::Right)), Some(WebAction::Changed));
    assert_eq!(t.web_config.bind_ip, "192.168.1.50");
    assert_eq!(t.web_config.bind_source, NetworkSource::Lan);
    t.handle_key(&key(KeyCode::Left));
    assert_eq!(t.web_config.bind_ip, first);
}

#[test]
fn bind_ip_without_candidates_is_inert() {
    let mut t = WebTab {
        focused_row: ROW_BIND,
        ..WebTab::default()
    };
    assert_eq!(t.handle_key(&key(KeyCode::Right)), None);
}

#[test]
fn port_edit_commits_valid_value() {
    let mut t = tab();
    t.focused_row = ROW_PORT;
    assert_eq!(t.handle_key(&key(KeyCode::Enter)), None);
    assert!(t.editing_port.is_some());
    for _ in 0..4 {
        t.handle_key(&key(KeyCode::Backspace));
    }
    for c in "4100".chars() {
        t.handle_key(&key(KeyCode::Char(c)));
    }
    t.handle_key(&key(KeyCode::Char('x')));
    assert_eq!(t.handle_key(&key(KeyCode::Enter)), Some(WebAction::Changed));
    assert_eq!(t.web_config.port, 4100);
    assert!(t.editing_port.is_none());
}

#[test]
fn port_edit_rejects_out_of_range_and_empty() {
    for bad in ["80", "1023", "65536", "99999", ""] {
        let mut t = tab();
        t.focused_row = ROW_PORT;
        let before = t.web_config.port;
        t.handle_key(&key(KeyCode::Enter));
        for _ in 0..6 {
            t.handle_key(&key(KeyCode::Backspace));
        }
        for c in bad.chars() {
            t.handle_key(&key(KeyCode::Char(c)));
        }
        assert_eq!(t.handle_key(&key(KeyCode::Enter)), None, "port {bad:?}");
        assert_eq!(t.web_config.port, before);
        assert!(t.flash_message.is_some(), "rejection must be explained");
    }
}

#[test]
fn port_edit_escape_cancels() {
    let mut t = tab();
    t.focused_row = ROW_PORT;
    let before = t.web_config.port;
    t.handle_key(&key(KeyCode::Enter));
    t.handle_key(&key(KeyCode::Char('9')));
    assert_eq!(t.handle_key(&key(KeyCode::Esc)), None);
    assert!(t.editing_port.is_none());
    assert_eq!(t.web_config.port, before);
}

#[test]
fn navigation_keys_do_not_leak_while_editing_port() {
    let mut t = tab();
    t.focused_row = ROW_PORT;
    t.handle_key(&key(KeyCode::Enter));
    t.handle_key(&key(KeyCode::Down));
    assert_eq!(t.focused_row, ROW_PORT);
}

#[test]
fn terminal_toggle_flips_ttyd() {
    let mut t = tab();
    t.focused_row = ROW_TERMINAL;
    assert!(t.web_config.ttyd_enabled);
    assert_eq!(t.handle_key(&key(KeyCode::Enter)), Some(WebAction::Changed));
    assert!(!t.web_config.ttyd_enabled);
    assert_eq!(t.handle_key(&key(KeyCode::Enter)), Some(WebAction::Changed));
    assert!(t.web_config.ttyd_enabled);
}

#[test]
fn token_is_masked_until_revealed() {
    let mut t = tab();
    let token = t.web_config.auth_token.clone();
    let hidden = render(&t, 100, 10);
    assert!(!hidden.contains(&token), "token must not render by default");

    t.focused_row = ROW_TOKEN;
    assert_eq!(t.handle_key(&key(KeyCode::Enter)), None);
    assert!(t.token_revealed);
    assert!(render(&t, 100, 10).contains(&token));
    t.handle_key(&key(KeyCode::Enter));
    assert!(!t.token_revealed);
}

#[test]
fn regenerate_requires_confirmation_and_rotates_token() {
    let mut t = acknowledged(true);
    t.focused_row = ROW_REGENERATE;
    let old = t.web_config.auth_token.clone();

    assert_eq!(t.handle_key(&key(KeyCode::Enter)), None);
    assert!(t.confirm_regenerate);
    assert_eq!(t.handle_key(&key(KeyCode::Esc)), None);
    assert_eq!(t.web_config.auth_token, old);

    t.handle_key(&key(KeyCode::Enter));
    t.handle_key(&key(KeyCode::Left));
    assert_eq!(
        t.handle_key(&key(KeyCode::Enter)),
        Some(WebAction::Regenerated)
    );
    let new = t.web_config.auth_token.clone();
    assert_ne!(new, old);
    assert_eq!(new.len(), "tandem_".len() + 32);
    assert!(
        new.starts_with("tandem_")
            && new["tandem_".len()..]
                .chars()
                .all(|c| c.is_ascii_hexdigit())
    );
}

#[test]
fn default_modal_focus_is_cancel_for_regenerate() {
    let mut t = acknowledged(true);
    t.focused_row = ROW_REGENERATE;
    let old = t.web_config.auth_token.clone();
    t.handle_key(&key(KeyCode::Enter));
    assert_eq!(t.handle_key(&key(KeyCode::Enter)), None);
    assert_eq!(t.web_config.auth_token, old);
}

#[test]
fn connect_requires_running_server() {
    let mut t = acknowledged(false);
    t.focused_row = ROW_CONNECT;
    assert_eq!(t.handle_key(&key(KeyCode::Enter)), None);
    assert!(!t.qr_open);
    assert!(
        t.flash_message
            .as_ref()
            .unwrap()
            .0
            .to_lowercase()
            .contains("enable")
    );
}

#[test]
fn connect_refuses_loopback_bind_with_hint() {
    let mut t = acknowledged(true);
    let mut cfg = t.web_config.clone();
    cfg.bind_ip = "127.0.0.1".into();
    t.set_web_config(cfg);
    t.web_status = WebStatus::Running {
        url: "http://127.0.0.1:3940".into(),
        pid: Some(1),
    };
    t.focused_row = ROW_CONNECT;
    t.handle_key(&key(KeyCode::Enter));
    assert!(!t.qr_open);
    assert!(t.flash_message.as_ref().unwrap().0.contains("VPN"));
}

#[test]
fn connect_opens_qr_overlay_and_escape_closes_it() {
    let mut t = acknowledged(true);
    t.web_status = WebStatus::Running {
        url: "http://100.101.102.103:3940".into(),
        pid: Some(1),
    };
    t.focused_row = ROW_CONNECT;
    t.handle_key(&key(KeyCode::Enter));
    assert!(t.qr_open);
    t.handle_key(&key(KeyCode::Down));
    assert_eq!(t.focused_row, ROW_CONNECT, "overlay owns input");
    t.handle_key(&key(KeyCode::Esc));
    assert!(!t.qr_open);
}

#[test]
fn qr_overlay_renders_blocks_when_it_fits_and_reports_size_when_not() {
    let mut t = acknowledged(true);
    t.web_status = WebStatus::Running {
        url: "http://100.101.102.103:3940".into(),
        pid: Some(1),
    };
    t.qr_open = true;
    let url = access_url(&t.web_config, true);
    let (w, h) = required_size(&url);

    let big = render(&t, w + 10, h + 4);
    assert!(big.contains('█') || big.contains('▀') || big.contains('▄'));
    assert!(!big.contains("Enable web"), "overlay replaces the rows");

    let small = render(&t, 40, 7);
    assert!(
        small.contains(&format!("{w}x{h}")),
        "small pane must state required size:\n{small}"
    );
}

#[test]
fn qr_overlay_never_prints_the_token_as_text() {
    let mut t = acknowledged(true);
    t.web_status = WebStatus::Running {
        url: "http://100.101.102.103:3940".into(),
        pid: Some(1),
    };
    t.qr_open = true;
    let out = render(&t, 120, 40);
    assert!(!out.contains(&t.web_config.auth_token));
}

#[test]
fn render_lists_all_settings_and_status() {
    let mut t = acknowledged(true);
    t.web_status = WebStatus::Running {
        url: "http://100.101.102.103:3940".into(),
        pid: Some(7),
    };
    let out = render(&t, 100, 10);
    for needle in [
        "Enable web",
        "Bind IP",
        "Port",
        "Terminal",
        "Token",
        "Connect",
        "Regenerate",
        "Running",
        "3940",
        "tailscale",
    ] {
        assert!(out.contains(needle), "missing {needle:?} in:\n{out}");
    }
}

#[test]
fn render_shows_start_error_text() {
    let mut t = acknowledged(true);
    t.web_status =
        WebStatus::Error("tandem-web is not built; run `agent-workspace web` once".into());
    let out = render(&t, 120, 10);
    assert!(out.contains("not built"), "{out}");
}

#[test]
fn render_never_panics_at_any_size() {
    let mut t = acknowledged(true);
    t.web_status = WebStatus::Running {
        url: "http://100.101.102.103:3940".into(),
        pid: Some(1),
    };
    t.token_revealed = true;
    for qr in [false, true] {
        t.qr_open = qr;
        for w in 0..=140u16 {
            for h in [0u16, 1, 3, 7, 12, 40] {
                let _ = render(&t, w, h);
            }
        }
    }
}

#[test]
fn mouse_click_selects_and_activates_rows() {
    let mut t = acknowledged(false);
    let area = Rect::new(0, 5, 100, 10);
    assert_eq!(
        t.handle_mouse(&click(10, 5 + ROW_ENABLE as u16), area),
        Some(WebAction::Changed)
    );
    assert!(t.web_config.enabled);
    assert_eq!(
        t.handle_mouse(&click(10, 5 + ROW_TERMINAL as u16), area),
        Some(WebAction::Changed)
    );
    assert!(!t.web_config.ttyd_enabled);
    t.handle_mouse(&click(10, 5 + ROW_REGENERATE as u16), area);
    assert!(t.confirm_regenerate);
}

#[test]
fn mouse_outside_rows_is_ignored() {
    let mut t = acknowledged(false);
    assert_eq!(
        t.handle_mouse(&click(10, 99), Rect::new(0, 0, 100, 10)),
        None
    );
}

#[test]
fn qr_lines_are_square_ish_block_art() {
    let lines = render_qr_lines(
        "http://100.101.102.103:3940/?token=tandem_0123456789abcdef0123456789abcdef",
    );
    assert!(lines.len() >= 10);
    let w = lines[0].chars().count();
    assert!(lines.iter().all(|l| l.chars().count() == w));
    assert!(lines.iter().all(|l| l.chars().all(|c| " ▀▄█".contains(c))));
    let (rw, rh) =
        required_size("http://100.101.102.103:3940/?token=tandem_0123456789abcdef0123456789abcdef");
    assert_eq!((rw as usize, rh as usize), (w, lines.len()));
}

#[test]
fn qr_is_deterministic_and_url_dependent() {
    let a = render_qr_lines("http://a.example:3940/?token=1");
    assert_eq!(a, render_qr_lines("http://a.example:3940/?token=1"));
    assert_ne!(a, render_qr_lines("http://a.example:3940/?token=2"));
}

#[test]
fn access_url_masks_or_includes_token() {
    let cfg = WebConfig {
        bind_ip: "100.64.0.2".into(),
        port: 4000,
        auth_token: "tandem_abc".into(),
        ..WebConfig::default()
    };
    assert_eq!(
        access_url(&cfg, true),
        "http://100.64.0.2:4000/?token=tandem_abc"
    );
    assert_eq!(access_url(&cfg, false), "http://100.64.0.2:4000/");
}
