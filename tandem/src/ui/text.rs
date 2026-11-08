pub fn truncate_to_width(text: &str, max_cols: usize) -> &str {
    let mut cols = 0usize;
    for (idx, ch) in text.char_indices() {
        cols += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if cols > max_cols {
            return &text[..idx];
        }
    }
    text
}
