use std::collections::BTreeSet;
use std::fmt;

use serde::Serialize;

const SECRET_KEY_FRAGMENTS: &[&str] = &[
    "secret",
    "token",
    "password",
    "passwd",
    "api_key",
    "apikey",
    "private_key",
    "credential",
];

const HASH_CONTEXT_KEYS: &[&str] = &[
    "sha256",
    "plan_sha256",
    "workflow_sha256",
    "commit",
    "commit_id",
    "base_commit",
    "head_commit",
    "hash",
    "digest",
    "checksum",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    Token,
    PrivateKey,
    UrlCredential,
    KeyValue,
    EnvSecret,
    HighEntropy,
}

impl SecretKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Token => "token",
            Self::PrivateKey => "private_key",
            Self::UrlCredential => "url_credential",
            Self::KeyValue => "key_value",
            Self::EnvSecret => "env_secret",
            Self::HighEntropy => "high_entropy",
        }
    }

    pub fn placeholder(self) -> String {
        format!("«redacted:{}»", self.as_str())
    }

    fn priority(self) -> u8 {
        match self {
            Self::PrivateKey => 6,
            Self::EnvSecret => 5,
            Self::Token => 4,
            Self::UrlCredential => 3,
            Self::KeyValue => 2,
            Self::HighEntropy => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redacted {
    text: String,
    replacements: usize,
}

impl Redacted {
    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn into_string(self) -> String {
        self.text
    }

    pub fn replacements(&self) -> usize {
        self.replacements
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

impl AsRef<str> for Redacted {
    fn as_ref(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for Redacted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Serialize for Redacted {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.text)
    }
}

#[derive(Debug, Clone, Default)]
pub struct RedactContext {
    env_secrets: Vec<String>,
}

impl RedactContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_secret_key_name(name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        SECRET_KEY_FRAGMENTS.iter().any(|frag| lower.contains(frag))
    }

    pub fn from_env() -> Self {
        Self::from_pairs_owned(std::env::vars())
    }

    pub fn from_pairs<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        Self::from_pairs_owned(
            pairs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string())),
        )
    }

    pub fn with_env_secrets(values: impl IntoIterator<Item = String>) -> Self {
        let mut set: BTreeSet<String> = values.into_iter().filter(|v| v.len() >= 8).collect();
        let mut env_secrets: Vec<String> = set.iter().cloned().collect();
        set.clear();
        env_secrets.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        Self { env_secrets }
    }

    fn from_pairs_owned(pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        let values = pairs
            .into_iter()
            .filter(|(k, v)| Self::is_secret_key_name(k) && v.len() >= 8)
            .map(|(_, v)| v);
        Self::with_env_secrets(values)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RedactMatch {
    pub kind: SecretKind,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
    kind: SecretKind,
}

pub fn redact(text: &str, context: &RedactContext) -> Redacted {
    inspect(text, context).0
}

pub fn inspect(text: &str, context: &RedactContext) -> (Redacted, Vec<RedactMatch>) {
    if text.is_empty() {
        return (
            Redacted {
                text: String::new(),
                replacements: 0,
            },
            Vec::new(),
        );
    }

    let mut spans = Vec::new();
    scan_private_keys(text, &mut spans);
    scan_env_secrets(text, &context.env_secrets, &mut spans);
    scan_fixed_tokens(text, &mut spans);
    scan_url_credentials(text, &mut spans);
    scan_key_value_pairs(text, &mut spans);
    scan_high_entropy(text, &mut spans);

    let accepted = resolve_overlaps(spans);
    if accepted.is_empty() {
        return (
            Redacted {
                text: text.to_string(),
                replacements: 0,
            },
            Vec::new(),
        );
    }

    let line_starts = compute_line_starts(text);
    let mut out = String::with_capacity(text.len());
    let mut matches = Vec::with_capacity(accepted.len());
    let mut cursor = 0usize;

    for span in &accepted {
        out.push_str(&text[cursor..span.start]);
        out.push_str(&span.kind.placeholder());
        cursor = span.end;
        matches.push(RedactMatch {
            kind: span.kind,
            line: line_for_offset(&line_starts, span.start),
        });
    }
    out.push_str(&text[cursor..]);

    (
        Redacted {
            text: out,
            replacements: accepted.len(),
        },
        matches,
    )
}

fn compute_line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (idx, b) in text.bytes().enumerate() {
        if b == b'\n' {
            starts.push(idx + 1);
        }
    }
    starts
}

fn line_for_offset(line_starts: &[usize], offset: usize) -> usize {
    match line_starts.binary_search(&offset) {
        Ok(idx) => idx + 1,
        Err(idx) => idx,
    }
}

fn resolve_overlaps(mut spans: Vec<Span>) -> Vec<Span> {
    spans.retain(|s| s.start < s.end);
    spans.sort_by(|a, b| {
        b.kind
            .priority()
            .cmp(&a.kind.priority())
            .then_with(|| a.start.cmp(&b.start))
            .then_with(|| b.end.cmp(&a.end))
    });

    let mut accepted: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        if accepted
            .iter()
            .any(|prev| span.start < prev.end && prev.start < span.end)
        {
            continue;
        }
        accepted.push(span);
    }
    accepted.sort_by_key(|s| s.start);
    accepted
}

