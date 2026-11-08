use std::fs;
use tandem::web_manager::{WebConfig, web_is_built, web_server_args};

fn cfg(ip: &str, port: u16) -> WebConfig {
    WebConfig {
        bind_ip: ip.to_string(),
        port,
        ..WebConfig::default()
    }
}

#[test]
fn server_args_pass_bind_ip_and_port_explicitly() {
    let args = web_server_args(&cfg("100.64.0.2", 4000)).expect("args");
    assert_eq!(
        args,
        ["run", "start", "--", "-H", "100.64.0.2", "-p", "4000"]
    );
}

#[test]
fn server_args_refuse_wildcard_bind() {
    assert!(web_server_args(&cfg("0.0.0.0", 3940)).is_err());
    assert!(web_server_args(&cfg("::", 3940)).is_err());
}

#[test]
fn server_args_refuse_empty_token() {
    let mut c = cfg("100.64.0.2", 3940);
    c.auth_token = String::new();
    assert!(web_server_args(&c).is_err());
}

#[test]
fn web_is_built_requires_build_id() {
    let dir = std::env::temp_dir().join(format!("tw-built-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".next")).unwrap();
    assert!(!web_is_built(&dir));
    fs::write(dir.join(".next").join("BUILD_ID"), "x").unwrap();
    assert!(web_is_built(&dir));
    let _ = fs::remove_dir_all(&dir);
}
