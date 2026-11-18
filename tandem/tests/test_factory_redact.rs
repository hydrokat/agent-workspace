use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use tandem::config::Config;
use tandem::factory::approval::mark_ready;
use tandem::factory::artifacts;
use tandem::factory::broker::checks::CheckOutcome;
use tandem::factory::checks::CheckKind;
use tandem::factory::commands::run_with_io;
use tandem::factory::events::{Event, append, read_all, redact_event};
use tandem::factory::evidence::{
    self, AddEvidenceInput, EvidenceContent, record_committed_diff_stat,
    record_create_tests_exception, record_demo_skipped_no_tool, record_review_findings,
    record_test_stage_evidence,
};
use tandem::factory::handoff::write_and_record_handoff;
use tandem::factory::machine::{Command, EventData};
use tandem::factory::model::{Actor, ActorKind, FeasibilityVerdict, Risk};
use tandem::factory::pr::{PrOutcome, open_pull_request_with_gh};
use tandem::factory::redact::{RedactContext, Redacted, SecretKind, inspect, redact};
use tandem::factory::run::{CreateRunOptions, create_run};
use tandem::factory::stages::review::{FindingsReport, ReviewVerdict};
use tandem::factory::stages::test::TestStageReport;
use tandem::factory::store::FactoryStore;
use tandem::factory::workspace::prepare_worktree;

fn call_factory(
    args: &[&str],
    ws: &Path,
    cfg: &Config,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
) -> i32 {
    let mut full = vec!["tandem"];
    full.extend_from_slice(args);
    let cli = tandem::cli::parse_from(full).unwrap();
    let tandem::cli::Command::Factory(f_args) = cli.command.unwrap() else {
        panic!("expected factory command");
    };
    let mut stdin = std::io::empty();
    run_with_io(&f_args, ws, cfg, &mut stdin, stdout, stderr)
}

fn temp_ws(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-redact-test-{label}-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn sys_actor() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "test".to_string(),
    }
}

fn user_actor() -> Actor {
    Actor {
        kind: ActorKind::User,
        id: "tester".to_string(),
    }
}

#[test]
fn redacts_fixed_token_formats() {
    let ctx = RedactContext::new();
    let cases = [
        ("openai", "key=sk-proj-abc1234567890xyz"),
        ("ghp", "token ghp_1234567890abcdefghij"),
        ("gho", "token gho_1234567890abcdefghij"),
        ("github_pat", "pat=github_pat_11ABCDEF0123456789_xyz"),
        ("slack", "bot=xoxb-1234567890-1234567890-abcdef"),
        ("aws", "aws_id=AKIAIOSFODNN7EXAMPLE"),
        (
            "bearer",
            "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload.sig",
        ),
    ];

    for (label, input) in cases {
        let res: Redacted = redact(input, &ctx);
        assert!(
            res.replacements() >= 1,
            "{label}: expected >= 1 replacement, got 0 in {:?}",
            res.as_str()
        );
        assert!(
            res.as_str().contains("«redacted:token»"),
            "{label}: expected «redacted:token» in {:?}",
            res.as_str()
        );
    }

    assert!(
        !redact("sk-proj-abc1234567890xyz", &ctx)
            .as_str()
            .contains("sk-proj-")
    );
    assert!(
        !redact("ghp_1234567890abcdefghij", &ctx)
            .as_str()
            .contains("ghp_")
    );
    assert!(
        !redact("AKIAIOSFODNN7EXAMPLE", &ctx)
            .as_str()
            .contains("AKIA")
    );
    assert!(
        !redact("Bearer supersecrettoken123", &ctx)
            .as_str()
            .contains("supersecrettoken123")
    );
}

