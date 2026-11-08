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
