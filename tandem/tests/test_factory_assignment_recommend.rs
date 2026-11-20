mod common;

use tandem::factory::model::FactorySettings;
use tandem::factory::model_catalog::{CatalogModel, CostBasis, ModelSource};
use tandem::factory::permissions::ToolGrant;
use tandem::factory::routing::{
    CliCatalog, Complexity, ModelTier, RoutingContext, escalate, route_interactive,
    route_interactive_selected,
};
use tandem::factory::stages::plan::{Slice, SlicesManifest};
use tandem::factory::workflow::DEFAULT_WORKFLOW_YAML;
use tandem::factory::workflow::parse::parse_workflow_str;
use tandem::factory::workflow::schema::{StageDef, Workflow};
use tandem::factory::workflow::validate::validate;

fn wf() -> Workflow {
    let raw = parse_workflow_str(DEFAULT_WORKFLOW_YAML).unwrap();
    validate(&raw, &FactorySettings::default()).unwrap()
}

fn stage<'a>(wf: &'a Workflow, name: &str) -> &'a StageDef {
    wf.stages.iter().find(|s| s.name == name).unwrap()
}

fn grant(wf: &Workflow, name: &str) -> ToolGrant {
    let s = stage(wf, name);
    ToolGrant::from_tools(s.agent, &s.name, "workspace", s.tools.clone())
}

fn cm(id: &str, tier: ModelTier, rank: f64) -> CatalogModel {
    CatalogModel {
        id: id.to_string(),
        label: id.to_string(),
        tier,
        cost_rank: rank,
        cost_basis: CostBasis::Heuristic,
        allowed: true,
        source: ModelSource::Live,
        unclassified: false,
    }
}

fn catalog() -> Vec<CliCatalog> {
    vec![
        CliCatalog {
            cli: "claude".into(),
            models: vec![
                cm("claude-haiku-4-5", ModelTier::Economy, 1.0),
                cm("claude-sonnet-4-6", ModelTier::Standard, 10.0),
                cm("claude-opus-4-7", ModelTier::Premium, 100.0),
            ],
        },
        CliCatalog {
            cli: "agy".into(),
            models: vec![
                cm("gemini-2.0-flash-lite", ModelTier::Economy, 0.4),
                cm("gemini-2.0-flash", ModelTier::Standard, 0.5),
                cm("gemini-2.5-pro", ModelTier::Premium, 20.0),
            ],
        },
    ]
}

fn ctx(catalog: Vec<CliCatalog>) -> RoutingContext {
    RoutingContext {
        catalog,
        ..RoutingContext::default()
    }
}

fn pick(
    name: &str,
    ctx: &RoutingContext,
    attempt: u32,
) -> (
    tandem::factory::routing::Route,
    Option<tandem::factory::routing::Selection>,
) {
    let wf = wf();
    route_interactive_selected(
        stage(&wf, name),
        &grant(&wf, name),
        &wf,
        &FactorySettings::default(),
        &["claude", "agy"],
        attempt,
        ctx,
    )
    .unwrap()
}

#[test]
fn complexity_maps_to_a_minimum_tier() {
    assert_eq!(Complexity::Low.min_tier(), ModelTier::Economy);
    assert_eq!(Complexity::Medium.min_tier(), ModelTier::Standard);
    assert_eq!(Complexity::High.min_tier(), ModelTier::Premium);
    assert_eq!(Complexity::parse("HIGH"), Some(Complexity::High));
    assert_eq!(Complexity::parse("nope"), None);
}

#[test]
fn picks_the_cheapest_capable_model_across_signed_in_clis() {
    let (route, selection) = pick("develop", &ctx(catalog()), 1);
    assert_eq!(
        (route.provider.as_str(), route.model.as_str()),
        ("agy", "gemini-2.0-flash")
    );
    let sel = selection.expect("catalog selection is recorded");
    assert_eq!(sel.required_tier, ModelTier::Standard);
    assert_eq!(sel.selected_tier, ModelTier::Standard);
    assert_eq!(sel.cost_basis, CostBasis::Heuristic);
    assert_eq!(
        sel.candidates, 4,
        "sonnet, opus, flash, and pro are capable"
    );
    assert!(sel.rationale.contains("cheapest"), "{}", sel.rationale);
}

