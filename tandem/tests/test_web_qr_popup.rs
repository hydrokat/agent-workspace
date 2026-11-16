use tandem::commands::render_qr_popup;
use tandem::status::qr::{
    QR_POPUP_CHROME_COLS, QR_POPUP_CHROME_ROWS, QrPopupContent, qr_popup_content, qr_popup_size,
    required_size,
};

#[test]
fn popup_size_adds_chrome_to_qr_size() {
    let url = "http://100.101.102.103:3940/?token=tandem_abc123&ws=sock";
    let (qr_w, qr_h) = required_size(url);
    assert!(qr_w > 0 && qr_h > 0);
    assert_eq!(
        qr_popup_size(url),
        (qr_w + QR_POPUP_CHROME_COLS, qr_h + QR_POPUP_CHROME_ROWS)
    );
}

#[test]
fn content_is_code_when_terminal_fits() {
    let qr_url = "http://100.101.102.103:3940/?token=tandem_abc123";
    let display_url = "http://100.101.102.103:3940";
    let (w, h) = required_size(qr_url);
    let content = qr_popup_content(qr_url, display_url, w, h + 2);
    match content {
        QrPopupContent::Code { lines, url } => {
            assert!(!lines.is_empty());
            assert_eq!(url, display_url);
        }
        QrPopupContent::TooSmall { .. } => panic!("expected Code when terminal fits"),
    }
}

#[test]
fn content_is_too_small_when_cols_short() {
    let qr_url = "http://100.101.102.103:3940/?token=tandem_abc123";
    let display_url = "http://100.101.102.103:3940";
    let (w, h) = required_size(qr_url);
    let content = qr_popup_content(qr_url, display_url, w - 1, h + 4);
    assert!(matches!(content, QrPopupContent::TooSmall { .. }));
}

#[test]
fn content_is_too_small_when_rows_short() {
    let qr_url = "http://100.101.102.103:3940/?token=tandem_abc123";
    let display_url = "http://100.101.102.103:3940";
    let (w, h) = required_size(qr_url);
    let content = qr_popup_content(qr_url, display_url, w + 4, h + 1);
    assert!(matches!(content, QrPopupContent::TooSmall { .. }));
}

#[test]
fn rendered_popup_never_contains_the_token() {
    let qr_url = "http://100.101.102.103:3940/?token=tandem_abc123";
    let display_url = "http://100.101.102.103:3940";

    let code = qr_popup_content(qr_url, display_url, 100, 50);
    let mut buf = Vec::new();
    render_qr_popup(&mut buf, &code, 100).unwrap();
    let rendered_code = String::from_utf8(buf).unwrap();
    assert!(!rendered_code.contains("tandem_abc123"));
    assert!(!rendered_code.contains("token="));

    let small = qr_popup_content(qr_url, display_url, 20, 10);
    let mut buf = Vec::new();
    render_qr_popup(&mut buf, &small, 20).unwrap();
    let rendered_small = String::from_utf8(buf).unwrap();
    assert!(!rendered_small.contains("tandem_abc123"));
    assert!(!rendered_small.contains("token="));
}

#[test]
fn rendered_code_lines_are_centred_and_end_with_hint() {
    let qr_url = "http://100.101.102.103:3940/?token=tandem_abc123";
    let display_url = "http://100.101.102.103:3940";
    let cols = 80;
    let content = qr_popup_content(qr_url, display_url, cols, 50);
    let mut buf = Vec::new();
    render_qr_popup(&mut buf, &content, cols).unwrap();
    let out = String::from_utf8(buf).unwrap();

    assert!(out.contains(display_url));
    assert!(out.contains("Scan with your phone camera · Esc to close"));

    let (qr_w, _) = required_size(qr_url);
    let expected_pad = " ".repeat(((cols - qr_w) / 2) as usize);
    let first_qr_line = out
        .lines()
        .find(|l| l.contains('█') || l.contains('▀') || l.contains('▄'))
        .expect("rendered output should contain QR block lines");
    assert!(
        first_qr_line.starts_with(&expected_pad),
        "expected line to be centred with {expected_pad:?}, got {first_qr_line:?}"
    );
}

#[test]
fn too_small_text_names_the_needed_size_and_the_address() {
    let qr_url = "http://100.101.102.103:3940/?token=tandem_abc123";
    let display_url = "http://100.101.102.103:3940";
    let (need_w, need_h) = qr_popup_size(qr_url);
    let content = qr_popup_content(qr_url, display_url, 20, 10);
    let mut buf = Vec::new();
    render_qr_popup(&mut buf, &content, 20).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains(&format!("{need_w}x{need_h}")), "{out}");
    assert!(out.contains(display_url), "{out}");
    assert!(out.contains("Press any key to close."), "{out}");
}
