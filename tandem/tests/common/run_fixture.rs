#![allow(dead_code)]

use std::fs;
use std::path::Path;

use tandem::factory::approval::mark_ready_with_workflow;
use tandem::factory::artifacts;
use tandem::factory::machine::Command;
use tandem::factory::model::{Actor, ActorKind, FeasibilityVerdict, Risk};
use tandem::factory::store::FactoryStore;

pub fn user_actor() -> Actor {
    Actor {
        kind: ActorKind::User,
        id: "argie".to_string(),
    }
}

pub fn orch_actor() -> Actor {
    Actor {
        kind: ActorKind::Orchestrator,
        id: "orchestrator".to_string(),
    }
}

pub fn sys_actor() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "runner".to_string(),
    }
}

pub fn full_plan_md() -> &'static str {
    "# Plan\n\n## Scope\nExport team activity as CSV.\n\n## Acceptance criteria\n- Exports CSV cleanly.\n\n## Risks\nLow risk.\n\n## Estimates\n1 day.\n\n## Workflow recommendation\ndefault-delivery\n"
}

pub fn setup_run(ws: &Path) -> (FactoryStore, String, String) {
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();
    let mut settings = store.settings().unwrap();
    settings.approval_grace_secs = 0;
    tandem::factory::settings::save(&store, &settings).unwrap();
    let user = user_actor();
    let orch = orch_actor();

    let idea = store
        .create_idea("Export team activity as CSV", "CSV export", &user.id, &user)
        .unwrap();
    store
        .execute(
            &idea.id,
            Command::BindOrchestrator {
                cli: "claude".to_string(),
                model: "opus".to_string(),
                conversation: Some("conv-1".to_string()),
            },
            &user,
        )
        .unwrap();
    store
        .execute(
            &idea.id,
            Command::Describe {
                title: "Export team activity as CSV".to_string(),
                text: "Allow exporting team activity.".to_string(),
            },
            &orch,
        )
        .unwrap();

    let feas_src = ws.join("feas.md");
    fs::write(&feas_src, "# Feasibility\n\nFeasible.\n").unwrap();
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
            &orch,
        )
        .unwrap();
    store
        .execute(&idea.id, Command::SelectDirection, &user)
        .unwrap();

    let plan_src = ws.join("plan.md");
    fs::write(&plan_src, full_plan_md()).unwrap();
    let plan_ref = artifacts::put(&store, &idea.id, "plan", &plan_src).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".to_string(),
                artifact: plan_ref,
                verdict: None,
                risk: None,
                workflow: Some("default-delivery".to_string()),
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();

    let env_fn = |k: &str| {
        if k == "TANDEM_ROLE" {
            None
        } else {
            std::env::var(k).ok()
        }
    };
    let caller = tandem::factory::approval::Caller::tui(&env_fn, &|_| None);
    let ready = mark_ready_with_workflow(
        &store,
        ws,
        &idea.id,
        &idea.id,
        &user.id,
        Some("default-delivery"),
        false,
        &caller,
    )
    .unwrap();
    assert!(!ready.message.contains("Execution arrives"));
    let run_id = ready.run.expect("run created").id;
    (store, idea.id, run_id)
}
