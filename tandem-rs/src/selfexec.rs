pub fn is_frozen() -> bool {
    false
}

pub fn self_argv() -> Vec<String> {
    if let Ok(exe) = std::env::current_exe() {
        vec![exe.to_string_lossy().into_owned()]
    } else {
        vec!["tandem".to_string()]
    }
}

pub fn shlex_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    if s.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '/' || c == '-' || c == '+'
    }) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn self_command(self_cmd: Option<&[&str]>) -> String {
    if let Some(cmd) = self_cmd {
        cmd.iter()
            .map(|s| shlex_quote(s))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        self_argv()
            .iter()
            .map(|s| shlex_quote(s))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_frozen_false() {
        assert!(!is_frozen());
    }

    #[test]
    fn test_shlex_quote() {
        assert_eq!(shlex_quote(""), "''");
        assert_eq!(shlex_quote("simple"), "simple");
        assert_eq!(shlex_quote("/usr/bin/tandem"), "/usr/bin/tandem");
        assert_eq!(shlex_quote("with space"), "'with space'");
        assert_eq!(shlex_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_self_command_with_explicit_cmd() {
        let cmd = ["/usr/bin/tandem", "status"];
        assert_eq!(self_command(Some(&cmd)), "/usr/bin/tandem status");

        let cmd_with_space = ["/opt/my tandem/bin", "status"];
        assert_eq!(
            self_command(Some(&cmd_with_space)),
            "'/opt/my tandem/bin' status"
        );
    }
}
