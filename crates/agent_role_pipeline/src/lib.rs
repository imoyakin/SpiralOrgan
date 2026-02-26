use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

pub const ROLE_PIPELINE_API_VERSION: &str = "spiralorgan.role-pipeline/v0.1";
pub const SELECTED_WORKER_ROLE_TOKEN: &str = "$selected_worker";

fn default_true() -> bool {
    true
}

fn default_edge_priority() -> u16 {
    100
}

fn default_max_iterations() -> u16 {
    24
}

fn default_max_workers_per_tick() -> u16 {
    8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LintSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintIssue {
    pub severity: LintSeverity,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RoleKind {
    #[default]
    Scheduler,
    Worker,
    Verifier,
    Reviewer,
    Router,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RoleProtocolKind {
    #[default]
    #[serde(rename = "google_a2a")]
    GoogleA2A,
    LocalLoop,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RoleProtocol {
    #[serde(default)]
    pub kind: RoleProtocolKind,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DispatchKind {
    #[default]
    Inherit,
    Provider,
    LocalClient,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DispatchTarget {
    #[serde(default)]
    pub kind: DispatchKind,
    #[serde(default)]
    pub reference: Option<String>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleSpec {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub kind: RoleKind,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub protocol: RoleProtocol,
    #[serde(default)]
    pub dispatch_target: DispatchTarget,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageKind {
    Start,
    Intake,
    SchedulerDispatch,
    WorkerDispatch,
    Verify,
    End,
    Custom,
}

impl StageKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            StageKind::Start => "start",
            StageKind::Intake => "intake",
            StageKind::SchedulerDispatch => "scheduler_dispatch",
            StageKind::WorkerDispatch => "worker_dispatch",
            StageKind::Verify => "verify",
            StageKind::End => "end",
            StageKind::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageSpec {
    pub id: String,
    pub kind: StageKind,
    pub role_id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub when: Option<String>,
    #[serde(default)]
    pub prompt_template: Option<String>,
    #[serde(default)]
    pub worker_pool: Vec<String>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeSpec {
    pub from: String,
    pub to: String,
    #[serde(default = "default_edge_priority")]
    pub priority: u16,
    #[serde(default)]
    pub when: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PipelineDefaults {
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u16,
    #[serde(default = "default_max_workers_per_tick")]
    pub max_workers_per_tick: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RolePipelineSpec {
    pub api_version: String,
    pub pipeline_id: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub defaults: PipelineDefaults,
    #[serde(default)]
    pub roles: Vec<RoleSpec>,
    #[serde(default)]
    pub stages: Vec<StageSpec>,
    #[serde(default)]
    pub edges: Vec<EdgeSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineRunInput {
    pub task_id: String,
    pub title: String,
    #[serde(default)]
    pub input: Option<String>,
    #[serde(default = "default_dispatch_kind")]
    pub dispatcher_kind: String,
    #[serde(default = "default_dispatch_reference")]
    pub dispatcher_ref: String,
    #[serde(default)]
    pub state: Map<String, Value>,
}

fn default_dispatch_kind() -> String {
    "provider".to_string()
}

fn default_dispatch_reference() -> String {
    "default".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveDispatchTarget {
    pub kind: String,
    pub reference: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageExecutionOutput {
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub state_patch: Map<String, Value>,
}

impl Default for StageExecutionOutput {
    fn default() -> Self {
        Self {
            output: None,
            state_patch: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageRunRecord {
    pub stage_id: String,
    pub stage_kind: String,
    pub role_id: String,
    #[serde(default)]
    pub selected_worker: Option<String>,
    pub dispatch_kind: String,
    pub dispatch_ref: String,
    pub duration_ms: u64,
    pub ok: bool,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineRunResult {
    pub pipeline_id: String,
    pub status: String,
    #[serde(default)]
    pub final_output: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub state: Map<String, Value>,
    #[serde(default)]
    pub stages: Vec<StageRunRecord>,
}

pub struct StageExecutionContext<'a> {
    pub spec: &'a RolePipelineSpec,
    pub stage: &'a StageSpec,
    pub role: &'a RoleSpec,
    pub input: &'a PipelineRunInput,
    pub selected_worker: Option<&'a str>,
    pub prompt: String,
    pub dispatch_target: EffectiveDispatchTarget,
    pub state: &'a Map<String, Value>,
}

#[async_trait]
pub trait RoleStageExecutor: Send + Sync {
    async fn execute_stage(
        &self,
        ctx: StageExecutionContext<'_>,
    ) -> Result<StageExecutionOutput, String>;
}

pub struct RolePipelineRunner {
    spec: RolePipelineSpec,
}

impl RolePipelineRunner {
    pub fn new(spec: RolePipelineSpec) -> Self {
        Self { spec }
    }

    pub fn spec(&self) -> &RolePipelineSpec {
        &self.spec
    }

    pub fn lint(&self) -> Vec<LintIssue> {
        lint_spec(&self.spec)
    }

    pub fn from_yaml_str(raw: &str) -> Result<Self, String> {
        let spec: RolePipelineSpec =
            serde_yaml::from_str(raw).map_err(|e| format!("invalid role pipeline yaml: {e}"))?;
        Ok(Self::new(spec))
    }

    pub fn from_yaml_path(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        Self::from_yaml_str(&content)
    }

    pub async fn run<E: RoleStageExecutor>(
        &self,
        input: PipelineRunInput,
        executor: &E,
    ) -> Result<PipelineRunResult, String> {
        let lint_issues = lint_spec(&self.spec);
        let errors: Vec<&LintIssue> = lint_issues
            .iter()
            .filter(|issue| issue.severity == LintSeverity::Error)
            .collect();
        if !errors.is_empty() {
            let message = errors
                .into_iter()
                .map(|issue| format!("{}: {}", issue.code, issue.message))
                .collect::<Vec<_>>()
                .join(" | ");
            return Err(format!("role pipeline lint failed: {message}"));
        }

        let role_by_id: HashMap<&str, &RoleSpec> = self
            .spec
            .roles
            .iter()
            .map(|role| (role.id.as_str(), role))
            .collect();
        let stage_by_id: HashMap<&str, &StageSpec> = self
            .spec
            .stages
            .iter()
            .map(|stage| (stage.id.as_str(), stage))
            .collect();

        let start_stage = self
            .spec
            .stages
            .iter()
            .find(|stage| stage.enabled && stage.kind == StageKind::Start)
            .ok_or_else(|| "role pipeline has no enabled start stage".to_string())?;

        let mut run_state = input.state.clone();
        run_state.insert("task_id".to_string(), Value::String(input.task_id.clone()));
        run_state.insert("task_title".to_string(), Value::String(input.title.clone()));
        if let Some(user_input) = input.input.clone() {
            run_state.insert("task_input".to_string(), Value::String(user_input));
        }

        let mut current_stage_id = start_stage.id.clone();
        let mut selected_workers: Vec<String> = Vec::new();
        let mut records: Vec<StageRunRecord> = Vec::new();
        let max_iterations = self.spec.defaults.max_iterations.max(1);

        for _ in 0..max_iterations {
            let stage = stage_by_id
                .get(current_stage_id.as_str())
                .copied()
                .ok_or_else(|| format!("stage not found: {}", current_stage_id))?;

            if !stage.enabled {
                if let Some(next) = select_next_stage(&self.spec, stage, &run_state) {
                    current_stage_id = next;
                    continue;
                }
                return Ok(finalize_result(
                    &self.spec.pipeline_id,
                    "done",
                    records,
                    run_state,
                    None,
                ));
            }

            if stage.kind == StageKind::SchedulerDispatch {
                selected_workers = resolve_worker_pool(
                    stage,
                    &role_by_id,
                    usize::from(self.spec.defaults.max_workers_per_tick.max(1)),
                );
                run_state.insert(
                    "selected_workers".to_string(),
                    Value::Array(
                        selected_workers
                            .iter()
                            .map(|id| Value::String(id.clone()))
                            .collect(),
                    ),
                );
                run_state.insert(
                    "selected_worker_count".to_string(),
                    Value::Number(Number::from(selected_workers.len() as u64)),
                );
            }

            let role_ids = resolve_stage_role_ids(stage, &selected_workers);
            if role_ids.is_empty() {
                records.push(StageRunRecord {
                    stage_id: stage.id.clone(),
                    stage_kind: stage.kind.as_str().to_string(),
                    role_id: stage.role_id.clone(),
                    selected_worker: None,
                    dispatch_kind: "inherit".to_string(),
                    dispatch_ref: "default".to_string(),
                    duration_ms: 0,
                    ok: true,
                    output: Some("no eligible workers selected".to_string()),
                    error: None,
                });
            }

            for role_id in role_ids {
                let role = role_by_id
                    .get(role_id.as_str())
                    .copied()
                    .ok_or_else(|| format!("role not found: {role_id}"))?;

                if !role.enabled {
                    continue;
                }

                let selected_worker = if stage.role_id == SELECTED_WORKER_ROLE_TOKEN {
                    Some(role.id.as_str())
                } else {
                    None
                };

                let dispatch_target = resolve_dispatch_target(role, &input);
                let prompt = render_stage_prompt(stage, role, &input, selected_worker);
                let started = Instant::now();
                let execution = executor
                    .execute_stage(StageExecutionContext {
                        spec: &self.spec,
                        stage,
                        role,
                        input: &input,
                        selected_worker,
                        prompt,
                        dispatch_target: dispatch_target.clone(),
                        state: &run_state,
                    })
                    .await;
                let elapsed_ms = started.elapsed().as_millis() as u64;

                match execution {
                    Ok(output) => {
                        merge_state_patch(&mut run_state, output.state_patch);
                        if let Some(text) = output.output.as_ref() {
                            run_state
                                .insert("last_output".to_string(), Value::String(text.clone()));
                        }
                        records.push(StageRunRecord {
                            stage_id: stage.id.clone(),
                            stage_kind: stage.kind.as_str().to_string(),
                            role_id: role.id.clone(),
                            selected_worker: selected_worker.map(str::to_string),
                            dispatch_kind: dispatch_target.kind.clone(),
                            dispatch_ref: dispatch_target.reference.clone(),
                            duration_ms: elapsed_ms,
                            ok: true,
                            output: output.output,
                            error: None,
                        });
                    }
                    Err(err) => {
                        records.push(StageRunRecord {
                            stage_id: stage.id.clone(),
                            stage_kind: stage.kind.as_str().to_string(),
                            role_id: role.id.clone(),
                            selected_worker: selected_worker.map(str::to_string),
                            dispatch_kind: dispatch_target.kind.clone(),
                            dispatch_ref: dispatch_target.reference.clone(),
                            duration_ms: elapsed_ms,
                            ok: false,
                            output: None,
                            error: Some(err.clone()),
                        });
                        return Ok(finalize_result(
                            &self.spec.pipeline_id,
                            "error",
                            records,
                            run_state,
                            Some(err),
                        ));
                    }
                }
            }

            if stage.kind == StageKind::End {
                return Ok(finalize_result(
                    &self.spec.pipeline_id,
                    "done",
                    records,
                    run_state,
                    None,
                ));
            }

            match select_next_stage(&self.spec, stage, &run_state) {
                Some(next_stage_id) => {
                    current_stage_id = next_stage_id;
                }
                None => {
                    return Ok(finalize_result(
                        &self.spec.pipeline_id,
                        "done",
                        records,
                        run_state,
                        None,
                    ));
                }
            }
        }

        Ok(finalize_result(
            &self.spec.pipeline_id,
            "error",
            records,
            run_state,
            Some("role pipeline exceeded max_iterations".to_string()),
        ))
    }
}

fn finalize_result(
    pipeline_id: &str,
    status: &str,
    stages: Vec<StageRunRecord>,
    state: Map<String, Value>,
    error: Option<String>,
) -> PipelineRunResult {
    let final_output = stages.iter().rev().find_map(|stage| stage.output.clone());
    PipelineRunResult {
        pipeline_id: pipeline_id.to_string(),
        status: status.to_string(),
        final_output,
        error,
        state,
        stages,
    }
}

fn merge_state_patch(state: &mut Map<String, Value>, patch: Map<String, Value>) {
    for (key, value) in patch {
        state.insert(key, value);
    }
}

fn resolve_stage_role_ids(stage: &StageSpec, selected_workers: &[String]) -> Vec<String> {
    if stage.role_id == SELECTED_WORKER_ROLE_TOKEN {
        return selected_workers.to_vec();
    }
    vec![stage.role_id.clone()]
}

fn resolve_worker_pool(
    stage: &StageSpec,
    role_by_id: &HashMap<&str, &RoleSpec>,
    max_workers: usize,
) -> Vec<String> {
    let mut workers = if stage.worker_pool.is_empty() {
        role_by_id
            .values()
            .filter(|role| role.enabled && role.kind == RoleKind::Worker)
            .map(|role| role.id.clone())
            .collect::<Vec<_>>()
    } else {
        stage
            .worker_pool
            .iter()
            .filter_map(|worker_id| {
                let role = role_by_id.get(worker_id.as_str())?;
                if role.enabled && role.kind == RoleKind::Worker {
                    Some((*role).id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };

    workers.sort();
    workers.truncate(max_workers.max(1));
    workers
}

fn resolve_dispatch_target(role: &RoleSpec, input: &PipelineRunInput) -> EffectiveDispatchTarget {
    match role.dispatch_target.kind {
        DispatchKind::Inherit => EffectiveDispatchTarget {
            kind: input.dispatcher_kind.clone(),
            reference: input.dispatcher_ref.clone(),
        },
        DispatchKind::Provider => EffectiveDispatchTarget {
            kind: "provider".to_string(),
            reference: role
                .dispatch_target
                .reference
                .clone()
                .unwrap_or_else(|| "default".to_string()),
        },
        DispatchKind::LocalClient => EffectiveDispatchTarget {
            kind: "local_client".to_string(),
            reference: role
                .dispatch_target
                .reference
                .clone()
                .unwrap_or_else(|| "codex".to_string()),
        },
        DispatchKind::Custom => EffectiveDispatchTarget {
            kind: "custom".to_string(),
            reference: role
                .dispatch_target
                .reference
                .clone()
                .unwrap_or_else(|| "custom".to_string()),
        },
    }
}

fn render_stage_prompt(
    stage: &StageSpec,
    role: &RoleSpec,
    input: &PipelineRunInput,
    selected_worker: Option<&str>,
) -> String {
    if let Some(template) = stage.prompt_template.as_ref() {
        let mut prompt = template.clone();
        prompt = prompt.replace("{{task.title}}", input.title.trim());
        prompt = prompt.replace("{{task.input}}", input.input.as_deref().unwrap_or(""));
        prompt = prompt.replace("{{stage.id}}", &stage.id);
        prompt = prompt.replace("{{stage.kind}}", stage.kind.as_str());
        prompt = prompt.replace("{{role.id}}", &role.id);
        prompt = prompt.replace("{{role.name}}", &role.display_name);
        prompt = prompt.replace("{{worker.id}}", selected_worker.unwrap_or(""));
        return prompt;
    }

    let mut lines = vec![
        format!("Task: {}", input.title.trim()),
        format!("Stage: {} ({})", stage.id, stage.kind.as_str()),
        format!("Role: {} ({})", role.display_name, role.id),
    ];
    if let Some(worker_id) = selected_worker {
        lines.push(format!("Selected Worker: {worker_id}"));
    }
    if let Some(user_input) = input.input.as_ref() {
        let trimmed = user_input.trim();
        if !trimmed.is_empty() {
            lines.push("Prompt:".to_string());
            lines.push(trimmed.to_string());
        }
    }
    lines.join("\n")
}

fn select_next_stage(
    spec: &RolePipelineSpec,
    stage: &StageSpec,
    state: &Map<String, Value>,
) -> Option<String> {
    let mut edges = spec
        .edges
        .iter()
        .filter(|edge| edge.from == stage.id)
        .collect::<Vec<_>>();
    if edges.is_empty() {
        return None;
    }
    edges.sort_by_key(|edge| std::cmp::Reverse(edge.priority));
    edges
        .into_iter()
        .find(|edge| eval_when_clause(edge.when.as_deref(), state))
        .map(|edge| edge.to.clone())
}

fn eval_when_clause(when: Option<&str>, state: &Map<String, Value>) -> bool {
    let Some(raw) = when else {
        return true;
    };
    let expr = raw.trim();
    if expr.is_empty() || expr.eq_ignore_ascii_case("always") {
        return true;
    }

    if let Some((lhs, rhs)) = expr.split_once("==") {
        let key = lhs
            .trim()
            .strip_prefix("state.")
            .unwrap_or(lhs.trim())
            .trim();
        let expected = rhs.trim();
        let Some(actual) = state.get(key) else {
            return false;
        };
        if expected.eq_ignore_ascii_case("true") || expected.eq_ignore_ascii_case("false") {
            return actual.as_bool() == Some(expected.eq_ignore_ascii_case("true"));
        }
        if let Ok(number) = expected.parse::<i64>() {
            return actual.as_i64() == Some(number);
        }
        let expected = expected.trim_matches('"').trim_matches('\'');
        return actual.as_str() == Some(expected);
    }

    if let Some(key) = expr.strip_prefix("state.") {
        return state
            .get(key.trim())
            .and_then(Value::as_bool)
            .unwrap_or(false);
    }

    false
}

pub fn lint_spec(spec: &RolePipelineSpec) -> Vec<LintIssue> {
    let mut issues = Vec::new();

    if spec.api_version.trim() != ROLE_PIPELINE_API_VERSION {
        issues.push(LintIssue {
            severity: LintSeverity::Error,
            code: "RP001".to_string(),
            message: format!(
                "unsupported api_version: {} (expected {})",
                spec.api_version, ROLE_PIPELINE_API_VERSION
            ),
        });
    }

    if spec.pipeline_id.trim().is_empty() {
        issues.push(LintIssue {
            severity: LintSeverity::Error,
            code: "RP002".to_string(),
            message: "pipeline_id must not be empty".to_string(),
        });
    }

    if spec.roles.is_empty() {
        issues.push(LintIssue {
            severity: LintSeverity::Error,
            code: "RP003".to_string(),
            message: "roles must not be empty".to_string(),
        });
    }

    let mut role_ids = HashSet::new();
    let mut scheduler_count = 0;
    let mut worker_count = 0;
    for role in &spec.roles {
        if role.id.trim().is_empty() {
            issues.push(LintIssue {
                severity: LintSeverity::Error,
                code: "RP004".to_string(),
                message: "role id must not be empty".to_string(),
            });
            continue;
        }
        if !role_ids.insert(role.id.clone()) {
            issues.push(LintIssue {
                severity: LintSeverity::Error,
                code: "RP005".to_string(),
                message: format!("duplicate role id: {}", role.id),
            });
        }
        if role.kind == RoleKind::Scheduler && role.enabled {
            scheduler_count += 1;
        }
        if role.kind == RoleKind::Worker && role.enabled {
            worker_count += 1;
        }
    }
    if scheduler_count == 0 {
        issues.push(LintIssue {
            severity: LintSeverity::Error,
            code: "RP006".to_string(),
            message: "at least one enabled scheduler role is required".to_string(),
        });
    }
    if worker_count == 0 {
        issues.push(LintIssue {
            severity: LintSeverity::Warning,
            code: "RP007".to_string(),
            message: "no enabled worker role found".to_string(),
        });
    }

    let mut stage_ids = HashSet::new();
    let mut start_count = 0;
    let mut end_count = 0;
    for stage in &spec.stages {
        if stage.id.trim().is_empty() {
            issues.push(LintIssue {
                severity: LintSeverity::Error,
                code: "RP008".to_string(),
                message: "stage id must not be empty".to_string(),
            });
            continue;
        }
        if !stage_ids.insert(stage.id.clone()) {
            issues.push(LintIssue {
                severity: LintSeverity::Error,
                code: "RP009".to_string(),
                message: format!("duplicate stage id: {}", stage.id),
            });
        }
        if stage.kind == StageKind::Start && stage.enabled {
            start_count += 1;
        }
        if stage.kind == StageKind::End && stage.enabled {
            end_count += 1;
        }
        if stage.role_id != SELECTED_WORKER_ROLE_TOKEN && !role_ids.contains(&stage.role_id) {
            issues.push(LintIssue {
                severity: LintSeverity::Error,
                code: "RP010".to_string(),
                message: format!(
                    "stage {} references unknown role {}",
                    stage.id, stage.role_id
                ),
            });
        }
        if stage.kind == StageKind::SchedulerDispatch && stage.worker_pool.is_empty() {
            issues.push(LintIssue {
                severity: LintSeverity::Warning,
                code: "RP011".to_string(),
                message: format!("scheduler stage {} has empty worker_pool", stage.id),
            });
        }
    }

    if start_count != 1 {
        issues.push(LintIssue {
            severity: LintSeverity::Error,
            code: "RP012".to_string(),
            message: format!("expected exactly one enabled start stage, found {start_count}"),
        });
    }
    if end_count == 0 {
        issues.push(LintIssue {
            severity: LintSeverity::Error,
            code: "RP013".to_string(),
            message: "expected at least one enabled end stage".to_string(),
        });
    }

    for stage in &spec.stages {
        if stage.kind != StageKind::SchedulerDispatch {
            continue;
        }
        for worker_id in &stage.worker_pool {
            match spec.roles.iter().find(|role| role.id == *worker_id) {
                Some(role) if role.kind == RoleKind::Worker => {}
                Some(_) => issues.push(LintIssue {
                    severity: LintSeverity::Error,
                    code: "RP014".to_string(),
                    message: format!(
                        "scheduler stage {} references non-worker role {} in worker_pool",
                        stage.id, worker_id
                    ),
                }),
                None => issues.push(LintIssue {
                    severity: LintSeverity::Error,
                    code: "RP015".to_string(),
                    message: format!(
                        "scheduler stage {} references unknown worker {}",
                        stage.id, worker_id
                    ),
                }),
            }
        }
    }

    for edge in &spec.edges {
        if !stage_ids.contains(&edge.from) {
            issues.push(LintIssue {
                severity: LintSeverity::Error,
                code: "RP016".to_string(),
                message: format!("edge.from references unknown stage: {}", edge.from),
            });
        }
        if !stage_ids.contains(&edge.to) {
            issues.push(LintIssue {
                severity: LintSeverity::Error,
                code: "RP017".to_string(),
                message: format!("edge.to references unknown stage: {}", edge.to),
            });
        }
    }

    issues
}

pub fn load_spec_from_yaml_path(path: &Path) -> Result<RolePipelineSpec, String> {
    RolePipelineRunner::from_yaml_path(path).map(|runner| runner.spec)
}

pub fn default_pipeline_template() -> &'static str {
    include_str!("../templates/pipeline.role.default.yaml")
}

pub fn ensure_default_pipeline_file(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, default_pipeline_template())
        .map_err(|e| format!("failed to write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    struct TestExecutor {
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl RoleStageExecutor for TestExecutor {
        async fn execute_stage(
            &self,
            ctx: StageExecutionContext<'_>,
        ) -> Result<StageExecutionOutput, String> {
            self.calls
                .lock()
                .expect("calls mutex")
                .push(format!("{}:{}", ctx.stage.id, ctx.role.id));
            let mut patch = Map::new();
            patch.insert(format!("stage_{}_ok", ctx.stage.id), Value::Bool(true));
            Ok(StageExecutionOutput {
                output: Some(format!("ok:{}:{}", ctx.stage.id, ctx.role.id)),
                state_patch: patch,
            })
        }
    }

    #[test]
    fn default_template_parses() {
        let runner = RolePipelineRunner::from_yaml_str(default_pipeline_template())
            .expect("default template should parse");
        let issues = runner.lint();
        let errors = issues
            .into_iter()
            .filter(|issue| issue.severity == LintSeverity::Error)
            .collect::<Vec<_>>();
        assert!(
            errors.is_empty(),
            "default template lint errors: {errors:?}"
        );
    }

    #[tokio::test]
    async fn runner_executes_scheduler_and_worker_stages() {
        let runner = RolePipelineRunner::from_yaml_str(default_pipeline_template())
            .expect("default template should parse");
        let executor = TestExecutor {
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let result = runner
            .run(
                PipelineRunInput {
                    task_id: "t1".to_string(),
                    title: "demo".to_string(),
                    input: Some("hello".to_string()),
                    dispatcher_kind: "provider".to_string(),
                    dispatcher_ref: "default".to_string(),
                    state: Map::new(),
                },
                &executor,
            )
            .await
            .expect("pipeline run should succeed");

        assert_eq!(result.status, "done");
        let calls = executor.calls.lock().expect("calls mutex");
        assert!(calls.iter().any(|line| line.contains("scheduler")));
        assert!(calls.iter().any(|line| line.contains("worker.default")));
    }
}