#[test]
fn the_preferred_cli_wins_even_when_another_cli_is_cheaper() {
    let mut c = ctx(catalog());
    c.preferred_provider = Some("claude".into());
    let (route, _) = pick("develop", &c, 1);
    assert_eq!(
        (route.provider.as_str(), route.model.as_str()),
        ("claude", "claude-sonnet-4-6")
    );
}

#[test]
fn high_complexity_raises_the_required_tier() {
    let mut c = ctx(catalog());
    c.preferred_provider = Some("claude".into());
    c.complexity = Some(Complexity::High);
    let (route, sel) = pick("develop", &c, 1);
    assert_eq!(route.model, "claude-opus-4-7");
    assert_eq!(sel.unwrap().required_tier, ModelTier::Premium);
}

#[test]
fn a_required_tier_hint_is_honored_and_low_complexity_never_goes_below_the_stage_floor() {
    let mut c = ctx(catalog());
    c.preferred_provider = Some("claude".into());
    c.required_tier = Some(ModelTier::Premium);
    assert_eq!(pick("develop", &c, 1).0.model, "claude-opus-4-7");
    c.required_tier = None;
    c.complexity = Some(Complexity::Low);
    let (route, sel) = pick("develop", &c, 1);
    assert_eq!(
        route.model, "claude-sonnet-4-6",
        "develop keeps its Standard floor"
    );
    assert_eq!(sel.unwrap().required_tier, ModelTier::Standard);
}

#[test]
fn cheap_stages_use_economy_models() {
    let (route, sel) = pick("create-tests", &ctx(catalog()), 1);
    assert_eq!(route.model, "gemini-2.0-flash-lite");
    assert_eq!(sel.unwrap().selected_tier, ModelTier::Economy);
}

#[test]
fn review_floor_is_standard_and_avoids_the_develop_model() {
    let mut c = ctx(catalog());
    c.preferred_provider = Some("claude".into());
    c.avoid_model = Some("claude-sonnet-4-6".into());
    let (route, sel) = pick("review", &c, 1);
    assert_eq!(route.model, "claude-opus-4-7");
    assert_eq!(sel.unwrap().required_tier, ModelTier::Standard);
}

#[test]
fn review_independence_is_soft_when_no_other_model_is_capable() {
    let only = vec![CliCatalog {
        cli: "claude".into(),
        models: vec![cm("claude-sonnet-4-6", ModelTier::Standard, 10.0)],
    }];
    let mut c = ctx(only);
    c.avoid_model = Some("claude-sonnet-4-6".into());
    let (route, _) = pick("review", &c, 1);
    assert_eq!(route.model, "claude-sonnet-4-6");
}

#[test]
fn disallowed_models_are_never_picked() {
    let mut cat = catalog();
    cat[1].models[1].allowed = false;
    let (route, _) = pick("develop", &ctx(cat), 1);
    assert_eq!(route.model, "claude-sonnet-4-6");
}

#[test]
fn a_retry_raises_the_required_tier_one_step() {
    let mut c = ctx(catalog());
    c.preferred_provider = Some("claude".into());
    let (route, sel) = pick("develop", &c, 2);
    assert_eq!(route.model, "claude-opus-4-7");
    assert_eq!(sel.unwrap().required_tier, ModelTier::Premium);
}

#[test]
fn without_a_catalog_the_static_tiers_apply_and_no_selection_is_recorded() {
    let wf = wf();
    let (route, sel) = pick("develop", &RoutingContext::default(), 1);
    assert!(sel.is_none());
    let legacy = route_interactive(
        stage(&wf, "develop"),
        &grant(&wf, "develop"),
        &wf,
        &FactorySettings::default(),
        &["claude", "agy"],
        1,
        &RoutingContext::default(),
    )
    .unwrap();
    assert_eq!(route, legacy);
}