#[test]
fn redacts_private_key_blocks() {
    let ctx = RedactContext::new();
    let input = "\
before
-----BEGIN RSA PRIVATE KEY-----
MIIEowIBAAKCAQEA1234567890abcdef
-----END RSA PRIVATE KEY-----
after";
    let res = redact(input, &ctx);
    assert_eq!(res.replacements(), 1);
    assert!(res.as_str().contains("«redacted:private_key»"));
    assert!(!res.as_str().contains("BEGIN RSA PRIVATE KEY"));
    assert!(!res.as_str().contains("MIIEowIBAAKCAQEA1234567890abcdef"));
    assert!(res.as_str().contains("before"));
    assert!(res.as_str().contains("after"));
}

#[test]
fn redacts_url_credentials() {
    let ctx = RedactContext::new();
    let input = "remote https://alice:s3cr3tP4ss@github.com/acme/repo.git failed";
    let res = redact(input, &ctx);
    assert_eq!(res.replacements(), 1);
    assert!(res.as_str().contains("«redacted:url_credential»"));
    assert!(!res.as_str().contains("s3cr3tP4ss"));
    assert!(res.as_str().contains("github.com/acme/repo.git"));
}

#[test]
fn redacts_key_value_pairs_in_assignment_json_and_yaml() {
    let ctx = RedactContext::new();
    let input = "\
API_KEY=my_plain_secret_val
my_password = \"quoted_pass_val\"
{\"client_secret\": \"json_secret_val\", \"apiKey\": \"camel_key_val\"}
db_credential: yaml_cred_val
auth_passwd: 'yaml_quoted_passwd'
";
    let res = redact(input, &ctx);
    assert!(res.replacements() >= 5, "got {}", res.replacements());
    for leaked in [
        "my_plain_secret_val",
        "quoted_pass_val",
        "json_secret_val",
        "camel_key_val",
        "yaml_cred_val",
        "yaml_quoted_passwd",
    ] {
        assert!(
            !res.as_str().contains(leaked),
            "leaked {leaked} in:\n{}",
            res.as_str()
        );
    }
    assert!(res.as_str().contains("«redacted:key_value»"));
}

#[test]
fn redacts_env_secrets_at_least_eight_chars() {
    let ctx = RedactContext::from_pairs([
        ("MY_SERVICE_TOKEN", "unusual-env-secret-value-99"),
        ("SHORT_SECRET", "tiny123"),
        ("HOME", "/Users/tester"),
    ]);
    let input = "leaked unusual-env-secret-value-99 and tiny123 and /Users/tester";
    let res = redact(input, &ctx);
    assert_eq!(res.replacements(), 1);
    assert!(res.as_str().contains("«redacted:env_secret»"));
    assert!(!res.as_str().contains("unusual-env-secret-value-99"));
    assert!(res.as_str().contains("tiny123"));
}

#[test]
fn redacts_high_entropy_strings_but_preserves_hash_fields() {
    let ctx = RedactContext::new();
    let secret_b64 = "aB3dE5fG7hI9jK1lM3nO5pQ7rS9tU1vW3xY5zA7bC9dE";
    let commit_sha = "4a8f1c9e2b7d3056a1b2c3d4e5f60718293a4b5c";
    let plan_sha = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    let wf_sha = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    let input = format!(
        "output: {secret_b64}\n\
         commit {commit_sha}\n\
         \"sha256\": \"{plan_sha}\"\n\
         plan_sha256: {plan_sha}\n\
         workflow_sha256: {wf_sha}\n"
    );
    let res = redact(&input, &ctx);
    assert!(
        !res.as_str().contains(secret_b64),
        "high-entropy secret leaked:\n{}",
        res.as_str()
    );
    assert!(res.as_str().contains("«redacted:high_entropy»"));
    assert!(res.as_str().contains(commit_sha), "commit sha was redacted");
    assert!(res.as_str().contains(plan_sha), "plan sha256 was redacted");
    assert!(
        res.as_str().contains(wf_sha),
        "workflow sha256 was redacted"
    );
}

#[test]
fn inspect_reports_kinds_line_numbers_and_counts() {
    let ctx = RedactContext::from_pairs([("APP_SECRET", "env_secret_value_123")]);
    let input = "clean line\nline two sk-1234567890abcdef\nline three env_secret_value_123\n";
    let (redacted, matches) = inspect(input, &ctx);
    assert_eq!(redacted.replacements(), 2);
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].kind, SecretKind::Token);
    assert_eq!(matches[0].line, 2);
    assert_eq!(matches[1].kind, SecretKind::EnvSecret);
    assert_eq!(matches[1].line, 3);
}

