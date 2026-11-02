pub fn clean(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n");
    let mut out = String::with_capacity(normalized.len());
    let chars: Vec<char> = normalized.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '\x1b' {
            if i + 1 < len && chars[i + 1] == ']' {
                // OSC: \x1b\][^\x07\x1b]*(?:\x07|\x1b\\)?
                i += 2;
                while i < len {
                    if chars[i] == '\x07' {
                        i += 1;
                        break;
                    }
                    if chars[i] == '\x1b' {
                        if i + 1 < len && chars[i + 1] == '\\' {
                            i += 2;
                        }
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            if i + 1 < len && chars[i + 1] == '[' {
                // CSI: \x1b\[[0-?]*[ -/]*[@-~]
                i += 2;
                while i < len && ('0'..='?').contains(&chars[i]) {
                    i += 1;
                }
                while i < len && (' '..='/').contains(&chars[i]) {
                    i += 1;
                }
                if i < len && ('@'..='~').contains(&chars[i]) {
                    i += 1;
                }
                continue;
            }
            // ESC: \x1b[@-_]?
            i += 1;
            if i < len && ('@'..='_').contains(&chars[i]) {
                i += 1;
            }
            continue;
        }

        let c = chars[i];
        let code = c as u32;

        // CTRL: [\x00-\x08\x0b-\x1f\x7f-\x9f]
        let is_ctrl =
            (code <= 0x08) || (0x0b..=0x1f).contains(&code) || (0x7f..=0x9f).contains(&code);

        if !is_ctrl {
            out.push(c);
        }
        i += 1;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_strips_controls_and_escapes() {
        assert_eq!(
            clean("a\x1b[31mb\x1b[0m\x1b]0;title\x07c\x00d\r\ne"),
            "abcd\ne"
        );
    }

    #[test]
    fn test_clean_preserves_tabs_and_newlines() {
        assert_eq!(clean("col1\tcol2\nrow2"), "col1\tcol2\nrow2");
    }

    #[test]
    fn test_clean_plain_text_untouched() {
        assert_eq!(clean("Hello, world!"), "Hello, world!");
    }
}