#[test]
fn a_cli_missing_from_the_catalog_falls_back_to_static_tiers() {
    let only_agy = vec![catalog().remove(1)];
    let mut c = ctx(only_agy);
    c.preferred_provider = Some("claude".into());
    let (route, _) = pick("develop", &c, 1);
    assert_eq!(
        (route.provider.as_str(), route.model.as_str()),
        ("claude", "claude-sonnet-4-6")
    );
}

#[test]
fn escalation_picks_the_next_cheapest_model_one_tier_up_and_skips_the_failed_one() {
    let claude = catalog().remove(0).models;
    let (next, sel) = escalate(&claude, "claude-haiku-4-5").unwrap();
    assert_eq!(next.id, "claude-sonnet-4-6");
    assert_eq!(sel.required_tier, ModelTier::Standard);
    assert!(sel.rationale.contains("claude-haiku-4-5"));
    let (top, _) = escalate(&claude, "claude-sonnet-4-6").unwrap();
    assert_eq!(top.id, "claude-opus-4-7");
    assert!(
        escalate(&claude, "claude-opus-4-7").is_none(),
        "nothing above Premium"
    );
    assert!(escalate(&claude, "unknown-model").is_none());
}

#[test]
fn legacy_slices_without_complexity_still_parse_and_new_fields_round_trip() {
    let legacy =
        r#"{"slices":[{"id":"s1","title":"T","type":"feature","paths":["a"],"ui":false}]}"#;
    let manifest: SlicesManifest = serde_json::from_str(legacy).unwrap();
    assert_eq!(manifest.slices[0].complexity, None);
    assert_eq!(manifest.slices[0].required_tier, None);
    let out = serde_json::to_string(&manifest).unwrap();
    assert!(!out.contains("complexity") && !out.contains("model_rationale"));

    let rich = r#"{"slices":[{"id":"s1","title":"T","type":"feature","paths":["a"],"ui":false,"complexity":"high","required_tier":"premium","model_rationale":"touches auth"}]}"#;
    let manifest: SlicesManifest = serde_json::from_str(rich).unwrap();
    let slice: &Slice = &manifest.slices[0];
    assert_eq!(slice.complexity, Some(Complexity::High));
    assert_eq!(slice.required_tier, Some(ModelTier::Premium));
    assert_eq!(slice.model_rationale.as_deref(), Some("touches auth"));
}

mod team_provenance {
    use std::fs;

    use tandem::adapters::base::{AdapterError, Model};
    use tandem::factory::broker::job::{correlation_id, job_dir};
    use tandem::factory::routing::{Complexity, ModelTier, Selection};
    use tandem::factory::store::FactoryStore;
    use tandem::factory::team::{
        AssignmentSource, load_assignments, record_default_selection, slice_hints,
        suggest_escalation,
    };

    use super::*;
    use crate::common::TempDir;

    const RUN: &str = "r-7k2m9x4q";

    fn runner(_cli: &str) -> Result<Vec<Model>, AdapterError> {
        Ok(vec![
            Model {
                id: "claude-haiku-4-5".into(),
                label: "Haiku".into(),
            },
            Model {
                id: "claude-sonnet-4-6".into(),
                label: "Sonnet".into(),
            },
            Model {
                id: "claude-opus-4-7".into(),
                label: "Opus".into(),
            },
        ])
    }

    fn store_with_job(ws: &TempDir, model: &str, result_status: &str) -> (FactoryStore, String) {
        let store = FactoryStore::new(ws.path());
        store.ensure().unwrap();
        let dir = job_dir(&store, RUN, "develop", 1);
        fs::create_dir_all(&dir).unwrap();
        let input = serde_json::json!({
            "correlation_id": correlation_id(RUN, "develop", 1), "run": RUN, "stage": "develop",
            "attempt": 1, "agent": "senior-backend-dev", "provider": "claude", "model": model,
            "tool_grants": [], "cwd": ws.path().to_string_lossy(), "time_limit_secs": 60,
            "context_manifest": {"items": [], "total_bytes": 0}
        });
        fs::write(dir.join("input.json"), input.to_string()).unwrap();
        fs::write(
            dir.join("result.json"),
            serde_json::json!({"status": result_status, "summary": "x"}).to_string(),
        )
        .unwrap();
        (store, correlation_id(RUN, "develop", 1))
    }

