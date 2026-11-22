use tandem::adapters::base::ToolFlag;
use tandem::factory::model::FactorySettings;
use tandem::factory::permissions::ToolGrant;
use tandem::factory::routing::{
    ModelTier, RoutingContext, TemperatureSupport, Unroutable, map_agent_definition,
    route_interactive as route,
};
use tandem::factory::workflow::DEFAULT_WORKFLOW_YAML;
use tandem::factory::workflow::parse::parse_workflow_str;
use tandem::factory::workflow::schema::{WorkflowAgent, WorkflowTool};
use tandem::factory::workflow::validate::validate;

fn default_wf() -> tandem::factory::workflow::schema::Workflow {
    let raw = parse_workflow_str(DEFAULT_WORKFLOW_YAML).unwrap();
    validate(&raw, &FactorySettings::default()).unwrap()
}

#[test]
fn maps_every_workflow_agent_to_workspace_definition() {
    assert_eq!(
        map_agent_definition(WorkflowAgent::Orchestrator, false),
        "tech-lead-orchestrator"
    );
    assert_eq!(
        map_agent_definition(WorkflowAgent::TestEngineer, false),
        "qa-engineer"
    );
    assert_eq!(
        map_agent_definition(WorkflowAgent::Implementer, false),
        "senior-backend-dev"
    );
    assert_eq!(
        map_agent_definition(WorkflowAgent::Implementer, true),
        "frontend-dev"
    );
    assert_eq!(
        map_agent_definition(WorkflowAgent::Verifier, false),
        "qa-engineer"
    );
    assert_eq!(
        map_agent_definition(WorkflowAgent::Reviewer, false),
        "code-reviewer"
    );
    assert_eq!(
        map_agent_definition(WorkflowAgent::DemoMaker, true),
        "frontend-dev"
    );
    assert_eq!(
        map_agent_definition(WorkflowAgent::ReleaseEngineer, false),
        "terminal-agent"
    );
}

#[test]
fn routes_tiers_upgrades_on_retry_prefers_orchestrator_provider_and_records_temp_support() {
    let wf = default_wf();
    let settings = FactorySettings::default();

    // No open providers -> Unroutable
    let grant0 = ToolGrant::from_tools(
        wf.stages[0].agent,
        &wf.stages[0].name,
        "workspace",
        wf.stages[0].tools.clone(),
    );
    let err = route(
        &wf.stages[0],
        &grant0,
        &wf,
        &settings,
        &[],
        1,
        &RoutingContext::default(),
    )
    .unwrap_err();
    assert!(matches!(err, Unroutable::NoOpenProvider));

    // Plan starts at standard; attempt 2 upgrades to premium
    let ctx = RoutingContext {
        preferred_provider: Some("claude".into()),
        is_ui_work: true,
        ..Default::default()
    };
    let plan_stage = wf.stages.iter().find(|s| s.name == "plan").unwrap();
    let plan_grant = ToolGrant::from_tools(
        plan_stage.agent,
        &plan_stage.name,
        "workspace",
        plan_stage.tools.clone(),
    );
    let r1 = route(
        plan_stage,
        &plan_grant,
        &wf,
        &settings,
        &["agy", "claude"],
        1,
        &ctx,
    )
    .unwrap();
    assert_eq!(r1.provider, "claude");
    assert_eq!(r1.tier, ModelTier::Standard);
    assert_eq!(r1.model, "claude-sonnet-4-6");
    assert_eq!(r1.temperature, TemperatureSupport::Unsupported);

    let r2 = route(
        plan_stage,
        &plan_grant,
        &wf,
        &settings,
        &["agy", "claude"],
        2,
        &ctx,
    )
    .unwrap();
    assert_eq!(r2.tier, ModelTier::Premium);
    assert_eq!(r2.model, "claude-opus-4-7");
    assert!(r2.upgraded_from.is_some());

    // Review starts at economy; read-only tools never map to Edit/Write/Bash
    let review_stage = wf.stages.iter().find(|s| s.name == "review").unwrap();
    let review_grant = ToolGrant::from_tools(
        review_stage.agent,
        &review_stage.name,
        "workspace",
        review_stage.tools.clone(),
    );
    let rev = route(
        review_stage,
        &review_grant,
        &wf,
        &settings,
        &["claude"],
        1,
        &ctx,
    )
    .unwrap();
    assert_eq!(rev.tier, ModelTier::Economy);
    assert_eq!(rev.model, "claude-haiku-4-5");
    assert!(!rev.tool_flags.iter().any(|f| {
        matches!(f, ToolFlag::Allow(name) if name == "Edit" || name == "Write" || name == "Bash")
    }));
}

#[test]
fn unmappable_tool_makes_stage_unroutable_instead_of_widening_permissions() {
    let wf = default_wf();
    let settings = FactorySettings::default();
    let develop_stage = wf.stages.iter().find(|s| s.name == "develop").unwrap();

    // agy interactive workers cannot be granted CI/deployment tooling; it must fail closed as Unroutable
    let dev_grant = ToolGrant::from_tools(
        develop_stage.agent,
        &develop_stage.name,
        "workspace",
        vec![WorkflowTool::RepoWrite, WorkflowTool::Ci],
    );
    let err = route(
        develop_stage,
        &dev_grant,
        &wf,
        &settings,
        &["agy"],
        1,
        &RoutingContext::default(),
    )
    .unwrap_err();
    assert!(matches!(err, Unroutable::UnsupportedTool { .. }));
}