fn is_token_boundary_before(bytes: &[u8], idx: usize) -> bool {
    idx == 0 || !(bytes[idx - 1].is_ascii_alphanumeric() || bytes[idx - 1] == b'_')
}

fn is_token_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')
}

fn is_bearer_token_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'+' | b'-' | b'/' | b'=')
}

fn scan_fixed_tokens(text: &str, spans: &mut Vec<Span>) {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;

    while i < len {
        if text[i..].starts_with("«redacted:")
            && let Some(end) = text[i..].find('»')
        {
            i += end + '»'.len_utf8();
            continue;
        }

        if is_token_boundary_before(bytes, i) {
            if let Some(prefix_len) = match_token_prefix(&text[i..]) {
                let mut end = i + prefix_len;
                while end < len && is_token_char(bytes[end]) {
                    end += 1;
                }
                let min_suffix = if text[i..].starts_with("AKIA") {
                    12
                } else if text[i..].starts_with("sk-") {
                    8
                } else {
                    4
                };
                if end - (i + prefix_len) >= min_suffix {
                    spans.push(Span {
                        start: i,
                        end,
                        kind: SecretKind::Token,
                    });
                    i = end;
                    continue;
                }
            }

            if len - i >= 7 && bytes[i..i + 7].eq_ignore_ascii_case(b"bearer ") {
                let tok_start = i + 7;
                let mut end = tok_start;
                while end < len && is_bearer_token_char(bytes[end]) {
                    end += 1;
                }
                if end - tok_start >= 8 {
                    spans.push(Span {
                        start: i,
                        end,
                        kind: SecretKind::Token,
                    });
                    i = end;
                    continue;
                }
            }
        }

        let ch_len = text[i..].chars().next().map_or(1, char::len_utf8);
        i += ch_len;
    }
}

fn match_token_prefix(rest: &str) -> Option<usize> {
    const PREFIXES: &[&str] = &[
        "github_pat_",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "xoxr-",
        "xoxs-",
        "glpat-",
        "AKIA",
        "AIza",
        "sk-",
    ];
    PREFIXES
        .iter()
        .find(|p| rest.starts_with(**p))
        .map(|p| p.len())
}

fn scan_private_keys(text: &str, spans: &mut Vec<Span>) {
    let mut pos = 0usize;
    while let Some(rel_begin) = text[pos..].find("-----BEGIN ") {
        let begin = pos + rel_begin;
        let line_end = text[begin..].find('\n').map_or(text.len(), |n| begin + n);
        let header = &text[begin..line_end];
        if !header.contains("PRIVATE KEY-----") {
            pos = line_end;
            continue;
        }
        if let Some(rel_end_marker) = text[line_end..].find("-----END ") {
            let end_marker = line_end + rel_end_marker;
            let end_line_end = text[end_marker..]
                .find('\n')
                .map_or(text.len(), |n| end_marker + n);
            if text[end_marker..end_line_end].contains("PRIVATE KEY-----") {
                spans.push(Span {
                    start: begin,
                    end: end_line_end,
                    kind: SecretKind::PrivateKey,
                });
                pos = end_line_end;
                continue;
            }
        }
        spans.push(Span {
            start: begin,
            end: line_end,
            kind: SecretKind::PrivateKey,
        });
        pos = line_end;
    }
}

