pub mod domain;
pub mod error;
pub mod ffi;
pub mod implementations;
pub mod local_client;
pub mod runtime_config;
pub mod runtime_provider;
pub mod server;
pub mod services;
pub mod traits;
pub mod validation;

pub use crate::domain::*;
pub use crate::error::CoreError;
pub use crate::traits::*;

#[cfg(test)]
mod tests {
    use crate::domain::{
        ChangeFile, ChangeStatus, ChangeView, MemoryEntry, PipelineSpec, RiskLevel, Role, TaskCard,
    };
    use crate::implementations::{
        AllowAllPolicyEngine, InMemoryChangeStore, InMemoryMemoryStore, NoopPipelineEngine,
        NoopProvider, NoopTool,
    };
    use crate::services::{ExecutionService, ResearchAlignedOrchestrator};
    use crate::traits::{
        ChangeStore, MemoryStore, PipelineEngine, PolicyEngine, Provider, SessionEngine, Tool,
    };
    use crate::traits::{Orchestrator, Planner, Supervisor, Worker};

    #[test]
    fn in_memory_memory_store_roundtrip() {
        let store = InMemoryMemoryStore::default();
        let memory = MemoryEntry {
            id: "m1".to_string(),
            title: "Build flow".to_string(),
            tags: vec!["build".to_string()],
            score: 1.0,
            source: "unit-test".to_string(),
            content: "cargo build then test".to_string(),
        };
        store
            .save(memory.clone())
            .expect("memory save should succeed");
        let hits = store
            .search("build", 5)
            .expect("memory search should succeed");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0], memory);
    }

    #[test]
    fn policy_engine_marks_high_risk_as_approval_required() {
        let policy = AllowAllPolicyEngine;
        let task = TaskCard::new("task-1", "run shell", Role::Worker);
        assert!(
            policy
                .needs_approval(&task, RiskLevel::High)
                .expect("approval decision should succeed")
        );
    }

    #[test]
    fn noop_provider_and_tool_are_callable() {
        let provider = NoopProvider::new("noop");
        let resp = provider
            .complete(crate::domain::ModelRequest {
                prompt: "hello".to_string(),
            })
            .expect("provider should return response");
        assert!(!resp.output.is_empty());

        let tool = NoopTool::new("echo");
        let out = tool
            .execute(crate::domain::ToolRequest {
                task_id: "task-1".to_string(),
                payload: "{}".to_string(),
            })
            .expect("tool execution should return output");
        assert!(out.success);
    }

    #[test]
    fn change_store_supports_raw_patch_diff() {
        let store = InMemoryChangeStore::default();
        let file = ChangeFile {
            path: "src/lib.rs".to_string(),
            status: ChangeStatus::Modified,
            additions: 3,
            deletions: 1,
            task_id: "task-1".to_string(),
            agent_id: "worker-1".to_string(),
        };
        store.append(file).expect("append should succeed");
        let list = store.list("task-1").expect("list should succeed");
        assert_eq!(list.len(), 1);
        let _ = store
            .view("src/lib.rs", ChangeView::Diff)
            .expect("diff view should succeed");
    }

    #[test]
    fn pipeline_lint_rejects_invalid_shape() {
        let engine = NoopPipelineEngine;
        let invalid = PipelineSpec {
            api_version: "spiralorgan.pipeline/v0.1".to_string(),
            pipeline_id: "broken".to_string(),
            defaults: crate::domain::PipelineDefaults::default(),
            nodes: vec![],
            edges: vec![],
            hooks: vec![],
        };
        assert!(engine.lint(&invalid).is_err());
    }

    #[test]
    fn execution_service_runs_single_turn() {
        let provider = NoopProvider::new("noop");
        let memory = InMemoryMemoryStore::default();
        memory
            .save(MemoryEntry {
                id: "m1".to_string(),
                title: "greeting".to_string(),
                tags: vec![],
                score: 1.0,
                source: "test".to_string(),
                content: "hello context".to_string(),
            })
            .expect("memory save should succeed");
        let engine = ExecutionService::new(provider, memory);
        let output = engine
            .run_turn(
                &crate::domain::TargetSession {
                    id: "s1".to_string(),
                    project_id: "p1".to_string(),
                    target: "demo".to_string(),
                    status: crate::domain::SessionStatus::Running,
                },
                "hello",
            )
            .expect("run_turn should succeed");
        assert!(output.contains("noop-response"));
    }

    #[test]
    fn orchestrator_tick_runs_without_failure() {
        struct MockPlanner;
        impl Planner for MockPlanner {
            fn build_tasks(
                &self,
                _session: &crate::domain::TargetSession,
            ) -> crate::traits::CoreResult<Vec<TaskCard>> {
                Ok(vec![TaskCard::new("t1", "demo", Role::Worker)])
            }
        }

        struct MockWorker;
        impl Worker for MockWorker {
            fn execute_task(
                &self,
                task: &TaskCard,
            ) -> crate::traits::CoreResult<crate::domain::AuditEvent> {
                Ok(crate::domain::AuditEvent {
                    id: "a1".to_string(),
                    event_type: "task.executed".to_string(),
                    session_id: "s1".to_string(),
                    task_id: Some(task.id.clone()),
                    timestamp_ms: 0,
                    payload: "{}".to_string(),
                })
            }
        }

        struct MockSupervisor;
        impl Supervisor for MockSupervisor {
            fn verify_task(&self, _task: &TaskCard) -> crate::traits::CoreResult<bool> {
                Ok(true)
            }
        }

        let orch = ResearchAlignedOrchestrator::new(
            MockPlanner,
            MockWorker,
            MockSupervisor,
            NoopPipelineEngine,
            AllowAllPolicyEngine,
        );
        orch.tick(&crate::domain::TargetSession {
            id: "s1".to_string(),
            project_id: "p1".to_string(),
            target: "demo".to_string(),
            status: crate::domain::SessionStatus::Running,
        })
        .expect("orchestrator tick should succeed");
    }
}