#[test]
fn cli_redact_test_reports_kinds_lines_and_count() {
    let ws = temp_ws("cli-redact-test");
    let sample = ws.join("sample.txt");
    fs::write(
        &sample,
        "ok\nkey=ghp_1234567890abcdefghij\nhttps://bob:pw123456@example.com/x\n",
    )
    .unwrap();

    let cfg = Config {
        factory: true,
        state_dir: ws.join("state"),
        ..Default::default()
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let sample_str = sample.to_string_lossy().into_owned();
    let code = call_factory(
        &["factory", "redact", "test", &sample_str],
        &ws,
        &cfg,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));
    let out = String::from_utf8_lossy(&stdout);
    assert!(out.contains("line 2"), "missing line 2 in:\n{out}");
    assert!(out.contains("line 3"), "missing line 3 in:\n{out}");
    assert!(out.contains("2"), "missing count in:\n{out}");

    let _ = fs::remove_dir_all(&ws);
}

fn init_git_repo(dir: &Path) {
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Tester"],
    ] {
        assert!(
            StdCommand::new("git")
                .args(&args)
                .current_dir(dir)
                .status()
                .unwrap()
                .success()
        );
    }
    fs::write(dir.join("README.md"), "init\n").unwrap();
    assert!(
        StdCommand::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
}

fn init_bare_remote(bare: &Path) {
    assert!(
        StdCommand::new("git")
            .args(["init", "--bare", "-b", "main"])
            .arg(bare)
            .status()
            .unwrap()
            .success()
    );
}

fn write_stub_gh(bin_dir: &Path, body_out: &Path, title_out: &Path) -> PathBuf {
    fs::create_dir_all(bin_dir).unwrap();
    let gh = bin_dir.join("gh");
    let script = format!(
        r#"#!/bin/sh
case "$1 $2" in
  "auth status")
    exit 0
    ;;
  "repo view")
    echo '{{"nameWithOwner":"acme/widget","defaultBranchRef":{{"name":"main"}}}}'
    exit 0
    ;;
  "pr list")
    echo '[]'
    exit 0
    ;;
  "pr create")
    while [ $# -gt 0 ]; do
      case "$1" in
        --body-file)
          cp "$2" "{body}"
          shift 2
          ;;
        --title)
          printf '%s' "$2" > "{title}"
          shift 2
          ;;
        *)
          shift
          ;;
      esac
    done
    echo "https://github.com/acme/widget/pull/77"
    exit 0
    ;;
  "pr view")
    echo '{{"number":77,"url":"https://github.com/acme/widget/pull/77"}}'
    exit 0
    ;;
  *)
    exit 1
    ;;
esac
"#,
        body = body_out.display(),
        title = title_out.display(),
    );
    fs::write(&gh, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
    }
    gh
}

