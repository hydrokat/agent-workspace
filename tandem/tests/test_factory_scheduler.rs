use std::collections::{HashMap, HashSet};

use tandem::factory::model::{
    Actor, ActorKind, Approval, ApprovalGate, FactorySettings, RunState, Stage, StageState,
};
use tandem::factory::workflow::parse::parse_workflow_str;
use tandem::factory::workflow::scheduler::{
    RunFacts, SchedulerLimits, TriState, next, next_stages,
};
use tandem::factory::workflow::schema::{
    Policy, StageBudget, StageCondition, StageDef, StageRequirement, Workflow, WorkflowAgent,
    WorkflowTrigger,
};
use tandem::factory::workflow::validate::validate;

const DEFAULT_WORKFLOW_YAML: &str = r#"name: default-delivery
version: 1
trigger: ready-for-development
policy:
  max_parallel_agents: 3
  default_temperature: 0.4
  require_human_approval: [production_deploy]
stages:
  - name: plan
    agent: orchestrator
    tools: [repo_read, issue_context]
  - name: create-tests
    agent: test-engineer
    depends_on: [plan]
    tools: [repo_read, repo_write, test_runner]
  - name: develop
    agent: implementer
    depends_on: [create-tests]
    tools: [repo_read, repo_write, shell]
  - name: test
    agent: verifier
    depends_on: [develop]
    tools: [test_runner, lint_runner]
  - name: review
    agent: reviewer
    depends_on: [test]
    tools: [repo_read, diff_review]
  - name: demo
    when: ui_changed
    agent: demo-maker
    depends_on: [test]
    tools: [browser, screenshot]
  - name: deploy
    when: deploy_requested
    requires: [human_approval]
    agent: release-engineer
    depends_on: [review]
    tools: [ci, deployment]
"#;

fn default_workflow() -> Workflow {
    let raw = parse_workflow_str(DEFAULT_WORKFLOW_YAML).expect("parse default workflow");
    validate(&raw, &FactorySettings::default()).expect("validate default workflow")
}

fn make_stages(wf: &Workflow, overrides: &[(&str, StageState, u32)]) -> Vec<Stage> {
    let map: HashMap<&str, (StageState, u32)> = overrides
        .iter()
        .map(|&(name, state, attempts)| (name, (state, attempts)))
        .collect();
    wf.stages
        .iter()
        .map(|s| {
            let (state, attempts) = map
                .get(s.name.as_str())
                .copied()
                .unwrap_or((StageState::Pending, 0));
            Stage {
                name: s.name.clone(),
                agent: s.agent.as_str().to_string(),
                state,
                attempts,
                interrupted_attempts: 0,
                started: None,
                finished: None,
                artifacts: vec![],
                rationale: None,
                retry_hypothesis: None,
            }
        })
        .collect()
}

fn protected_approval(subject: Option<&str>) -> Approval {
    Approval {
        gate: ApprovalGate::ProtectedAction,
        actor: Actor {
            kind: ActorKind::User,
            id: "alice".to_string(),
        },
        via: "cli".to_string(),
        at: 100.0,
        subject: subject.map(ToString::to_string),
    }
}