fn scan_url_credentials(text: &str, spans: &mut Vec<Span>) {
    let bytes = text.as_bytes();
    let mut pos = 0usize;
    while let Some(rel_sep) = text[pos..].find("://") {
        let sep = pos + rel_sep;
        let mut scheme_start = sep;
        while scheme_start > 0
            && (bytes[scheme_start - 1].is_ascii_alphanumeric()
                || matches!(bytes[scheme_start - 1], b'+' | b'.' | b'-'))
        {
            scheme_start -= 1;
        }
        if scheme_start == sep || !bytes[scheme_start].is_ascii_alphabetic() {
            pos = sep + 3;
            continue;
        }

        let auth_start = sep + 3;
        let mut i = auth_start;
        let mut at_pos = None;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'@' {
                at_pos = Some(i);
                break;
            }
            if b.is_ascii_whitespace()
                || matches!(
                    b,
                    b'/' | b'?' | b'#' | b'"' | b'\'' | b'>' | b')' | b']' | b','
                )
            {
                break;
            }
            i += 1;
        }

        if let Some(at) = at_pos {
            let userinfo = &text[auth_start..at];
            if userinfo.contains(':') && !userinfo.contains("«redacted:") {
                spans.push(Span {
                    start: auth_start,
                    end: at,
                    kind: SecretKind::UrlCredential,
                });
            }
            pos = at + 1;
        } else {
            pos = auth_start;
        }
    }
}

fn scan_env_secrets(text: &str, env_secrets: &[String], spans: &mut Vec<Span>) {
    for secret in env_secrets {
        if secret.len() < 8 {
            continue;
        }
        for (idx, _) in text.match_indices(secret.as_str()) {
            spans.push(Span {
                start: idx,
                end: idx + secret.len(),
                kind: SecretKind::EnvSecret,
            });
        }
    }
}

fn scan_key_value_pairs(text: &str, spans: &mut Vec<Span>) {
    let mut line_offset = 0usize;
    for raw_line in text.split('\n') {
        scan_key_value_line(raw_line, line_offset, spans);
        line_offset += raw_line.len() + 1;
    }
}

fn scan_key_value_line(line: &str, base_offset: usize, spans: &mut Vec<Span>) {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;

    while i < len {
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            let key_start = i + 1;
            if let Some(rel_close) = line[key_start..].find(quote as char) {
                let key_end = key_start + rel_close;
                let key = &line[key_start..key_end];
                let mut after = key_end + 1;
                while after < len && bytes[after].is_ascii_whitespace() {
                    after += 1;
                }
                if after < len
                    && matches!(bytes[after], b':' | b'=')
                    && is_valid_key_token(key)
                    && RedactContext::is_secret_key_name(key)
                    && let Some((v_start, v_end)) = extract_value_span(line, after + 1)
                {
                    spans.push(Span {
                        start: base_offset + v_start,
                        end: base_offset + v_end,
                        kind: SecretKind::KeyValue,
                    });
                    i = if v_end < len && matches!(bytes[v_end], b'"' | b'\'') {
                        v_end + 1
                    } else {
                        v_end
                    };
                    continue;
                }
                i = key_end + 1;
                continue;
            }
        }

        if is_key_char(bytes[i]) && (i == 0 || !is_key_char(bytes[i - 1])) {
            let key_start = i;
            let mut key_end = i;
            while key_end < len && is_key_char(bytes[key_end]) {
                key_end += 1;
            }
            let key = &line[key_start..key_end];
            let mut after = key_end;
            while after < len && bytes[after] == b' ' {
                after += 1;
            }
            let is_sep = after < len
                && (bytes[after] == b'='
                    || (bytes[after] == b':' && (after + 1 >= len || bytes[after + 1] != b'/')));
            if is_sep
                && RedactContext::is_secret_key_name(key)
                && let Some((v_start, v_end)) = extract_value_span(line, after + 1)
            {
                spans.push(Span {
                    start: base_offset + v_start,
                    end: base_offset + v_end,
                    kind: SecretKind::KeyValue,
                });
                i = v_end;
                continue;
            }
            i = key_end;
            continue;
        }

        i += line[i..].chars().next().map_or(1, char::len_utf8);
    }
}

fn is_key_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')
}

fn is_valid_key_token(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(is_key_char)
}

