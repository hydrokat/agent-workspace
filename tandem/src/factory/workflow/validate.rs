use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::parse::RawWorkflow;
use super::schema::{
    EvidenceKind, Policy, ProtectedAction, StageBudget, StageCondition, StageDef, StageRequirement,
    Workflow, WorkflowAgent, WorkflowTool, WorkflowTrigger,
};
use crate::factory::model::FactorySettings;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {} {}", self.line, self.code, self.message)
    }
}

pub fn is_valid_workflow_identifier(s: &str) -> bool {
    let bytes = s.as_bytes();
    (1..=48).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn resolve_workflow_path(workspace: &Path, name_or_path: &str) -> Result<PathBuf, String> {
    if is_valid_workflow_identifier(name_or_path) {
        return Ok(workspace
            .join("workflows")
            .join(format!("{name_or_path}.yaml")));
    }
    Err(format!(
        "invalid workflow name \"{name_or_path}\": must match ^[a-z][a-z0-9-]{{0,47}}$"
    ))
}

pub fn validate(
    raw: &RawWorkflow,
    settings: &FactorySettings,
) -> Result<Workflow, Vec<Diagnostic>> {
    let mut diags = Vec::new();

    let name = match &raw.name {
        Some(n) if is_valid_workflow_identifier(&n.value) => n.value.clone(),
        Some(n) => {
            diags.push(Diagnostic {
                code: "W001",
                line: n.line,
                message: format!(
                    "workflow name \"{}\" must match ^[a-z][a-z0-9-]{{0,47}}$",
                    n.value
                ),
            });
            n.value.clone()
        }
        None => {
            diags.push(Diagnostic {
                code: "W001",
                line: 1,
                message: "missing workflow name".to_string(),
            });
            String::new()
        }
    };

    let version = match &raw.version {
        Some(v) if v.value > 0 && v.value <= u32::MAX as i64 => v.value as u32,
        Some(v) => {
            diags.push(Diagnostic {
                code: "W001",
                line: v.line,
                message: format!("workflow version {} must be a positive integer", v.value),
            });
            1
        }
        None => {
            diags.push(Diagnostic {
                code: "W001",
                line: 1,
                message: "missing workflow version".to_string(),
            });
            1
        }
    };

    let trigger = match &raw.trigger {
        Some(t) => match WorkflowTrigger::parse(&t.value) {
            Some(parsed) => parsed,
            None => {
                diags.push(Diagnostic {
                    code: "W002",
                    line: t.line,
                    message: format!("unknown trigger \"{}\"", t.value),
                });
                WorkflowTrigger::ReadyForDevelopment
            }
        },
        None => {
            diags.push(Diagnostic {
                code: "W002",
                line: 1,
                message: "missing workflow trigger".to_string(),
            });
            WorkflowTrigger::ReadyForDevelopment
        }
    };

    let mut max_parallel = 1u32;
    let mut default_temp = 0.4f64;
    let mut approvals = Vec::new();

    if let Some(ref pol) = raw.policy {
        if let Some(ref mp) = pol.max_parallel_agents {
            if mp.value < 1 || mp.value as usize > settings.max_parallel_agents {
                diags.push(Diagnostic {
                    code: "W009",
                    line: mp.line,
                    message: format!(
                        "max_parallel_agents {} must be between 1 and factory limit {}",
                        mp.value, settings.max_parallel_agents
                    ),
                });
            } else {
                max_parallel = mp.value as u32;
            }
        }
        if let Some(ref dt) = pol.default_temperature {
            if !(0.0..=1.0).contains(&dt.value) || dt.value.is_nan() {
                diags.push(Diagnostic {
                    code: "W010",
                    line: dt.line,
                    message: format!(
                        "default_temperature {} must be between 0.0 and 1.0",
                        dt.value
                    ),
                });
            } else {
                default_temp = dt.value;
            }
        }
        if let Some(ref reqs) = pol.require_human_approval {
            for item in &reqs.items {
                match ProtectedAction::parse(&item.value) {
                    Some(pa) => approvals.push(pa),
                    None => diags.push(Diagnostic {
                        code: "W007",
                        line: item.line,
                        message: format!("unknown protected action \"{}\"", item.value),
                    }),
                }
            }
        }
    }

    let mut stage_names = HashSet::new();
    let mut stage_name_list = Vec::new();
    let mut stages = Vec::new();
    let mut has_test_runner = false;
    let mut has_diff_review = false;
    let mut deployment_lines = Vec::new();

    for st in &raw.stages {
        let sname = match &st.name {
            Some(n) => {
                if !is_valid_workflow_identifier(&n.value) {
                    diags.push(Diagnostic {
                        code: "W003",
                        line: n.line,
                        message: format!(
                            "stage name \"{}\" must match ^[a-z][a-z0-9-]{{0,47}}$",
                            n.value
                        ),
                    });
                }
                if !stage_names.insert(n.value.clone()) {
                    diags.push(Diagnostic {
                        code: "W003",
                        line: n.line,
                        message: format!("duplicate stage name \"{}\"", n.value),
                    });
                }
                stage_name_list.push((n.value.clone(), st.line));
                n.value.clone()
            }
            None => {
                diags.push(Diagnostic {
                    code: "W003",
                    line: st.line,
                    message: "stage missing name".to_string(),
                });
                String::new()
            }
        };

        let agent = match &st.agent {
            Some(a) => match WorkflowAgent::parse(&a.value) {
                Some(parsed) => parsed,
                None => {
                    diags.push(Diagnostic {
                        code: "W007",
                        line: a.line,
                        message: format!("unknown agent \"{}\"", a.value),
                    });
                    WorkflowAgent::Implementer
                }
            },
            None => {
                diags.push(Diagnostic {
                    code: "W007",
                    line: st.line,
                    message: format!("stage \"{sname}\" missing agent"),
                });
                WorkflowAgent::Implementer
            }
        };

        let mut tools = Vec::new();
        let mut uses_deployment = false;
        if let Some(ref tlist) = st.tools {
            for t in &tlist.items {
                match WorkflowTool::parse(&t.value) {
                    Some(parsed) => {
                        if parsed == WorkflowTool::TestRunner {
                            has_test_runner = true;
                        }
                        if parsed == WorkflowTool::DiffReview {
                            has_diff_review = true;
                        }
                        if parsed == WorkflowTool::Deployment {
                            uses_deployment = true;
                        }
                        tools.push(parsed);
                    }
                    None => diags.push(Diagnostic {
                        code: "W007",
                        line: t.line,
                        message: format!("unknown tool \"{}\"", t.value),
                    }),
                }
            }
        }

        let when = st
            .when
            .as_ref()
            .and_then(|w| match StageCondition::parse(&w.value) {
                Some(c) => Some(c),
                None => {
                    diags.push(Diagnostic {
                        code: "W007",
                        line: w.line,
                        message: format!("unknown condition \"{}\"", w.value),
                    });
                    None
                }
            });

        let mut requires = Vec::new();
        if let Some(ref rlist) = st.requires {
            for r in &rlist.items {
                match StageRequirement::parse(&r.value) {
                    Some(parsed) => requires.push(parsed),
                    None => diags.push(Diagnostic {
                        code: "W007",
                        line: r.line,
                        message: format!("unknown requirement \"{}\"", r.value),
                    }),
                }
            }
        }

        if uses_deployment {
            deployment_lines.push((sname.clone(), st.line));
            if !requires.contains(&StageRequirement::HumanApproval) {
                diags.push(Diagnostic {
                    code: "W008",
                    line: st.line,
                    message: format!(
                        "stage \"{sname}\" uses deployment and must declare requires: [human_approval]"
                    ),
                });
            }
        }

        let mut budget = StageBudget::default();
        if let Some(ref retries) = st.retries {
            if retries.value < 0 {
                diags.push(Diagnostic {
                    code: "W012",
                    line: retries.line,
                    message: "stage retries must not be negative".to_string(),
                });
            } else {
                budget.max_retries = Some(retries.value as u32);
            }
        }

        let depends_on: Vec<String> = st
            .depends_on
            .as_ref()
            .map(|d| d.items.iter().map(|i| i.value.clone()).collect())
            .unwrap_or_default();

        let mut evidence = Vec::new();
        if let Some(ref ev_list) = st.evidence {
            for item in &ev_list.items {
                match EvidenceKind::parse(&item.value) {
                    Some(k) => evidence.push(k),
                    None => diags.push(Diagnostic {
                        code: "W013",
                        line: item.line,
                        message: format!(
                            "unknown evidence kind \"{}\" on stage \"{}\"",
                            item.value, sname
                        ),
                    }),
                }
            }
        }

        stages.push(StageDef {
            name: sname,
            agent,
            depends_on,
            tools,
            when,
            requires,
            budget,
            evidence,
        });
    }

    // W004: every depends_on entry names an existing stage
    for (idx, st) in raw.stages.iter().enumerate() {
        if let Some(ref deps) = st.depends_on {
            for dep in &deps.items {
                if !stage_names.contains(&dep.value) {
                    diags.push(Diagnostic {
                        code: "W004",
                        line: dep.line,
                        message: format!(
                            "stage \"{}\" depends_on unknown stage \"{}\"",
                            stages[idx].name, dep.value
                        ),
                    });
                }
            }
        }
    }

    // W006: exactly one root stage (0 dependencies)
    let root_count = stages.iter().filter(|s| s.depends_on.is_empty()).count();
    if root_count != 1 {
        diags.push(Diagnostic {
            code: "W006",
            line: raw.stages.first().map_or(1, |s| s.line),
            message: format!(
                "expected exactly 1 root stage without depends_on, found {root_count}"
            ),
        });
    }

    // W005: cycle detection
    if has_cycle(&stages) {
        diags.push(Diagnostic {
            code: "W005",
            line: raw.stages.first().map_or(1, |s| s.line),
            message: "workflow stage graph contains a dependency cycle".to_string(),
        });
    }

    // W011: required test_runner and diff_review stages
    if !has_test_runner || !has_diff_review {
        diags.push(Diagnostic {
            code: "W011",
            line: raw.stages.first().map_or(1, |s| s.line),
            message:
                "workflow must include at least one test_runner stage and one diff_review stage"
                    .to_string(),
        });
    }

    // W013: deployment requires production_deploy in policy.require_human_approval
    if !deployment_lines.is_empty() && !approvals.contains(&ProtectedAction::ProductionDeploy) {
        diags.push(Diagnostic {
            code: "W013",
            line: raw.policy.as_ref().map_or(1, |p| p.line),
            message: "policy.require_human_approval must include production_deploy when a stage uses deployment".to_string(),
        });
    }

    if diags.is_empty() {
        let policy = crate::factory::settings::effective(
            &Policy {
                max_parallel_agents: max_parallel,
                default_temperature: default_temp,
                require_human_approval: approvals,
            },
            settings,
        );
        Ok(Workflow {
            name,
            version,
            trigger,
            policy,
            stages,
            sha256: raw.sha256.clone(),
            raw_text: raw.raw_text.clone(),
        })
    } else {
        Err(diags)
    }
}

fn has_cycle(stages: &[StageDef]) -> bool {
    let mut state: HashMap<&str, u8> = HashMap::new();
    let by_name: HashMap<&str, &StageDef> = stages.iter().map(|s| (s.name.as_str(), s)).collect();

    fn dfs<'a>(
        name: &'a str,
        by_name: &HashMap<&'a str, &'a StageDef>,
        state: &mut HashMap<&'a str, u8>,
    ) -> bool {
        match state.get(name).copied() {
            Some(1) => return true,
            Some(2) => return false,
            _ => {}
        }
        state.insert(name, 1);
        if let Some(st) = by_name.get(name) {
            for dep in &st.depends_on {
                if by_name.contains_key(dep.as_str()) && dfs(dep.as_str(), by_name, state) {
                    return true;
                }
            }
        }
        state.insert(name, 2);
        false
    }

    for st in stages {
        if dfs(st.name.as_str(), &by_name, &mut state) {
            return true;
        }
    }
    false
}