struct StepCase {
    name: &'static str,
    states: Vec<(&'static str, StageState, u32)>,
    ui_changed: TriState,
    deploy_requested: bool,
    approvals: Vec<Approval>,
    expected_start: Vec<&'static str>,
    expected_skip: Vec<&'static str>,
    expected_blocked: Vec<&'static str>,
    expected_waiting: Vec<&'static str>,
    expected_failed: Vec<(&'static str, bool)>,
    expected_finished: Option<RunState>,
}

#[test]
fn test_default_workflow_table_cases() {
    let wf = default_workflow();
    let settings = FactorySettings::default();

    let cases = vec![
        StepCase {
            name: "initial_starts_plan",
            states: vec![],
            ui_changed: TriState::Unknown,
            deploy_requested: false,
            approvals: vec![],
            expected_start: vec!["plan"],
            expected_skip: vec![],
            expected_blocked: vec![],
            expected_waiting: vec![],
            expected_failed: vec![],
            expected_finished: None,
        },
        StepCase {
            name: "plan_succeeded_starts_create_tests",
            states: vec![("plan", StageState::Succeeded, 1)],
            ui_changed: TriState::Unknown,
            deploy_requested: false,
            approvals: vec![],
            expected_start: vec!["create-tests"],
            expected_skip: vec![],
            expected_blocked: vec![],
            expected_waiting: vec![],
            expected_failed: vec![],
            expected_finished: None,
        },
        StepCase {
            name: "create_tests_succeeded_starts_develop",
            states: vec![
                ("plan", StageState::Succeeded, 1),
                ("create-tests", StageState::Succeeded, 1),
            ],
            ui_changed: TriState::Unknown,
            deploy_requested: false,
            approvals: vec![],
            expected_start: vec!["develop"],
            expected_skip: vec![],
            expected_blocked: vec![],
            expected_waiting: vec![],
            expected_failed: vec![],
            expected_finished: None,
        },
        StepCase {
            name: "develop_succeeded_starts_test",
            states: vec![
                ("plan", StageState::Succeeded, 1),
                ("create-tests", StageState::Succeeded, 1),
                ("develop", StageState::Succeeded, 1),
            ],
            ui_changed: TriState::Unknown,
            deploy_requested: false,
            approvals: vec![],
            expected_start: vec!["test"],
            expected_skip: vec![],
            expected_blocked: vec![],
            expected_waiting: vec![],
            expected_failed: vec![],
            expected_finished: None,
        },
        StepCase {
            name: "test_succeeded_ui_changed_yes_starts_review_and_demo",
            states: vec![
                ("plan", StageState::Succeeded, 1),
                ("create-tests", StageState::Succeeded, 1),
                ("develop", StageState::Succeeded, 1),
                ("test", StageState::Succeeded, 1),
            ],
            ui_changed: TriState::Yes,
            deploy_requested: false,
            approvals: vec![],
            expected_start: vec!["review", "demo"],
            expected_skip: vec![],
            expected_blocked: vec![],
            expected_waiting: vec![],
            expected_failed: vec![],
            expected_finished: None,
        },
        StepCase {
            name: "test_succeeded_ui_changed_no_skips_demo",
            states: vec![
                ("plan", StageState::Succeeded, 1),
                ("create-tests", StageState::Succeeded, 1),
                ("develop", StageState::Succeeded, 1),
                ("test", StageState::Succeeded, 1),
            ],
            ui_changed: TriState::No,
            deploy_requested: false,
            approvals: vec![],
            expected_start: vec!["review"],
            expected_skip: vec!["demo"],
            expected_blocked: vec![],
            expected_waiting: vec![],
            expected_failed: vec![],
            expected_finished: None,
        },
        StepCase {
            name: "test_succeeded_ui_changed_unknown_waits_demo",
            states: vec![
                ("plan", StageState::Succeeded, 1),
                ("create-tests", StageState::Succeeded, 1),
                ("develop", StageState::Succeeded, 1),
                ("test", StageState::Succeeded, 1),
            ],
            ui_changed: TriState::Unknown,
            deploy_requested: false,
            approvals: vec![],
            expected_start: vec!["review"],
            expected_skip: vec![],
            expected_blocked: vec![],
            expected_waiting: vec!["demo"],
            expected_failed: vec![],
            expected_finished: None,
        },
        StepCase {
            name: "review_succeeded_deploy_not_requested_skips_deploy",
            states: vec![
                ("plan", StageState::Succeeded, 1),
                ("create-tests", StageState::Succeeded, 1),
                ("develop", StageState::Succeeded, 1),
                ("test", StageState::Succeeded, 1),
                ("review", StageState::Succeeded, 1),
                ("demo", StageState::Skipped, 0),
            ],
            ui_changed: TriState::No,
            deploy_requested: false,
            approvals: vec![],
            expected_start: vec![],
            expected_skip: vec!["deploy"],
            expected_blocked: vec![],
            expected_waiting: vec![],
            expected_failed: vec![],
            expected_finished: None,
        },
        StepCase {
            name: "review_succeeded_deploy_requested_without_approval_blocks_deploy",
            states: vec![
                ("plan", StageState::Succeeded, 1),
                ("create-tests", StageState::Succeeded, 1),
                ("develop", StageState::Succeeded, 1),
                ("test", StageState::Succeeded, 1),
                ("review", StageState::Succeeded, 1),
                ("demo", StageState::Skipped, 0),
            ],
            ui_changed: TriState::No,
            deploy_requested: true,
            approvals: vec![],
            expected_start: vec![],
            expected_skip: vec![],
            expected_blocked: vec!["deploy"],
            expected_waiting: vec![],
            expected_failed: vec![],
            expected_finished: None,
        },
        StepCase {
            name: "review_succeeded_deploy_requested_wrong_subject_still_blocks",
            states: vec![
                ("plan", StageState::Succeeded, 1),
                ("create-tests", StageState::Succeeded, 1),
                ("develop", StageState::Succeeded, 1),
                ("test", StageState::Succeeded, 1),
                ("review", StageState::Succeeded, 1),
                ("demo", StageState::Skipped, 0),
            ],
            ui_changed: TriState::No,
            deploy_requested: true,
            approvals: vec![protected_approval(Some("other-stage"))],
            expected_start: vec![],
            expected_skip: vec![],
            expected_blocked: vec!["deploy"],
            expected_waiting: vec![],
            expected_failed: vec![],
            expected_finished: None,
        },
        StepCase {
            name: "review_succeeded_deploy_requested_with_approval_starts_deploy",
            states: vec![
                ("plan", StageState::Succeeded, 1),
                ("create-tests", StageState::Succeeded, 1),
                ("develop", StageState::Succeeded, 1),
                ("test", StageState::Succeeded, 1),
                ("review", StageState::Succeeded, 1),
                ("demo", StageState::Skipped, 0),
            ],
            ui_changed: TriState::No,
            deploy_requested: true,
            approvals: vec![protected_approval(Some("deploy"))],
            expected_start: vec!["deploy"],
            expected_skip: vec![],
            expected_blocked: vec![],
            expected_waiting: vec![],
            expected_failed: vec![],
            expected_finished: None,
        },
        StepCase {
            name: "all_succeeded_or_skipped_finishes_run",
            states: vec![
                ("plan", StageState::Succeeded, 1),
                ("create-tests", StageState::Succeeded, 1),
                ("develop", StageState::Succeeded, 1),
                ("test", StageState::Succeeded, 1),
                ("review", StageState::Succeeded, 1),
                ("demo", StageState::Skipped, 0),
                ("deploy", StageState::Succeeded, 1),
            ],
            ui_changed: TriState::No,
            deploy_requested: true,
            approvals: vec![protected_approval(Some("deploy"))],
            expected_start: vec![],
            expected_skip: vec![],
            expected_blocked: vec![],
            expected_waiting: vec![],
            expected_failed: vec![],
            expected_finished: Some(RunState::Succeeded),
        },
    ];

    for case in cases {
        let stages = make_stages(&wf, &case.states);
        let facts = RunFacts {
            ui_changed: case.ui_changed,
            deploy_requested: case.deploy_requested,
            approvals: case.approvals,
        };
        let limits = SchedulerLimits::from_workflow_and_settings(&wf, &settings, &stages);
        let decision = next(&wf, &stages, &facts, &limits);
        assert_eq!(next_stages(&wf, &stages, &facts, &limits), decision);

        assert_eq!(decision.start, case.expected_start, "case {}", case.name);
        let skipped_names: Vec<&str> = decision.skip.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(skipped_names, case.expected_skip, "case {}", case.name);
        for s in &decision.skip {
            assert!(!s.reason.is_empty(), "skip reason empty in {}", case.name);
        }
        let blocked_names: Vec<&str> = decision.blocked.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(blocked_names, case.expected_blocked, "case {}", case.name);
        for b in &decision.blocked {
            assert!(
                !b.reason.is_empty(),
                "blocked reason empty in {}",
                case.name
            );
        }
        assert_eq!(
            decision.waiting, case.expected_waiting,
            "case {}",
            case.name
        );
        let failed_summary: Vec<(&str, bool)> = decision
            .failed
            .iter()
            .map(|f| (f.name.as_str(), f.retries_exhausted))
            .collect();
        assert_eq!(failed_summary, case.expected_failed, "case {}", case.name);
        assert_eq!(
            decision.finished, case.expected_finished,
            "case {}",
            case.name
        );
    }
}

#[test]
fn test_failure_at_each_stage_never_advances_dependents() {
    let wf = default_workflow();
    let settings = FactorySettings::default();
    let stage_names: Vec<String> = wf.stages.iter().map(|s| s.name.clone()).collect();

    for (idx, failed_name) in stage_names.iter().enumerate() {
        let mut overrides: Vec<(&str, StageState, u32)> = Vec::new();
        for prior in &stage_names[..idx] {
            overrides.push((prior.as_str(), StageState::Succeeded, 1));
        }
        overrides.push((failed_name.as_str(), StageState::Failed, 1));

        let stages = make_stages(&wf, &overrides);
        let facts = RunFacts {
            ui_changed: TriState::Yes,
            deploy_requested: true,
            approvals: vec![protected_approval(Some("deploy"))],
        };
        let limits = SchedulerLimits::from_workflow_and_settings(&wf, &settings, &stages);
        let decision = next(&wf, &stages, &facts, &limits);

        assert_eq!(decision.failed.len(), 1, "failed stage {failed_name}");
        assert_eq!(decision.failed[0].name, *failed_name);
        assert!(decision.failed[0].retries_exhausted);
        if decision.start.is_empty() {
            assert_eq!(decision.finished, Some(RunState::Failed));
        } else {
            assert_eq!(decision.finished, None);
        }

        for start_name in &decision.start {
            let def = wf.stages.iter().find(|s| s.name == *start_name).unwrap();
            assert!(
                !def.depends_on.contains(failed_name),
                "dependent stage {start_name} must not start when {failed_name} failed"
            );
        }
        for skip in &decision.skip {
            let def = wf.stages.iter().find(|s| s.name == skip.name).unwrap();
            assert!(
                !def.depends_on.contains(failed_name),
                "dependent stage {} must not skip when {failed_name} failed",
                skip.name
            );
        }
    }
}

#[test]
fn test_retry_budget_reporting() {
    let mut wf = default_workflow();
    wf.stages[0].budget.max_retries = Some(2);
    let settings = FactorySettings::default();

    let stages_attempt_1 = make_stages(&wf, &[("plan", StageState::Failed, 1)]);
    let limits = SchedulerLimits::from_workflow_and_settings(&wf, &settings, &stages_attempt_1);
    let d1 = next(&wf, &stages_attempt_1, &RunFacts::default(), &limits);
    assert_eq!(d1.failed.len(), 1);
    assert!(!d1.failed[0].retries_exhausted);
    assert_eq!(d1.finished, None);

    let stages_attempt_3 = make_stages(&wf, &[("plan", StageState::Failed, 3)]);
    let d3 = next(&wf, &stages_attempt_3, &RunFacts::default(), &limits);
    assert_eq!(d3.failed.len(), 1);
    assert!(d3.failed[0].retries_exhausted);
    assert_eq!(d3.finished, Some(RunState::Failed));
}

#[test]
fn test_parallel_limit_cap_and_file_order() {
    let wf = Workflow {
        name: "fanout".to_string(),
        version: 1,
        trigger: WorkflowTrigger::ReadyForDevelopment,
        policy: Policy {
            max_parallel_agents: 4,
            default_temperature: 0.4,
            require_human_approval: vec![],
        },
        stages: vec!["s1", "s2", "s3", "s4", "s5"]
            .into_iter()
            .map(|name| StageDef {
                name: name.to_string(),
                agent: WorkflowAgent::Implementer,
                depends_on: vec![],
                tools: vec![],
                when: None,
                requires: vec![],
                budget: StageBudget::default(),
                evidence: vec![],
            })
            .collect(),
        sha256: "abc".to_string(),
        raw_text: String::new(),
    };

    let settings = FactorySettings {
        max_parallel_agents: 2,
        ..FactorySettings::default()
    };

    let stages = make_stages(&wf, &[]);
    let limits = SchedulerLimits::from_workflow_and_settings(&wf, &settings, &stages);
    assert_eq!(limits.effective_max_parallel, 2);
    let d = next(&wf, &stages, &RunFacts::default(), &limits);
    assert_eq!(d.start, vec!["s1", "s2"]);

    let stages_one_running = make_stages(
        &wf,
        &[
            ("s1", StageState::Running, 1),
            ("s2", StageState::Succeeded, 1),
        ],
    );
    let limits2 = SchedulerLimits::from_workflow_and_settings(&wf, &settings, &stages_one_running);
    assert_eq!(limits2.running_count, 1);
    let d2 = next(&wf, &stages_one_running, &RunFacts::default(), &limits2);
    assert_eq!(d2.start, vec!["s3"]);

    let stages_full = make_stages(
        &wf,
        &[
            ("s1", StageState::Running, 1),
            ("s2", StageState::Running, 1),
        ],
    );
    let limits_full = SchedulerLimits::from_workflow_and_settings(&wf, &settings, &stages_full);
    let d_full = next(&wf, &stages_full, &RunFacts::default(), &limits_full);
    assert!(d_full.start.is_empty());
}

struct XorShift64(u64);

impl XorShift64 {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn gen_range(&mut self, upper: usize) -> usize {
        (self.next_u64() as usize) % upper
    }
}

fn generate_dag(rng: &mut XorShift64, num_stages: usize, max_parallel: u32) -> Workflow {
    let mut stages = Vec::with_capacity(num_stages);
    for i in 0..num_stages {
        let mut depends_on = Vec::new();
        for j in 0..i {
            if rng.gen_range(3) == 0 {
                depends_on.push(format!("stage-{j}"));
            }
        }
        let when = match rng.gen_range(5) {
            0 => Some(StageCondition::UiChanged),
            1 => Some(StageCondition::DeployRequested),
            _ => None,
        };
        let requires = if rng.gen_range(6) == 0 {
            vec![StageRequirement::HumanApproval]
        } else {
            vec![]
        };
        stages.push(StageDef {
            name: format!("stage-{i}"),
            agent: WorkflowAgent::Implementer,
            depends_on,
            tools: vec![],
            when,
            requires,
            budget: StageBudget::default(),
            evidence: vec![],
        });
    }
    Workflow {
        name: "generated".to_string(),
        version: 1,
        trigger: WorkflowTrigger::ReadyForDevelopment,
        policy: Policy {
            max_parallel_agents: max_parallel,
            default_temperature: 0.4,
            require_human_approval: vec![],
        },
        stages,
        sha256: "gen".to_string(),
        raw_text: String::new(),
    }
}

#[test]
fn test_generated_dags_never_violate_invariants() {
    let mut rng = XorShift64(0x9e37_79b9_7f4a_7c15);

    for case_idx in 0..60 {
        let num_stages = 2 + rng.gen_range(12);
        let wf_cap = 1 + rng.gen_range(4) as u32;
        let settings_cap = 1 + rng.gen_range(4);
        let wf = generate_dag(&mut rng, num_stages, wf_cap);
        let settings = FactorySettings {
            max_parallel_agents: settings_cap,
            ..FactorySettings::default()
        };

        let ui_changed = match rng.gen_range(3) {
            0 => TriState::Yes,
            1 => TriState::No,
            _ => TriState::Unknown,
        };
        let deploy_requested = rng.gen_range(2) == 0;
        let grant_approvals = rng.gen_range(2) == 0;
        let approvals = if grant_approvals {
            vec![protected_approval(None)]
        } else {
            vec![]
        };
        let facts = RunFacts {
            ui_changed,
            deploy_requested,
            approvals,
        };

        let mut stages = make_stages(&wf, &[]);
        let mut ticks = 0;

        loop {
            ticks += 1;
            assert!(
                ticks <= 200,
                "dag {case_idx} did not terminate within 200 ticks"
            );

            let limits = SchedulerLimits::from_workflow_and_settings(&wf, &settings, &stages);
            let decision = next(&wf, &stages, &facts, &limits);

            assert!(
                limits.running_count + decision.start.len() <= limits.effective_max_parallel,
                "dag {case_idx} exceeded parallel limit"
            );

            let state_by_name: HashMap<&str, StageState> =
                stages.iter().map(|s| (s.name.as_str(), s.state)).collect();

            for start_name in &decision.start {
                let def = wf.stages.iter().find(|s| s.name == *start_name).unwrap();
                for dep in &def.depends_on {
                    let dep_state = state_by_name[dep.as_str()];
                    assert!(
                        matches!(dep_state, StageState::Succeeded | StageState::Skipped),
                        "dag {case_idx} started {start_name} while dependency {dep} was {dep_state:?}"
                    );
                }
            }

            if decision.finished.is_some() {
                break;
            }

            let skip_set: HashSet<&str> = decision.skip.iter().map(|s| s.name.as_str()).collect();
            let start_set: HashSet<&str> = decision.start.iter().map(String::as_str).collect();

            if start_set.is_empty() && skip_set.is_empty() && limits.running_count == 0 {
                assert!(
                    !decision.blocked.is_empty()
                        || !decision.waiting.is_empty()
                        || !decision.failed.is_empty(),
                    "dag {case_idx} stalled without reporting why"
                );
                break;
            }

            for stage in &mut stages {
                if skip_set.contains(stage.name.as_str()) {
                    stage.state = StageState::Skipped;
                } else if start_set.contains(stage.name.as_str()) {
                    stage.state = StageState::Running;
                    stage.attempts += 1;
                } else if stage.state == StageState::Running {
                    stage.state = StageState::Succeeded;
                }
            }
        }
    }
}
