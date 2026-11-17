use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const MAX_WORKFLOW_BYTES: usize = 64 * 1024;
pub const MAX_DEPTH: usize = 8;
pub const MAX_STAGES: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("refusing to follow symlink at {}", .0.display())]
    Symlink(PathBuf),
    #[error("workflow path is not a regular file: {}", .0.display())]
    NotRegularFile(PathBuf),
    #[error("workflow file size {bytes} exceeds {max} byte limit")]
    TooLarge { bytes: usize, max: usize },
    #[error("line {line}: {message}")]
    Syntax { line: usize, message: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub value: T,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpannedList {
    pub items: Vec<Spanned<String>>,
    pub line: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawPolicy {
    pub line: usize,
    pub max_parallel_agents: Option<Spanned<i64>>,
    pub default_temperature: Option<Spanned<f64>>,
    pub require_human_approval: Option<SpannedList>,
    pub budget_max_cost_usd: Option<Spanned<f64>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawStage {
    pub line: usize,
    pub name: Option<Spanned<String>>,
    pub agent: Option<Spanned<String>>,
    pub depends_on: Option<SpannedList>,
    pub tools: Option<SpannedList>,
    pub when: Option<Spanned<String>>,
    pub requires: Option<SpannedList>,
    pub retries: Option<Spanned<i64>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawWorkflow {
    pub name: Option<Spanned<String>>,
    pub version: Option<Spanned<i64>>,
    pub trigger: Option<Spanned<String>>,
    pub policy: Option<RawPolicy>,
    pub stages: Vec<RawStage>,
    pub sha256: String,
    pub raw_text: String,
}

fn strip_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_double => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single
                && !in_double
                && (idx == 0 || line[..idx].ends_with(char::is_whitespace)) =>
            {
                return &line[..idx];
            }
            _ => {}
        }
    }
    line
}

fn unquote_scalar(raw: &str, line: usize) -> Result<String, ParseError> {
    let t = raw.trim();
    if t.starts_with('&') {
        return Err(ParseError::Syntax {
            line,
            message: "YAML anchors (&) are not allowed".to_string(),
        });
    }
    if t.starts_with('*') {
        return Err(ParseError::Syntax {
            line,
            message: "YAML aliases (*) are not allowed".to_string(),
        });
    }
    if t.starts_with('!') {
        return Err(ParseError::Syntax {
            line,
            message: "YAML tags (!) are not allowed".to_string(),
        });
    }
    if let Some(inner) = t
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| t.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
    {
        Ok(inner.to_string())
    } else {
        Ok(t.to_string())
    }
}

fn parse_flow_list(raw: &str, line: usize) -> Result<Vec<Spanned<String>>, ParseError> {
    let t = raw.trim();
    if !t.starts_with('[') || !t.ends_with(']') {
        return Err(ParseError::Syntax {
            line,
            message: "expected flow list '[...] '".to_string(),
        });
    }
    let inner = t[1..t.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for part in inner.split(',') {
        let s = unquote_scalar(part, line)?;
        if s.is_empty() {
            return Err(ParseError::Syntax {
                line,
                message: "empty list item".to_string(),
            });
        }
        out.push(Spanned { value: s, line });
    }
    Ok(out)
}

fn suggest_key(unknown: &str, allowed: &[&str]) -> String {
    for &cand in allowed {
        if cand.starts_with(unknown) || unknown.starts_with(cand) {
            return format!("unknown key \"{unknown}\" (did you mean \"{cand}\"?)");
        }
    }
    format!("unknown key \"{unknown}\"")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Root,
    Policy,
    Stages,
    StageItem,
    List { target: ListTarget },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ListTarget {
    PolicyApproval,
    StageDependsOn,
    StageTools,
    StageRequires,
}

pub fn load_workflow_file(path: &Path) -> Result<RawWorkflow, ParseError> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(ParseError::Symlink(path.to_path_buf()));
    }
    if !meta.file_type().is_file() {
        return Err(ParseError::NotRegularFile(path.to_path_buf()));
    }
    if meta.len() as usize > MAX_WORKFLOW_BYTES {
        return Err(ParseError::TooLarge {
            bytes: meta.len() as usize,
            max: MAX_WORKFLOW_BYTES,
        });
    }

    let bytes = fs::read(path)?;
    if bytes.len() > MAX_WORKFLOW_BYTES {
        return Err(ParseError::TooLarge {
            bytes: bytes.len(),
            max: MAX_WORKFLOW_BYTES,
        });
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| ParseError::Syntax {
        line: 1,
        message: "workflow file must be valid UTF-8".to_string(),
    })?;

    use sha2::{Digest, Sha256};
    let digest = Sha256::new().chain_update(&bytes).finalize();
    let sha256: String = digest.iter().map(|b| format!("{b:02x}")).collect();

    let mut wf = parse_workflow_str(text)?;
    wf.sha256 = sha256;
    wf.raw_text = text.to_string();
    Ok(wf)
}

pub fn parse_workflow_str(text: &str) -> Result<RawWorkflow, ParseError> {
    let mut wf = RawWorkflow::default();
    let mut section = Section::Root;
    let mut root_keys = HashSet::new();
    let mut policy_keys = HashSet::new();
    let mut stage_keys: HashSet<String> = HashSet::new();
    let mut indent_stack: Vec<usize> = vec![0];

    for (idx, raw_line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let clean = strip_comment(raw_line).trim_end();
        if clean.trim().is_empty() {
            continue;
        }
        let trimmed = clean.trim();
        if trimmed == "---" || trimmed == "..." {
            return Err(ParseError::Syntax {
                line: line_no,
                message: "multiple YAML documents are not allowed".to_string(),
            });
        }
        if trimmed.contains("<<:") || trimmed.starts_with("<<") {
            return Err(ParseError::Syntax {
                line: line_no,
                message: "YAML merge keys (<<) are not allowed".to_string(),
            });
        }

        let indent = clean.len() - clean.trim_start_matches(' ').len();
        if clean.trim_start().starts_with('\t') {
            return Err(ParseError::Syntax {
                line: line_no,
                message: "tabs are not allowed for YAML indentation".to_string(),
            });
        }
        while indent_stack.last().is_some_and(|&top| indent < top) {
            indent_stack.pop();
        }
        if indent_stack.last().is_some_and(|&top| indent > top) {
            indent_stack.push(indent);
            if indent_stack.len() > MAX_DEPTH {
                return Err(ParseError::Syntax {
                    line: line_no,
                    message: format!("nesting depth exceeds maximum of {MAX_DEPTH}"),
                });
            }
        }

        if indent == 0 && !trimmed.starts_with('-') {
            section = Section::Root;
        } else if matches!(section, Section::List { .. }) && !trimmed.starts_with('-') {
            section = if indent <= 2 {
                Section::Policy
            } else {
                Section::StageItem
            };
        }

        if let Some(after_dash) = trimmed.strip_prefix('-')
            && (after_dash.is_empty() || after_dash.starts_with(' '))
        {
            let rest = after_dash.trim();
            match section {
                Section::Stages | Section::StageItem if indent <= 4 => {
                    if wf.stages.len() >= MAX_STAGES {
                        return Err(ParseError::Syntax {
                            line: line_no,
                            message: format!("stage count exceeds maximum of {MAX_STAGES}"),
                        });
                    }
                    wf.stages.push(RawStage {
                        line: line_no,
                        ..Default::default()
                    });
                    stage_keys.clear();
                    section = Section::StageItem;
                    if !rest.is_empty() {
                        parse_stage_kv(rest, line_no, &mut wf, &mut stage_keys, &mut section)?;
                    }
                    continue;
                }
                Section::List { target } => {
                    let val = unquote_scalar(rest, line_no)?;
                    append_list_item(
                        &mut wf,
                        target,
                        Spanned {
                            value: val,
                            line: line_no,
                        },
                    );
                    continue;
                }
                _ => {
                    return Err(ParseError::Syntax {
                        line: line_no,
                        message: "unexpected list item".to_string(),
                    });
                }
            }
        }

        let Some((k_raw, v_raw)) = trimmed.split_once(':') else {
            return Err(ParseError::Syntax {
                line: line_no,
                message: "expected 'key: value'".to_string(),
            });
        };
        let key = k_raw.trim();
        let val = v_raw.trim();

        match section {
            Section::Root => {
                if !root_keys.insert(key.to_string()) {
                    return Err(ParseError::Syntax {
                        line: line_no,
                        message: format!("duplicate key \"{key}\""),
                    });
                }
                match key {
                    "name" => {
                        wf.name = Some(Spanned {
                            value: unquote_scalar(val, line_no)?,
                            line: line_no,
                        });
                    }
                    "version" => {
                        let s = unquote_scalar(val, line_no)?;
                        let n = s.parse::<i64>().map_err(|_| ParseError::Syntax {
                            line: line_no,
                            message: format!("expected integer version, got \"{s}\""),
                        })?;
                        wf.version = Some(Spanned {
                            value: n,
                            line: line_no,
                        });
                    }
                    "trigger" => {
                        wf.trigger = Some(Spanned {
                            value: unquote_scalar(val, line_no)?,
                            line: line_no,
                        });
                    }
                    "policy" => {
                        wf.policy = Some(RawPolicy {
                            line: line_no,
                            ..Default::default()
                        });
                        policy_keys.clear();
                        section = Section::Policy;
                    }
                    "stages" => {
                        section = Section::Stages;
                    }
                    _ => {
                        return Err(ParseError::Syntax {
                            line: line_no,
                            message: suggest_key(
                                key,
                                &["name", "version", "trigger", "policy", "stages"],
                            ),
                        });
                    }
                }
            }
            Section::Policy => {
                if !policy_keys.insert(key.to_string()) {
                    return Err(ParseError::Syntax {
                        line: line_no,
                        message: format!("duplicate policy key \"{key}\""),
                    });
                }
                let pol = wf.policy.as_mut().expect("policy initialized");
                match key {
                    "max_parallel_agents" => {
                        let s = unquote_scalar(val, line_no)?;
                        let n = s.parse::<i64>().map_err(|_| ParseError::Syntax {
                            line: line_no,
                            message: format!("expected integer, got \"{s}\""),
                        })?;
                        pol.max_parallel_agents = Some(Spanned {
                            value: n,
                            line: line_no,
                        });
                    }
                    "default_temperature" => {
                        let s = unquote_scalar(val, line_no)?;
                        let f = s.parse::<f64>().map_err(|_| ParseError::Syntax {
                            line: line_no,
                            message: format!("expected float, got \"{s}\""),
                        })?;
                        pol.default_temperature = Some(Spanned {
                            value: f,
                            line: line_no,
                        });
                    }
                    "budget_max_cost_usd" => {
                        let s = unquote_scalar(val, line_no)?;
                        let f = s.parse::<f64>().map_err(|_| ParseError::Syntax {
                            line: line_no,
                            message: format!("expected float, got \"{s}\""),
                        })?;
                        pol.budget_max_cost_usd = Some(Spanned {
                            value: f,
                            line: line_no,
                        });
                    }
                    "require_human_approval" => {
                        if val.is_empty() {
                            pol.require_human_approval = Some(SpannedList {
                                items: Vec::new(),
                                line: line_no,
                            });
                            section = Section::List {
                                target: ListTarget::PolicyApproval,
                            };
                        } else {
                            pol.require_human_approval = Some(SpannedList {
                                items: parse_flow_list(val, line_no)?,
                                line: line_no,
                            });
                        }
                    }
                    _ => {
                        return Err(ParseError::Syntax {
                            line: line_no,
                            message: suggest_key(
                                key,
                                &[
                                    "max_parallel_agents",
                                    "default_temperature",
                                    "require_human_approval",
                                    "budget_max_cost_usd",
                                ],
                            ),
                        });
                    }
                }
            }
            Section::StageItem | Section::Stages => {
                parse_stage_kv(trimmed, line_no, &mut wf, &mut stage_keys, &mut section)?;
            }
            Section::List { .. } => unreachable!(),
        }
    }

    Ok(wf)
}

fn append_list_item(wf: &mut RawWorkflow, target: ListTarget, item: Spanned<String>) {
    match target {
        ListTarget::PolicyApproval => {
            if let Some(ref mut p) = wf.policy
                && let Some(ref mut list) = p.require_human_approval
            {
                list.items.push(item);
            }
        }
        ListTarget::StageDependsOn => {
            if let Some(s) = wf.stages.last_mut()
                && let Some(ref mut list) = s.depends_on
            {
                list.items.push(item);
            }
        }
        ListTarget::StageTools => {
            if let Some(s) = wf.stages.last_mut()
                && let Some(ref mut list) = s.tools
            {
                list.items.push(item);
            }
        }
        ListTarget::StageRequires => {
            if let Some(s) = wf.stages.last_mut()
                && let Some(ref mut list) = s.requires
            {
                list.items.push(item);
            }
        }
    }
}

fn parse_stage_kv(
    trimmed: &str,
    line_no: usize,
    wf: &mut RawWorkflow,
    stage_keys: &mut HashSet<String>,
    section: &mut Section,
) -> Result<(), ParseError> {
    let Some((k_raw, v_raw)) = trimmed.split_once(':') else {
        return Err(ParseError::Syntax {
            line: line_no,
            message: "expected 'key: value' inside stage".to_string(),
        });
    };
    let key = k_raw.trim();
    let val = v_raw.trim();
    if !stage_keys.insert(key.to_string()) {
        return Err(ParseError::Syntax {
            line: line_no,
            message: format!("duplicate stage key \"{key}\""),
        });
    }
    let stage = wf.stages.last_mut().ok_or_else(|| ParseError::Syntax {
        line: line_no,
        message: "stage key outside stage item".to_string(),
    })?;
    match key {
        "name" => {
            stage.name = Some(Spanned {
                value: unquote_scalar(val, line_no)?,
                line: line_no,
            });
        }
        "agent" => {
            stage.agent = Some(Spanned {
                value: unquote_scalar(val, line_no)?,
                line: line_no,
            });
        }
        "when" => {
            stage.when = Some(Spanned {
                value: unquote_scalar(val, line_no)?,
                line: line_no,
            });
        }
        "retries" => {
            let s = unquote_scalar(val, line_no)?;
            let n = s.parse::<i64>().map_err(|_| ParseError::Syntax {
                line: line_no,
                message: format!("expected integer retries, got \"{s}\""),
            })?;
            stage.retries = Some(Spanned {
                value: n,
                line: line_no,
            });
        }
        "depends_on" => {
            if val.is_empty() {
                stage.depends_on = Some(SpannedList {
                    items: Vec::new(),
                    line: line_no,
                });
                *section = Section::List {
                    target: ListTarget::StageDependsOn,
                };
            } else {
                stage.depends_on = Some(SpannedList {
                    items: parse_flow_list(val, line_no)?,
                    line: line_no,
                });
            }
        }
        "tools" => {
            if val.is_empty() {
                stage.tools = Some(SpannedList {
                    items: Vec::new(),
                    line: line_no,
                });
                *section = Section::List {
                    target: ListTarget::StageTools,
                };
            } else {
                stage.tools = Some(SpannedList {
                    items: parse_flow_list(val, line_no)?,
                    line: line_no,
                });
            }
        }
        "requires" => {
            if val.is_empty() {
                stage.requires = Some(SpannedList {
                    items: Vec::new(),
                    line: line_no,
                });
                *section = Section::List {
                    target: ListTarget::StageRequires,
                };
            } else {
                stage.requires = Some(SpannedList {
                    items: parse_flow_list(val, line_no)?,
                    line: line_no,
                });
            }
        }
        _ => {
            return Err(ParseError::Syntax {
                line: line_no,
                message: suggest_key(
                    key,
                    &[
                        "name",
                        "agent",
                        "depends_on",
                        "tools",
                        "when",
                        "requires",
                        "retries",
                    ],
                ),
            });
        }
    }
    Ok(())
}
