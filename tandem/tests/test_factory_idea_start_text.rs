use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use clap::Parser;
use tandem::cli::{Cli, Command as TandemCommand};
use tandem::factory::commands::{FactoryCommand, FactoryIdeaCommand};
use tandem::factory::model::{IdeaState, TEXT_MAX_LEN};
use tandem::factory::store::FactoryStore;

struct Ws {
    base: PathBuf,
    root: PathBuf,
}

impl Ws {
    fn new(name: &str) -> Self {
        let base =
            std::env::temp_dir().join(format!("tandem-start-text-{name}-{}", uuid::Uuid::new_v4()));
        let root = base.join("ws");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(base.join("state")).unwrap();
        fs::create_dir_all(base.join("tmux")).unwrap();
        Self { base, root }
    }

    fn run(&self, args: &[&str], stdin: Option<&str>) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tandem"));
        cmd.args(args)
            .env("TANDEM_WORKSPACE", &self.root)
            .env("XDG_STATE_HOME", self.base.join("state"))
            .env("TMUX_TMPDIR", self.base.join("tmux"))
            .env("TANDEM_FACTORY", "1")
            .env("PATH", "/usr/bin:/bin")
            .env_remove("TANDEM_ROLE")
            .env_remove("TANDEM_SESSION_ID")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        let mut child = cmd.spawn().unwrap();
        if let Some(input) = stdin
            && let Some(mut pipe) = child.stdin.take()
        {
            pipe.write_all(input.as_bytes()).unwrap();
        }
        child.wait_with_output().unwrap()
    }

    fn start(&self, extra: &[&str], stdin: Option<&str>) -> Output {
        let mut args = vec![
            "factory", "idea", "start", "--cli", "claude", "--model", "opus", "--title", "Export",
            "--json",
        ];
        args.extend_from_slice(extra);
        self.run(&args, stdin)
    }

    fn ideas(&self) -> Vec<tandem::factory::model::Idea> {
        FactoryStore::new(&self.root)
            .list_ideas()
            .unwrap_or_default()
    }
}

impl Drop for Ws {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn parse(args: &[&str]) -> Result<tandem::factory::commands::FactoryIdeaStartArgs, clap::Error> {
    let mut argv = vec![
        "tandem", "factory", "idea", "start", "--cli", "claude", "--model", "opus",
    ];
    argv.extend_from_slice(args);
    Cli::try_parse_from(argv).map(|cli| match cli.command {
        Some(TandemCommand::Factory(f)) => match f.command {
            FactoryCommand::Idea(i) => match i.command {
                FactoryIdeaCommand::Start(s) => s,
                other => panic!("expected Start, got {other:?}"),
            },
            other => panic!("expected Idea, got {other:?}"),
        },
        other => panic!("expected Factory, got {other:?}"),
    })
}

#[test]
fn start_parses_text_file_and_text_stdin_and_defaults_to_no_text() {
    let none = parse(&[]).unwrap();
    assert_eq!(none.text_file, None);
    assert!(!none.text_stdin);

    let file = parse(&["--text-file", "body.md"]).unwrap();
    assert_eq!(file.text_file.as_deref(), Some("body.md"));

    let stdin = parse(&["--text-stdin"]).unwrap();
    assert!(stdin.text_stdin);
}

#[test]
fn start_rejects_text_file_together_with_text_stdin() {
    assert!(parse(&["--text-file", "body.md", "--text-stdin"]).is_err());
}

#[test]
fn the_description_is_stored_with_the_idea_from_a_file() {
    let ws = Ws::new("file");
    let body = ws.base.join("body.md");
    fs::write(&body, "Export weekly reports as CSV.\nInclude totals.").unwrap();

    let _ = ws.start(&["--text-file", body.to_str().unwrap()], None);
    let ideas = ws.ideas();
    assert_eq!(ideas.len(), 1);
    assert_eq!(ideas[0].title, "Export");
    assert_eq!(
        ideas[0].text,
        "Export weekly reports as CSV.\nInclude totals."
    );
}

#[test]
fn the_description_can_be_read_from_stdin() {
    let ws = Ws::new("stdin");
    let _ = ws.start(&["--text-stdin"], Some("From the dialog"));
    let ideas = ws.ideas();
    assert_eq!(ideas.len(), 1);
    assert_eq!(ideas[0].text, "From the dialog");
}

#[test]
fn without_text_the_idea_still_starts_blank() {
    let ws = Ws::new("blank");
    let _ = ws.start(&[], None);
    let ideas = ws.ideas();
    assert_eq!(ideas.len(), 1);
    assert_eq!(ideas[0].text, "");
}

#[test]
fn oversized_text_is_rejected_before_any_idea_is_created() {
    let ws = Ws::new("oversized");
    let body = ws.base.join("big.md");
    fs::write(&body, "x".repeat(TEXT_MAX_LEN + 1)).unwrap();

    let out = ws.start(&["--text-file", body.to_str().unwrap()], None);
    assert_eq!(out.status.code(), Some(1));
    assert!(ws.ideas().is_empty(), "no idea for rejected text");

    let huge = ws.start(&["--text-stdin"], Some(&"y".repeat(TEXT_MAX_LEN * 8)));
    assert_eq!(huge.status.code(), Some(1));
    assert!(ws.ideas().is_empty());
}

#[test]
fn control_characters_and_symlinked_files_are_rejected() {
    let ws = Ws::new("unsafe");
    let out = ws.start(&["--text-stdin"], Some("bad \u{0007} text"));
    assert_eq!(out.status.code(), Some(1));
    assert!(ws.ideas().is_empty());

    #[cfg(unix)]
    {
        let real = ws.base.join("real.md");
        fs::write(&real, "secret").unwrap();
        let link = ws.base.join("link.md");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let out = ws.start(&["--text-file", link.to_str().unwrap()], None);
        assert_eq!(out.status.code(), Some(1));
        assert!(ws.ideas().is_empty());
    }
}

#[test]
fn secrets_in_the_description_are_redacted_in_the_event_log_like_describe() {
    let ws = Ws::new("redact");
    let secret = format!("sk-{}", "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6");
    let _ = ws.start(&["--text-stdin"], Some(&format!("key is {secret} ok")));
    let log = fs::read_to_string(ws.root.join(".agent-workspace/factory/events.jsonl")).unwrap();
    assert!(
        !log.contains(&secret),
        "secret must not reach the event log"
    );
    assert!(log.contains("redacted:token"), "log: {log}");
}

#[test]
fn an_orchestrator_that_fails_to_launch_cancels_the_idea_but_keeps_its_text() {
    let ws = Ws::new("cancel");
    let out = ws.start(&["--text-stdin"], Some("Keep me"));
    assert_ne!(out.status.code(), Some(0), "no tmux session in tests");
    let ideas = ws.ideas();
    assert_eq!(ideas.len(), 1);
    assert_eq!(ideas[0].state, IdeaState::Cancelled);
    assert_eq!(ideas[0].text, "Keep me");
}
