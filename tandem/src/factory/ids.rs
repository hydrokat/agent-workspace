use uuid::Uuid;

const ID_ALPHABET: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";
const ID_RANDOM_LEN: usize = 8;

fn new_prefixed_id(prefix: &str) -> String {
    let uuid = Uuid::new_v4();
    let bytes = uuid.as_bytes();
    let mut out = String::with_capacity(prefix.len() + ID_RANDOM_LEN);
    out.push_str(prefix);
    for &b in &bytes[..ID_RANDOM_LEN] {
        out.push(ID_ALPHABET[(b as usize) % ID_ALPHABET.len()] as char);
    }
    out
}

fn is_valid_prefixed_id(id: &str, prefix: &str) -> bool {
    let Some(rest) = id.strip_prefix(prefix) else {
        return false;
    };
    rest.len() == ID_RANDOM_LEN
        && rest
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

pub fn new_idea_id() -> String {
    new_prefixed_id("i-")
}

pub fn new_run_id() -> String {
    new_prefixed_id("r-")
}

pub fn is_valid_idea_id(id: &str) -> bool {
    is_valid_prefixed_id(id, "i-")
}

pub fn is_valid_run_id(id: &str) -> bool {
    is_valid_prefixed_id(id, "r-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generated_ids_are_valid() {
        let idea = new_idea_id();
        assert!(idea.starts_with("i-"));
        assert_eq!(idea.len(), 10);
        assert!(is_valid_idea_id(&idea));

        let run = new_run_id();
        assert!(run.starts_with("r-"));
        assert_eq!(run.len(), 10);
        assert!(is_valid_run_id(&run));
    }

    #[test]
    fn test_invalid_ids_rejected() {
        assert!(!is_valid_idea_id("i-short"));
        assert!(!is_valid_idea_id("i-toolong12"));
        assert!(!is_valid_idea_id("i-Upper123"));
        assert!(!is_valid_idea_id("x-12345678"));
        assert!(!is_valid_run_id("i-12345678"));
        assert!(!is_valid_run_id("r-1234567!"));
    }
}
