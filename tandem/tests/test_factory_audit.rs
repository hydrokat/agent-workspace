use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use sha2::{Digest, Sha256};
use tandem::config::Config;
use tandem::factory::artifacts;
use tandem::factory::audit::{
    self, AuditFilter, AuditKind, AuditVerifyReport, export_json, export_markdown, verify,
};
use tandem::factory::broker::context::{ContextItem, ContextManifest};
use tandem::factory::broker::job::{JobSpec, prepare_job};
use tandem::factory::commands::run_with_io;
use tandem::factory::events::{self, Event, redact_event};
use tandem::factory::machine::{Command, EventData};
use tandem::factory::model::{Actor, ActorKind, Budget, FeasibilityVerdict, Risk};
use tandem::factory::permissions::ToolGrant;
use tandem::factory::protected::ProtectedAction;
use tandem::factory::redact::RedactContext;
use tandem::factory::run::{CreateRunOptions, create_run};
use tandem::factory::store::FactoryStore;
use tandem::factory::telemetry::{Cost, CostBasis};
use tandem::factory::workflow::schema::{WorkflowAgent, WorkflowTool};
use tandem::status::factory_audit::{AuditKeyOutcome, FactoryAudit};
use tandem::status::factory_detail::{DetailAction, DetailKeyOutcome, FactoryDetail};

fn temp_ws(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-factory-audit-test-{label}-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(dir.join(".agent-workspace/coordination/queries")).unwrap();
    fs::create_dir_all(dir.join("specs")).unwrap();
    dir
}

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

