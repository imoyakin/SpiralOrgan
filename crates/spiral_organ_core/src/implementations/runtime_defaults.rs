use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::{
    AuditEvent, ExecutionMode, GhostProfile, HookSpec, NodeKind, NodeSpec, PipelineDefaults,
    PipelineSpec, Role, SessionStatus, TargetSession, TaskCard,
};
use crate::error::CoreError;
use crate::traits::{ChannelAdapter, CoreResult, GhostRegistry, Planner, Supervisor, Worker};

pub struct DemoPlanner;

impl Planner for DemoPlanner {
    fn build_tasks(&self, session: &TargetSession) -> CoreResult<Vec<TaskCard>> {
        let mut plan = TaskCard::new(
            format!("{}-plan", session.id),
            format!("Plan target: {}", session.target),
            Role::Designer,
        );
        plan.acceptance_criteria = "Target decomposed into actionable work items.".to_string();

        let mut execute = TaskCard::new(
            format!("{}-exec", session.id),
            "Execute implementation task".to_string(),
            Role::Worker,
        );
        execute.depends_on.push(plan.id.clone());
        execute.acceptance_criteria = "Core flow runs and emits audit events.".to_string();

        let mut verify = TaskCard::new(
            format!("{}-verify", session.id),
            "Verify output".to_string(),
            Role::Supervisor,
        );
        verify.depends_on.push(execute.id.clone());
        verify.acceptance_criteria = "Verification result is true.".to_string();

        Ok(vec![plan, execute, verify])
    }
}

pub struct DemoWorker;

impl Worker for DemoWorker {
    fn execute_task(&self, task: &TaskCard) -> CoreResult<AuditEvent> {
        Ok(AuditEvent {
            id: format!("evt-{}", task.id),
            event_type: "task.executed".to_string(),
            session_id: "demo-session".to_string(),
            task_id: Some(task.id.clone()),
            timestamp_ms: now_ms(),
            payload: format!("{{\"task\":\"{}\",\"status\":\"ok\"}}", task.title),
        })
    }
}

pub struct DemoSupervisor;

impl Supervisor for DemoSupervisor {
    fn verify_task(&self, _task: &TaskCard) -> CoreResult<bool> {
        Ok(true)
    }
}

pub struct StdoutChannelAdapter;

impl ChannelAdapter for StdoutChannelAdapter {
    fn channel_id(&self) -> &str {
        "stdout"
    }

    fn send(&self, to: &str, content: &str) -> CoreResult<()> {
        println!("[channel:{} -> {}] {}", self.channel_id(), to, content);
        Ok(())
    }
}

pub struct DemoGhostRegistry {
    ghosts: Mutex<HashMap<String, GhostProfile>>,
}

impl Default for DemoGhostRegistry {
    fn default() -> Self {
        let mut ghosts = HashMap::new();
        ghosts.insert(
            "default-worker".to_string(),
            GhostProfile {
                id: "default-worker".to_string(),
                name: "Default Worker Ghost".to_string(),
                role: Role::Worker,
                ghost_path: ".ghost/default-worker.md".to_string(),
                memory_namespace: "ghost.default.worker".to_string(),
            },
        );
        Self {
            ghosts: Mutex::new(ghosts),
        }
    }
}

impl GhostRegistry for DemoGhostRegistry {
    fn load(&self, ghost_id: &str) -> CoreResult<GhostProfile> {
        let guard = self
            .ghosts
            .lock()
            .map_err(|_| CoreError::Internal("ghost registry mutex poisoned".to_string()))?;
        guard
            .get(ghost_id)
            .cloned()
            .ok_or_else(|| CoreError::NotFound(format!("ghost not found: {ghost_id}")))
    }
}

pub fn default_demo_pipeline() -> PipelineSpec {
    PipelineSpec {
        api_version: "spiralorgan.pipeline/v0.1".to_string(),
        pipeline_id: "demo-default".to_string(),
        defaults: PipelineDefaults::default(),
        nodes: vec![
            NodeSpec {
                id: "start_01".to_string(),
                kind: NodeKind::Start,
                enabled: true,
                role_scope: vec![Role::Owner],
                execution_mode: ExecutionMode::Sequential,
                approval_required: false,
                allowed_tools: vec![],
            },
            NodeSpec {
                id: "plan_01".to_string(),
                kind: NodeKind::Plan,
                enabled: true,
                role_scope: vec![Role::Designer],
                execution_mode: ExecutionMode::Sequential,
                approval_required: false,
                allowed_tools: vec!["memory_recall".to_string(), "kanban_update".to_string()],
            },
            NodeSpec {
                id: "execute_01".to_string(),
                kind: NodeKind::Execute,
                enabled: true,
                role_scope: vec![Role::Worker],
                execution_mode: ExecutionMode::Parallel,
                approval_required: true,
                allowed_tools: vec![
                    "shell_exec".to_string(),
                    "file_read".to_string(),
                    "file_write".to_string(),
                ],
            },
            NodeSpec {
                id: "verify_01".to_string(),
                kind: NodeKind::Verify,
                enabled: true,
                role_scope: vec![Role::Supervisor],
                execution_mode: ExecutionMode::Sequential,
                approval_required: false,
                allowed_tools: vec!["test_run".to_string()],
            },
            NodeSpec {
                id: "end_01".to_string(),
                kind: NodeKind::End,
                enabled: true,
                role_scope: vec![Role::Owner],
                execution_mode: ExecutionMode::Sequential,
                approval_required: false,
                allowed_tools: vec![],
            },
        ],
        edges: vec![
            crate::domain::EdgeSpec {
                from: "start_01".to_string(),
                to: "plan_01".to_string(),
                priority: 100,
                when: None,
            },
            crate::domain::EdgeSpec {
                from: "plan_01".to_string(),
                to: "execute_01".to_string(),
                priority: 100,
                when: None,
            },
            crate::domain::EdgeSpec {
                from: "execute_01".to_string(),
                to: "verify_01".to_string(),
                priority: 100,
                when: None,
            },
            crate::domain::EdgeSpec {
                from: "verify_01".to_string(),
                to: "end_01".to_string(),
                priority: 100,
                when: None,
            },
        ],
        hooks: vec![HookSpec {
            id: "audit_after_execute".to_string(),
            phase: crate::domain::HookPhase::AfterNode,
            target: "execute_01".to_string(),
            action: "audit.flush".to_string(),
            enabled: true,
        }],
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[allow(dead_code)]
fn _demo_session() -> TargetSession {
    TargetSession {
        id: "demo-session".to_string(),
        project_id: "demo-project".to_string(),
        target: "run default pipeline".to_string(),
        status: SessionStatus::Running,
    }
}