#[test]
fn planted_secrets_never_reach_events_evidence_handoff_or_pr_body() {
    let ws = temp_ws("e2e-planted");
    let bare = temp_ws("e2e-bare");
    init_git_repo(&ws);
    init_bare_remote(&bare);
    assert!(
        StdCommand::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/acme/widget.git"
            ])
            .current_dir(&ws)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args([
                "remote",
                "set-url",
                "--push",
                "origin",
                &bare.display().to_string()
            ])
            .current_dir(&ws)
            .status()
            .unwrap()
            .success()
    );

    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();
    let orch = Actor {
        kind: ActorKind::Orchestrator,
        id: "orch".to_string(),
    };

    let idea = store
        .create_idea(
            "Add redaction",
            "Protect outbound boundaries",
            "tester",
            &user_actor(),
        )
        .unwrap();
    store
        .execute(&idea.id, Command::StartDiscovery, &orch)
        .unwrap();
    let feas = ws.join("feas.md");
    fs::write(&feas, "# Feasibility\nfeasible\n").unwrap();
    let feas_ref = artifacts::put(&store, &idea.id, "feasibility", &feas).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: feas_ref,
                verdict: Some(FeasibilityVerdict::Feasible),
                risk: Some(Risk::Low),
                workflow: None,
                missing_sections: vec![],
            },
            &orch,
        )
        .unwrap();
    let plan = ws.join("plan.md");
    fs::write(
        &plan,
        "## Scope\ns\n## Acceptance criteria\na\n## Risks\nr\n## Estimates\ne\n## Workflow recommendation\ndefault-delivery\n",
    )
    .unwrap();
    let plan_ref = artifacts::put(&store, &idea.id, "plan", &plan).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".to_string(),
                artifact: plan_ref,
                verdict: None,
                risk: None,
                workflow: Some("default-delivery".to_string()),
                missing_sections: vec![],
            },
            &orch,
        )
        .unwrap();

    store
        .execute(&idea.id, Command::SelectDirection, &user_actor())
        .unwrap();
    mark_ready(&store, &idea.id, &idea.id, "cli", "tester", &|_| None).unwrap();

    let run = create_run(
        &store,
        &ws,
        &idea.id,
        CreateRunOptions::default(),
        &user_actor(),
    )
    .unwrap();
    let prep = prepare_worktree(&store, &ws, &run.id).unwrap();

    fs::write(prep.worktree_path.join("change.txt"), "safe change\n").unwrap();
    for args in [vec!["add", "."], vec!["commit", "-m", "feat: add change"]] {
        assert!(
            StdCommand::new("git")
                .args(&args)
                .current_dir(&prep.worktree_path)
                .status()
                .unwrap()
                .success()
        );
    }

    let planted_token = "ghp_PlantedSecretToken1234567890";
    let planted_key = "-----BEGIN RSA PRIVATE KEY-----\nMIIEPlantedKeyBody1234567890\n-----END RSA PRIVATE KEY-----";
    let planted_url = "https://deploy:PlantedUrlPass99@internal.example.com/repo.git";
    let planted_kv = "service_password=PlantedKvSecret88";
    let planted_env = "PlantedEnvSecretValue77";
    let planted_entropy = "zX9cV8bN7mQ6wE5rT4yU3iO2pA1sD0fG9hJ8kL7";

    let ctx = RedactContext::from_pairs([("PLANTED_API_TOKEN", planted_env)]);

    let combined_output = format!(
        "tok={planted_token}\n{planted_key}\nurl={planted_url}\n{planted_kv}\nenv={planted_env}\nent={planted_entropy}"
    );

    let ev = Event::from_data(
        1.0,
        sys_actor(),
        idea.id.clone(),
        Some(run.id.clone()),
        &EventData::StageFailed {
            run: run.id.clone(),
            stage: "develop".to_string(),
            reason: combined_output.clone(),
        },
    )
    .unwrap();
    let redacted_ev = redact_event(ev, &ctx);
    let events_path = store.root.join("events.jsonl");
    let lock_path = store.root.join("factory.lock");
    append(&events_path, &lock_path, &[redacted_ev]).unwrap();

    let redacted_summary = redact(&combined_output, &ctx);
    let redacted_body = redact(&combined_output, &ctx);
    evidence::add(
        &store,
        AddEvidenceInput {
            idea_id: &idea.id,
            run_id: &run.id,
            kind: tandem::factory::workflow::schema::EvidenceKind::DiffStat,
            stage: "develop",
            attempt: 1,
            extension: "txt",
            content: EvidenceContent::Text(redacted_body),
            summary: redacted_summary,
        },
        &sys_actor(),
    )
    .unwrap();

    let run_dir = store.run_dir(&run.id);
    let approve = FindingsReport {
        verdict: ReviewVerdict::Approve,
        findings: vec![],
    };
    let approve_bytes = serde_json::to_vec_pretty(&approve).unwrap();
    fs::write(run_dir.join("findings.json"), &approve_bytes).unwrap();
    record_review_findings(
        &store,
        &idea.id,
        &run.id,
        1,
        &approve_bytes,
        "approve: 0 findings".to_string(),
        &sys_actor(),
    )
    .unwrap();
    record_create_tests_exception(&store, &idea.id, &run.id, 1, "test exception", &sys_actor())
        .unwrap();
    record_demo_skipped_no_tool(&store, &idea.id, &run.id, "headless test", &sys_actor()).unwrap();

    let log_file = run_dir.join("checks").join("test-1.log");
    fs::create_dir_all(log_file.parent().unwrap()).unwrap();
    fs::write(&log_file, "test failed\n").unwrap();
    let test_report = TestStageReport {
        outcomes: vec![CheckOutcome {
            kind: CheckKind::Test,
            argv: vec!["cargo".into(), "test".into()],
            exit_code: Some(1),
            duration_ms: 50,
            timed_out: false,
            log_cap_exceeded: false,
            log_path: log_file,
            summary: Some(format!("assertion failed: {planted_token}")),
            error: None,
        }],
        all_passed: false,
        failures_summary: format!("test foo failed with {planted_token}"),
    };
    record_test_stage_evidence(&store, &idea.id, &run.id, 1, &test_report, &sys_actor()).unwrap();
    record_committed_diff_stat(
        &store,
        &idea.id,
        &run.id,
        "develop",
        1,
        &prep.worktree_path,
        &sys_actor(),
    )
    .unwrap();

    let (handoff_path, _) = write_and_record_handoff(&store, &ws, &run.id, &sys_actor()).unwrap();

    let captured_body = ws.join("gh-body.md");
    let captured_title = ws.join("gh-title.txt");
    let gh = write_stub_gh(&ws.join("bin"), &captured_body, &captured_title);
    let outcome = open_pull_request_with_gh(&store, &ws, &run.id, &sys_actor(), &gh).unwrap();
    assert!(matches!(outcome, PrOutcome::Created(_)));

    let events_raw = fs::read_to_string(&events_path).unwrap();
    let handoff_raw = fs::read_to_string(&handoff_path).unwrap();
    let pr_body_raw = fs::read_to_string(&captured_body).unwrap();
    let evidence_items = evidence::list(&store, &run.id).unwrap();
    let mut evidence_raw = String::new();
    for item in &evidence_items {
        evidence_raw.push_str(&item.summary);
        if let Ok(text) = evidence::read_item(&store, &run.id, &item.id) {
            evidence_raw.push_str(&text);
        }
    }

    for secret in [
        "PlantedSecretToken1234567890",
        "MIIEPlantedKeyBody1234567890",
        "PlantedUrlPass99",
        "PlantedKvSecret88",
        "PlantedEnvSecretValue77",
        "zX9cV8bN7mQ6wE5rT4yU3iO2pA1sD0fG9hJ8kL7",
    ] {
        assert!(
            !events_raw.contains(secret),
            "secret {secret} leaked in events.jsonl"
        );
        assert!(
            !evidence_raw.contains(secret),
            "secret {secret} leaked in evidence"
        );
        assert!(
            !handoff_raw.contains(secret),
            "secret {secret} leaked in handoff.md"
        );
        assert!(
            !pr_body_raw.contains(secret),
            "secret {secret} leaked in pr body"
        );
    }

    // Verify failing-test excerpt is included (redacted) in PR body
    assert!(
        pr_body_raw.contains("assertion failed: «redacted:token»"),
        "expected redacted failing-test excerpt in PR body:\n{pr_body_raw}"
    );
    assert!(read_all(&events_path).is_ok());

    let _ = fs::remove_dir_all(&ws);
    let _ = fs::remove_dir_all(&bare);
}