fn extract_value_span(line: &str, mut pos: usize) -> Option<(usize, usize)> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    while pos < len && matches!(bytes[pos], b' ' | b'\t') {
        pos += 1;
    }
    if pos >= len {
        return None;
    }

    let (start, end) = if bytes[pos] == b'"' || bytes[pos] == b'\'' {
        let quote = bytes[pos];
        let inner_start = pos + 1;
        let mut j = inner_start;
        while j < len {
            if bytes[j] == b'\\' && j + 1 < len {
                j += 2;
                continue;
            }
            if bytes[j] == quote {
                break;
            }
            j += 1;
        }
        (inner_start, j)
    } else {
        let mut j = pos;
        while j < len
            && !bytes[j].is_ascii_whitespace()
            && !matches!(bytes[j], b',' | b'}' | b']' | b';' | b'#')
        {
            j += 1;
        }
        (pos, j)
    };

    if start >= end {
        return None;
    }
    let val = line[start..end].trim();
    if val.is_empty() || val.starts_with("«redacted:") || matches!(val, "null" | "true" | "false")
    {
        return None;
    }
    Some((start, end))
}

fn is_base64_or_hex_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'_' | b'-')
}

fn scan_high_entropy(text: &str, spans: &mut Vec<Span>) {
    let mut line_offset = 0usize;
    for raw_line in text.split('\n') {
        scan_high_entropy_line(raw_line, line_offset, spans);
        line_offset += raw_line.len() + 1;
    }
}

fn scan_high_entropy_line(line: &str, base_offset: usize, spans: &mut Vec<Span>) {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;

    while i < len {
        if line[i..].starts_with("«redacted:")
            && let Some(end) = line[i..].find('»')
        {
            i += end + '»'.len_utf8();
            continue;
        }

        if is_base64_or_hex_char(bytes[i]) {
            let start = i;
            let mut end = i;
            while end < len && is_base64_or_hex_char(bytes[end]) {
                end += 1;
            }
            while end < len && bytes[end] == b'=' {
                end += 1;
            }
            let candidate = &line[start..end];
            if candidate.len() >= 32
                && !candidate.starts_with('/')
                && !is_plain_identifier(candidate)
                && is_high_entropy_secret(candidate)
                && !is_in_hash_field(&line[..start])
            {
                spans.push(Span {
                    start: base_offset + start,
                    end: base_offset + end,
                    kind: SecretKind::HighEntropy,
                });
            }
            i = end;
            continue;
        }

        i += line[i..].chars().next().map_or(1, char::len_utf8);
    }
}

fn is_plain_identifier(s: &str) -> bool {
    let has_sep = s.contains('_') || s.contains('-');
    if !has_sep {
        return false;
    }
    let has_upper = s.bytes().any(|b| b.is_ascii_uppercase());
    let has_digit = s.bytes().any(|b| b.is_ascii_digit());
    !(has_upper && has_digit)
}

fn shannon_entropy(s: &str) -> f64 {
    let mut counts = [0usize; 256];
    for &b in s.as_bytes() {
        counts[b as usize] += 1;
    }
    let len = s.len() as f64;
    let mut entropy = 0.0f64;
    for &c in &counts {
        if c > 0 {
            let p = c as f64 / len;
            entropy -= p * p.log2();
        }
    }
    entropy
}

fn is_high_entropy_secret(s: &str) -> bool {
    let is_hex = s.bytes().all(|b| b.is_ascii_hexdigit());
    let entropy = shannon_entropy(s);
    if is_hex {
        return entropy >= 3.0;
    }
    if entropy < 3.5 {
        return false;
    }
    let mut classes = 0u8;
    if s.bytes().any(|b| b.is_ascii_uppercase()) {
        classes += 1;
    }
    if s.bytes().any(|b| b.is_ascii_lowercase()) {
        classes += 1;
    }
    if s.bytes().any(|b| b.is_ascii_digit()) {
        classes += 1;
    }
    if s.bytes().any(|b| matches!(b, b'+' | b'/' | b'=')) {
        classes += 1;
    }
    classes >= 2
}

fn is_in_hash_field(prefix_on_line: &str) -> bool {
    let trimmed = prefix_on_line.trim_end_matches([' ', '\t', '"', '\'', ':', '=']);
    let lower = trimmed.to_ascii_lowercase();
    if lower.ends_with("..") || lower.ends_with("from") {
        return true;
    }
    let last_token = lower
        .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .next()
        .unwrap_or("");
    HASH_CONTEXT_KEYS.contains(&last_token)
}