fn buffer_text(buf: &Buffer, area: Rect) -> String {
    let mut lines = Vec::new();
    for y in area.y..area.y + area.height {
        let mut line = String::new();
        for x in area.x..area.x + area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

fn user_actor() -> Actor {
    Actor {
        kind: ActorKind::User,
        id: "you".to_string(),
    }
}

fn sys_actor() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "system".to_string(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[test]
fn prev_hash_chain_handles_legacy_events_and_detects_tampering() {
    let ws = temp_ws("chain");
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();
    let events_path = ws.join(".agent-workspace/factory/events.jsonl");

    // Legacy events written before prev hash support.
    let legacy1 = serde_json::json!({
        "v": 1,
        "seq": 1,
        "ts": 1000.0,
        "actor": {"kind": "user", "id": "you"},
        "type": "idea.created",
        "idea": "i-11112222",
        "data": {"id": "i-11112222", "title": "Legacy", "text": "Legacy idea", "owner": "you"}
    });
    let legacy2 = serde_json::json!({
        "v": 1,
        "seq": 2,
        "ts": 1001.0,
        "actor": {"kind": "system", "id": "system"},
        "type": "idea.discovery_started",
        "idea": "i-11112222",
        "data": {}
    });
    let line1 = serde_json::to_string(&legacy1).unwrap();
    let line2 = serde_json::to_string(&legacy2).unwrap();
    fs::write(&events_path, format!("{line1}\n{line2}\n")).unwrap();

    // Older logs load and rebuild cleanly.
    let report = store.rebuild().unwrap();
    assert_eq!(report.ideas, 1);

    // Append new events; they carry prev = sha256(previous raw line).
    let ev3 = Event::from_data(
        1002.0,
        user_actor(),
        "i-11112222".to_string(),
        None,
        &EventData::IdeaAssumptionAdded {
            text: "first chained event".to_string(),
        },
    )
    .unwrap();
    let ev4 = Event::from_data(
        1003.0,
        user_actor(),
        "i-11112222".to_string(),
        None,
        &EventData::IdeaAssumptionAdded {
            text: "second chained event".to_string(),
        },
    )
    .unwrap();
    let ctx = RedactContext::new();
    let appended = events::append(
        &events_path,
        &ws.join(".agent-workspace/factory/.lock"),
        &[redact_event(ev3, &ctx), redact_event(ev4, &ctx)],
    )
    .unwrap();

    assert_eq!(appended.len(), 2);
    assert_eq!(appended[0].seq, 3);
    assert_eq!(
        appended[0].prev.as_deref(),
        Some(sha256_hex(line2.as_bytes()).as_str())
    );
    assert_eq!(appended[1].seq, 4);
    assert!(appended[1].prev.is_some());

    let verify_ok = verify(&events_path).unwrap();
    assert!(verify_ok.intact);
    assert_eq!(verify_ok.chain_start_seq, Some(3));
    assert_eq!(verify_ok.verified_events, 2);
    assert!(verify_ok.first_break.is_none());

    // Tamper with line 3 (after chain started) and verify break detection without blocking rebuild.
    let raw = fs::read_to_string(&events_path).unwrap();
    let tampered = raw.replace("first chained event", "tampered event text");
    fs::write(&events_path, tampered).unwrap();

    let verify_bad = verify(&events_path).unwrap();
    assert!(!verify_bad.intact);
    assert_eq!(verify_bad.chain_start_seq, Some(3));
    assert_eq!(verify_bad.first_break.as_ref().map(|b| b.seq), Some(4));

    // Normal work (rebuild/read) is never blocked by a chain break.
    assert!(store.rebuild().is_ok());

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn entries_collect_job_details_approvals_decisions_and_filters() {
    let ws = temp_ws("entries");
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();
    let user = user_actor();
    let sys = sys_actor();

    let idea = store
        .create_idea("Audit CSV", "Export CSV cleanly", "you", &user)
        .unwrap();
    store
        .execute(&idea.id, Command::StartDiscovery, &user)
        .unwrap();
    let feas_src = ws.join("feas.md");
    fs::write(
        &feas_src,
        "# Feasibility\n\n## Verdict\nfeasible\n\n## Risk\nlow\n",
    )
    .unwrap();
    let feas_ref = artifacts::put(&store, &idea.id, "feasibility", &feas_src).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: feas_ref,
                verdict: Some(FeasibilityVerdict::Feasible),
                risk: Some(Risk::Low),
                workflow: None,
                missing_sections: Vec::new(),
            },
            &user,
        )
        .unwrap();
    store
        .execute(&idea.id, Command::SelectDirection, &user)
        .unwrap();
    let plan_src = ws.join("plan.md");
    fs::write(
        &plan_src,
        "# Plan\n\n## Scope\nCSV\n\n## Acceptance criteria\n- Works\n\n## Risks\nLow\n\n## Estimates\n1d\n\n## Workflow recommendation\ndefault-delivery\n",
    )
    .unwrap();
    let plan_ref = artifacts::put(&store, &idea.id, "plan", &plan_src).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".to_string(),
                artifact: plan_ref.clone(),
                verdict: None,
                risk: None,
                workflow: Some("default-delivery".to_string()),
                missing_sections: Vec::new(),
            },
            &user,
        )
        .unwrap();
    let caller = tandem::factory::approval::Caller::tui(&|_| None, &|_| None);
    tandem::factory::approval::mark_ready(&store, &idea.id, &idea.id, "you", &caller).unwrap();

    let run = create_run(
        &store,
        &ws,
        &idea.id,
        CreateRunOptions {
            workflow: None,
            deploy_requested: false,
            budget: None,
        },
        &user,
    )
    .unwrap();

    // Prepare a job so input.json and prompt.md exist with context manifest + redactions.
    let mut ctx_manifest = ContextManifest {
        items: vec![
            ContextItem {
                kind: "plan".to_string(),
                path: "plan.md".to_string(),
                bytes: 1200,
                sha256: "abc".to_string(),
            },
            ContextItem {
                kind: "playbook".to_string(),
                path: "playbook.md".to_string(),
                bytes: 800,
                sha256: "def".to_string(),
            },
        ],
        total_bytes: 4096,
        tool_grant: Some(ToolGrant::from_tools(
            WorkflowAgent::Implementer,
            "develop",
            "agent-workspace",
            vec![WorkflowTool::RepoRead, WorkflowTool::RepoWrite],
        )),
        ..Default::default()
    };
    let assembled = tandem::factory::broker::context::AssembledContext {
        manifest: ctx_manifest.clone(),
        prompt: "Prompt with «redacted:token» and «redacted:key_value»".to_string(),
    };
    let _ = &mut ctx_manifest;
    let spec = JobSpec {
        run: run.id.clone(),
        stage: "develop".to_string(),
        attempt: 1,
        agent: "senior-backend-dev".to_string(),
        provider: "claude".to_string(),
        model: "sonnet".to_string(),
        tool_grants: vec!["repo_read".to_string(), "repo_write".to_string()],
        cwd: ws.display().to_string(),
        time_limit_secs: 300,
    };
    prepare_job(&store, &spec, &assembled).unwrap();

    let events_path = ws.join(".agent-workspace/factory/events.jsonl");
    let lock_path = ws.join(".agent-workspace/factory/.lock");
    let rctx = RedactContext::new();
    let extra = vec![
        Event::from_data(
            2000.0,
            sys.clone(),
            idea.id.clone(),
            Some(run.id.clone()),
            &EventData::StageStarted {
                run: run.id.clone(),
                stage: "develop".to_string(),
                attempt: 1,
                grant: Some(ToolGrant::from_tools(
                    WorkflowAgent::Implementer,
                    "develop",
                    "agent-workspace",
                    vec![WorkflowTool::RepoRead, WorkflowTool::RepoWrite],
                )),
            },
        )
        .unwrap(),
        Event::from_data(
            2050.0,
            sys.clone(),
            idea.id.clone(),
            Some(run.id.clone()),
            &EventData::JobUsage {
                run: run.id.clone(),
                stage: "develop".to_string(),
                attempt: 1,
                provider: "claude".to_string(),
                model: "sonnet".to_string(),
                tier: "standard".to_string(),
                duration_secs: 520.0,
                input_tokens: Some(12000),
                output_tokens: Some(3000),
                cost: Cost {
                    value: Some(0.21),
                    basis: CostBasis::Estimated,
                },
                cost_usd: Some(0.21),
            },
        )
        .unwrap(),
        Event::from_data(
            2051.0,
            sys.clone(),
            idea.id.clone(),
            Some(run.id.clone()),
            &EventData::StageFailed {
                run: run.id.clone(),
                stage: "develop".to_string(),
                reason: "secret sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789 leaked"
                    .to_string(),
            },
        )
        .unwrap(),
        Event::from_data(
            2052.0,
            sys.clone(),
            idea.id.clone(),
            Some(run.id.clone()),
            &EventData::StageRetryRequested {
                run: run.id.clone(),
                stage: "develop".to_string(),
                hypothesis: "handle empty teams".to_string(),
            },
        )
        .unwrap(),
        Event::from_data(
            2053.0,
            sys.clone(),
            idea.id.clone(),
            Some(run.id.clone()),
            &EventData::StageModelUpgraded {
                run: run.id.clone(),
                stage: "develop".to_string(),
                from: "standard".to_string(),
                to: "premium".to_string(),
            },
        )
        .unwrap(),
        Event::from_data(
            2054.0,
            user.clone(),
            idea.id.clone(),
            Some(run.id.clone()),
            &EventData::ApprovalGranted {
                gate: 2,
                action: ProtectedAction::ProductionDeploy,
                run: run.id.clone(),
                stage: "handoff".to_string(),
                workflow_sha256: "wfsha".to_string(),
                actor: user.clone(),
                surface: "web".to_string(),
                expires_at: 9999.0,
            },
        )
        .unwrap(),
        Event::from_data(
            2055.0,
            user.clone(),
            idea.id.clone(),
            Some(run.id.clone()),
            &EventData::StageReopened {
                run: run.id.clone(),
                stage: "review".to_string(),
                reason: "fix edge case".to_string(),
            },
        )
        .unwrap(),
        Event::from_data(
            2056.0,
            user.clone(),
            idea.id.clone(),
            Some(run.id.clone()),
            &EventData::BudgetChanged {
                run: run.id.clone(),
                old: Budget::default(),
                new: Budget {
                    max_cost_usd: Some(25.0),
                    ..Budget::default()
                },
            },
        )
        .unwrap(),
        Event::from_data(
            2057.0,
            user.clone(),
            "settings".to_string(),
            None,
            &EventData::SettingsChanged {
                key: "max_concurrent_runs".to_string(),
                old_value: serde_json::json!(1),
                new_value: serde_json::json!(2),
                actor: "you".to_string(),
            },
        )
        .unwrap(),
        Event::from_data(
            2058.0,
            user.clone(),
            "provider".to_string(),
            None,
            &EventData::ProviderConsented {
                provider: "claude".to_string(),
                auth_type: "api_key".to_string(),
                verified_on: "2026-11-01".to_string(),
                terms_record: "terms-v1".to_string(),
                consented_by: "you".to_string(),
            },
        )
        .unwrap(),
    ];
    let redacted_extra: Vec<_> = extra.into_iter().map(|e| redact_event(e, &rctx)).collect();
    events::append(&events_path, &lock_path, &redacted_extra).unwrap();

    let all = audit::entries(&store, &idea.id, &AuditFilter::default()).unwrap();
    assert!(!all.is_empty());

    let job_start = all
        .iter()
        .find(|e| e.kind == AuditKind::JobStart)
        .expect("job start entry");
    assert!(job_start.summary.contains("claude"));
    assert!(job_start.summary.contains("sonnet"));
    let details_joined = job_start
        .details
        .iter()
        .map(|d| format!("{}: {}", d.label, d.value))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(details_joined.contains("default-delivery"));
    assert!(details_joined.contains("2 files"));
    assert!(details_joined.contains("2 redactions"));

    let job_end = all
        .iter()
        .find(|e| e.kind == AuditKind::JobEnd)
        .expect("job end entry");
    let end_details = job_end
        .details
        .iter()
        .map(|d| format!("{}: {}", d.label, d.value))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(end_details.contains("estimated"));
    assert!(end_details.contains("8m 40s"));
    assert!(!job_end.summary.contains("sk-ant-api03"));
    assert!(!end_details.contains("sk-ant-api03"));

    // Approvals
    let approvals: Vec<_> = all
        .iter()
        .filter(|e| e.kind == AuditKind::Approval)
        .collect();
    assert_eq!(approvals.len(), 2);

    // Decisions
    let decisions: Vec<_> = all
        .iter()
        .filter(|e| e.kind == AuditKind::Decision)
        .collect();
    assert!(decisions.iter().any(|d| d.summary.contains("retry")));
    assert!(decisions.iter().any(|d| d.summary.contains("premium")));
    assert!(decisions.iter().any(|d| d.summary.contains("reopened")));
    assert!(decisions.iter().any(|d| d.summary.contains("budget")));
    assert!(decisions.iter().any(|d| d.summary.contains("settings")));
    assert!(decisions.iter().any(|d| d.summary.contains("provider")));

    // Filters
    let by_kind = audit::entries(
        &store,
        &idea.id,
        &AuditFilter {
            kind: Some(AuditKind::Approval),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_kind.len(), 2);

    let by_run = audit::entries(
        &store,
        &idea.id,
        &AuditFilter {
            run: Some(run.id.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        by_run
            .iter()
            .all(|e| e.run.as_deref() == Some(run.id.as_str()))
    );

    let by_actor = audit::entries(
        &store,
        &idea.id,
        &AuditFilter {
            actor: Some("system".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(by_actor.iter().all(|e| e.actor == "system"));

    // Redacted JSON & Markdown exports
    let md = export_markdown(&idea.id, &all, Some(&verify(&events_path).unwrap()));
    assert!(md.contains(&idea.id));
    assert!(!md.contains("sk-ant-api03"));
    let js = export_json(&all, Some(&verify(&events_path).unwrap())).unwrap();
    assert!(!js.contains("sk-ant-api03"));

    // CLI: tandem factory audit IDEA --json / --markdown / verify --json
    let cfg = Config {
        factory: true,
        state_dir: ws.join("state"),
        ..Default::default()
    };
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = call_factory(
        &["factory", "audit", &idea.id, "--json"],
        &ws,
        &cfg,
        &mut out,
        &mut err,
    );
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    let parsed: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(parsed["entries"].as_array().unwrap().len() >= 5);

    out.clear();
    err.clear();
    let code = call_factory(
        &["factory", "audit", "verify", "--json"],
        &ws,
        &cfg,
        &mut out,
        &mut err,
    );
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    let vrep: AuditVerifyReport = serde_json::from_slice(&out).unwrap();
    assert!(vrep.intact);

    // Status window: FactoryDetail 'u' opens Audit, FactoryAudit renders wide & narrow.
    let mut detail = FactoryDetail::load(&ws, &idea.id, &[], false);
    let outcome = detail.handle_key(&KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE));
    assert_eq!(
        outcome,
        DetailKeyOutcome::Action(DetailAction::OpenAudit {
            idea_id: idea.id.clone(),
        })
    );

    let mut audit_view = FactoryAudit::load(&ws, &idea.id);
    let mut wide_buf = Buffer::empty(Rect::new(0, 0, 120, 30));
    audit_view.render(Rect::new(0, 0, 120, 30), &mut wide_buf);
    let wide_text = buffer_text(&wide_buf, Rect::new(0, 0, 120, 30));
    assert!(wide_text.contains("Audit"));
    assert!(wide_text.contains(&idea.id));

    // Narrow mode: Enter opens detail, Esc closes detail, Esc returns to idea detail.
    let mut narrow_buf = Buffer::empty(Rect::new(0, 0, 60, 24));
    audit_view.render(Rect::new(0, 0, 60, 24), &mut narrow_buf);
    assert_eq!(
        audit_view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        AuditKeyOutcome::Consumed
    );
    assert!(audit_view.narrow_detail_open);
    assert_eq!(
        audit_view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        AuditKeyOutcome::Consumed
    );
    assert!(!audit_view.narrow_detail_open);
    assert_eq!(
        audit_view.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        AuditKeyOutcome::BackToDetail
    );

    // Filter cycle ('f'), verify ('v'), export ('x')
    assert_eq!(
        audit_view.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE)),
        AuditKeyOutcome::Consumed
    );
    assert_eq!(
        audit_view.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)),
        AuditKeyOutcome::Consumed
    );
    assert!(audit_view.verify_report.is_some());
    assert_eq!(
        audit_view.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        AuditKeyOutcome::Consumed
    );
    assert!(!audit_view.notice.is_empty());

    let _ = fs::remove_dir_all(&ws);
}
