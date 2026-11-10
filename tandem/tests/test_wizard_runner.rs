use tandem::wizard::runner::{installed_clis, load_models};

#[test]
fn test_installed_clis_reports_every_known_key() {
    let clis = installed_clis();
    assert_eq!(clis.len(), 3);
    assert!(clis.contains_key("claude"));
    assert!(clis.contains_key("agy"));
    assert!(clis.contains_key("opencode"));
}

#[test]
fn test_load_models_for_claude_needs_no_subprocess() {
    let models = load_models("claude").unwrap();
    assert_eq!(models[0].id, "opus");
}