    #[test]
    fn a_failed_default_job_suggests_the_next_tier_and_applies_it_automatically() {
        let ws = TempDir::new();
        let (store, job) = store_with_job(&ws, "claude-haiku-4-5", "failed");
        let s = suggest_escalation(&store, RUN, &job, &runner)
            .unwrap()
            .unwrap();
        assert_eq!(
            (s.cli.as_str(), s.model.as_str()),
            ("claude", "claude-sonnet-4-6")
        );
        assert!(s.applies_automatically);
        assert_eq!(s.selection.required_tier, ModelTier::Standard);
    }

    #[test]
    fn a_succeeded_job_has_no_escalation() {
        let ws = TempDir::new();
        let (store, job) = store_with_job(&ws, "claude-haiku-4-5", "succeeded");
        assert!(
            suggest_escalation(&store, RUN, &job, &runner)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn user_choices_get_a_suggestion_but_are_never_applied_automatically() {
        let ws = TempDir::new();
        let (store, job) = store_with_job(&ws, "claude-haiku-4-5", "failed");
        let mut file = load_assignments(&store, RUN).unwrap();
        file.stages.insert(
            "develop".into(),
            tandem::factory::team::StageAssignment {
                stage: "develop".into(),
                cli: "claude".into(),
                model: "claude-haiku-4-5".into(),
                role: "senior-backend-dev".into(),
                source: AssignmentSource::User,
                updated: 1.0,
                updated_by: "me".into(),
                selection: None,
            },
        );
        tandem::factory::team::save_assignments(&store, RUN, &file).unwrap();
        let s = suggest_escalation(&store, RUN, &job, &runner)
            .unwrap()
            .unwrap();
        assert!(!s.applies_automatically);
        assert_eq!(s.model, "claude-sonnet-4-6");
    }

    #[test]
    fn default_selection_provenance_is_saved_and_never_replaces_a_user_assignment() {
        let ws = TempDir::new();
        let store = FactoryStore::new(ws.path());
        store.ensure().unwrap();
        fs::create_dir_all(store.run_dir(RUN)).unwrap();
        let sel = Selection {
            required_tier: ModelTier::Standard,
            selected_tier: ModelTier::Standard,
            cost_basis: CostBasis::PriceTable,
            candidates: 3,
            rationale: "cheapest".into(),
            source: ModelSource::Live,
        };
        record_default_selection(
            &store,
            RUN,
            "develop",
            "agy",
            "gemini-2.0-flash",
            "senior-backend-dev",
            &sel,
        )
        .unwrap();
        let file = load_assignments(&store, RUN).unwrap();
        let saved = &file.stages["develop"];
        assert_eq!(saved.source, AssignmentSource::Default);
        assert_eq!(saved.selection.as_ref().unwrap().candidates, 3);

        let mut file = file;
        file.stages.get_mut("develop").unwrap().source = AssignmentSource::User;
        file.stages.get_mut("develop").unwrap().selection = None;
        tandem::factory::team::save_assignments(&store, RUN, &file).unwrap();
        record_default_selection(
            &store,
            RUN,
            "develop",
            "claude",
            "claude-opus-4-7",
            "x",
            &sel,
        )
        .unwrap();
        let kept = &load_assignments(&store, RUN).unwrap().stages["develop"];
        assert_eq!(
            (kept.source, kept.model.as_str()),
            (AssignmentSource::User, "gemini-2.0-flash")
        );
    }

    #[test]
    fn slice_hints_take_the_highest_complexity_and_required_tier() {
        let ws = TempDir::new();
        let store = FactoryStore::new(ws.path());
        store.ensure().unwrap();
        assert_eq!(slice_hints(&store, RUN), (None, None));
        let dir = store.run_dir(RUN).join("artifacts");
        fs::create_dir_all(&dir).unwrap();
        let slices = r#"{"slices":[
          {"id":"a","title":"A","type":"f","paths":["x"],"ui":false,"complexity":"low"},
          {"id":"b","title":"B","type":"f","paths":["y"],"ui":false,"complexity":"high","required_tier":"standard"}]}"#;
        fs::write(dir.join("slices.json"), slices).unwrap();
        assert_eq!(
            slice_hints(&store, RUN),
            (Some(Complexity::High), Some(ModelTier::Standard))
        );
    }
}

