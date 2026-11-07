use crate::common::assert_golden;
use clap::CommandFactory;
use tandem::cli::Cli;

#[test]
fn test_golden_cli_root_help() {
    let mut cmd = Cli::command();
    let help = cmd.render_help().to_string();
    assert_golden("cli/root_help.txt", &help);
}

#[test]
fn test_golden_cli_subcommands_help() {
    let cmd = Cli::command();
    let mut sub_names: Vec<_> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    sub_names.sort();

    let mut all_sub_help = String::new();
    for name in &sub_names {
        let mut sub_cmd = cmd.find_subcommand(name).unwrap().clone();
        let help = sub_cmd.render_help().to_string();
        all_sub_help.push_str(&format!("=== SUBCOMMAND: {name} ===\n"));
        all_sub_help.push_str(&help);
        all_sub_help.push_str("\n\n");
    }
    assert_golden("cli/subcommands_help.txt", &all_sub_help);
}
