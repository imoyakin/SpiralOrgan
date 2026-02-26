use crate::domain::{AuditEvent, PipelineSpec, TargetSession, TaskCard};

use super::CoreResult;

pub trait Planner: Send + Sync {
    fn build_tasks(&self, session: &TargetSession) -> CoreResult<Vec<TaskCard>>;
}

pub trait Worker: Send + Sync {
    fn execute_task(&self, task: &TaskCard) -> CoreResult<AuditEvent>;
}

pub trait Supervisor: Send + Sync {
    fn verify_task(&self, task: &TaskCard) -> CoreResult<bool>;
}

pub trait PipelineEngine: Send + Sync {
    fn lint(&self, spec: &PipelineSpec) -> CoreResult<()>;
    fn execute(&self, spec: &PipelineSpec, session: &TargetSession) -> CoreResult<()>;
}

pub trait Orchestrator: Send + Sync {
    fn tick(&self, session: &TargetSession) -> CoreResult<()>;
}