mod cli {
    use std::process::{Command, Output};

    use crate::common::TempDir;

    fn tandem(ws: &TempDir, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_tandem"))
            .args(["factory", "models"])
            .args(args)
            .env("TANDEM_WORKSPACE", ws.path())
            .env("XDG_STATE_HOME", ws.path().join("state"))
            .env_remove("TANDEM_FACTORY")
            .output()
            .unwrap()
    }

    fn text(out: &Output) -> String {
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    }

    #[test]
    fn recommend_rejects_a_bad_complexity_and_a_bad_tier() {
        let ws = TempDir::new();
        let bad = tandem(
            &ws,
            &[
                "recommend",
                "--run",
                "r-7k2m9x4q",
                "--stage",
                "develop",
                "--complexity",
                "huge",
                "--json",
            ],
        );
        assert!(!bad.status.success());
        assert!(text(&bad).contains("complexity"), "{}", text(&bad));
        let bad = tandem(
            &ws,
            &[
                "recommend",
                "--run",
                "r-7k2m9x4q",
                "--stage",
                "develop",
                "--required-tier",
                "gold",
                "--json",
            ],
        );
        assert!(!bad.status.success());
        assert!(text(&bad).contains("required tier"), "{}", text(&bad));
    }

    #[test]
    fn models_still_requires_a_cli_when_no_subcommand_is_given() {
        let ws = TempDir::new();
        let out = tandem(&ws, &["--json"]);
        assert!(!out.status.success());
        assert!(
            text(&out).to_lowercase().contains("--cli"),
            "{}",
            text(&out)
        );
    }

    #[test]
    fn models_lists_enriched_fields_with_the_old_ones_kept() {
        let ws = TempDir::new();
        let out = tandem(&ws, &["--cli", "claude", "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let first = &v.as_array().unwrap()[0];
        for key in [
            "id",
            "label",
            "tier",
            "cost_rank",
            "cost_basis",
            "allowed",
            "source",
        ] {
            assert!(first.get(key).is_some(), "missing {key}: {first}");
        }
    }
}

mod pinned {
    use tandem::factory::model::FactorySettings;
    use tandem::factory::model_catalog::{ModelSource, with_pinned};

    use super::*;

    #[test]
    fn pinned_tier_models_join_the_discovered_list_without_duplicates() {
        let live = vec![
            cm("sonnet", ModelTier::Standard, 10.0),
            cm("opus", ModelTier::Premium, 100.0),
        ];
        let merged = with_pinned("claude", live, &FactorySettings::default());
        let ids: Vec<&str> = merged.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"claude-haiku-4-5") && ids.contains(&"sonnet"));
        assert_eq!(ids.iter().filter(|&&i| i == "claude-sonnet-4-6").count(), 1);
        let haiku = merged.iter().find(|m| m.id == "claude-haiku-4-5").unwrap();
        assert_eq!(
            (haiku.tier, haiku.source),
            (ModelTier::Economy, ModelSource::Fallback)
        );
        let again = with_pinned("claude", merged.clone(), &FactorySettings::default());
        assert_eq!(again.len(), merged.len());
    }

    #[test]
    fn ties_prefer_classified_models_over_unclassified_ones() {
        let mut fable = cm("fable", ModelTier::Standard, 10.0);
        fable.unclassified = true;
        let cat = vec![CliCatalog {
            cli: "claude".into(),
            models: vec![fable, cm("sonnet", ModelTier::Standard, 10.0)],
        }];
        let (route, _) = super::pick("develop", &super::ctx(cat), 1);
        assert_eq!(route.model, "sonnet");
    }
}
