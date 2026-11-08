#![allow(dead_code)]

use std::process::Command;

pub fn require_tool(name: &str) -> bool {
    let flag = if name == "tmux" { "-V" } else { "--version" };
    let available = match Command::new(name).arg(flag).output() {
        Ok(out) => out.status.success(),
        Err(_) => false,
    };
    if !available
        && std::env::var("TANDEM_TEST_STRICT")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    {
        panic!("required tool '{name}' is not available and TANDEM_TEST_STRICT is set");
    }
    available
}
