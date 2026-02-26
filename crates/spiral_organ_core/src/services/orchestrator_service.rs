use crate::domain::{RiskLevel, TargetSession};
use crate::traits::{
    CoreResult, Orchestrator, PipelineEngine, Planner, PolicyEngine, Supervisor, Worker,
};

pub struct ResearchAlignedOrchestrator<P, W, S, PE, PO> {
    planner: P,
    worker: W,
    supervisor: S,
    pipeline: PE,
    policy: PO,
}

impl<P, W, S, PE, PO> ResearchAlignedOrchestrator<P, W, S, PE, PO> {
    pub fn new(planner: P, worker: W, supervisor: S, pipeline: PE, policy: PO) -> Self {
        Self {
            planner,
            worker,
            supervisor,
            pipeline,
            policy,
        }
    }
}

impl<P, W, S, PE, PO> Orchestrator for ResearchAlignedOrchestrator<P, W, S, PE, PO>
where
    P: Planner + Send + Sync,
    W: Worker + Send + Sync,
    S: Supervisor + Send + Sync,
    PE: PipelineEngine + Send + Sync,
    PO: PolicyEngine + Send + Sync,
{
    fn tick(&self, session: &TargetSession) -> CoreResult<()> {
        // Maps to .research three-layer synthesis:
        // 1) orchestration entry (planner)
        // 2) execution layer (worker)
        // 3) recovery/policy layer (supervisor + policy + pipeline lint/execute)
        let tasks = self.planner.build_tasks(session)?;
        for task in tasks {
            if self.policy.needs_approval(&task, RiskLevel::High)? {
                continue;
            }
            let _event = self.worker.execute_task(&task)?;
            let _ok = self.supervisor.verify_task(&task)?;
        }
        // A real run would pass a session-selected pipeline spec here.
        // For scaffold stage, pipeline execution is deferred to dedicated entrypoints.
        let _ = &self.pipeline;
        Ok(())
    }
}
