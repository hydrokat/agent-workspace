use qrcodegen::{QrCode, QrCodeEcc};

const QUIET_ZONE: i32 = 2;

fn encode(text: &str) -> Option<QrCode> {
    QrCode::encode_text(text, QrCodeEcc::Low).ok()
}

fn modules_wide(qr: &QrCode) -> i32 {
    qr.size() + 2 * QUIET_ZONE
}

pub fn required_size(text: &str) -> (u16, u16) {
    match encode(text) {
        Some(qr) => {
            let n = modules_wide(&qr);
            (n as u16, ((n + 1) / 2) as u16)
        }
        None => (0, 0),
    }
}

pub fn render_qr_lines(text: &str) -> Vec<String> {
    let Some(qr) = encode(text) else {
        return Vec::new();
    };
    let lo = -QUIET_ZONE;
    let hi = qr.size() + QUIET_ZONE;
    let dark = |x: i32, y: i32| {
        (0..qr.size()).contains(&x) && (0..qr.size()).contains(&y) && qr.get_module(x, y)
    };

    let mut lines = Vec::new();
    let mut y = lo;
    while y < hi {
        let mut line = String::new();
        for x in lo..hi {
            line.push(match (dark(x, y), dark(x, y + 1)) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            });
        }
        lines.push(line);
        y += 2;
    }
    lines
}

pub const QR_POPUP_CHROME_COLS: u16 = 4;
pub const QR_POPUP_CHROME_ROWS: u16 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QrPopupContent {
    Code {
        lines: Vec<String>,
        url: String,
    },
    TooSmall {
        url: String,
        need_cols: u16,
        need_rows: u16,
    },
}

pub fn qr_popup_content(
    url_for_qr: &str,
    url_for_display: &str,
    cols: u16,
    rows: u16,
) -> QrPopupContent {
    let (w, h) = required_size(url_for_qr);
    if w == 0 || cols < w || rows < h + 2 {
        return QrPopupContent::TooSmall {
            url: url_for_display.to_string(),
            need_cols: w + QR_POPUP_CHROME_COLS,
            need_rows: h + QR_POPUP_CHROME_ROWS,
        };
    }
    QrPopupContent::Code {
        lines: render_qr_lines(url_for_qr),
        url: url_for_display.to_string(),
    }
}

pub fn qr_popup_size(url_for_qr: &str) -> (u16, u16) {
    let (w, h) = required_size(url_for_qr);
    (w + QR_POPUP_CHROME_COLS, h + QR_POPUP_CHROME_ROWS)
}
