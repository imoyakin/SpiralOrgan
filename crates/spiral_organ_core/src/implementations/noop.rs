use crate::domain::{
    ModelRequest, ModelResponse, PipelineSpec, RiskLevel, TargetSession, TaskCard,
};
use crate::error::CoreError;
use crate::traits::{CoreResult, NotificationSink, PipelineEngine, PolicyEngine, Provider, Tool};
use crate::validation::{LintSeverity, lint_pipeline};

pub struct NoopProvider {
    name: String,
}

impl NoopProvider {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Provider for NoopProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn complete(&self, request: ModelRequest) -> CoreResult<ModelResponse> {
        Ok(ModelResponse {
            output: format!("noop-response: {}", request.prompt),
            tool_calls: Vec::new(),
        })
    }
}

pub struct NoopTool {
    name: String,
}

impl NoopTool {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Tool for NoopTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn execute(
        &self,
        _request: crate::domain::ToolRequest,
    ) -> CoreResult<crate::domain::ToolOutput> {
        Ok(crate::domain::ToolOutput {
            success: true,
            output: "noop-tool-output".to_string(),
        })
    }
}

pub struct AllowAllPolicyEngine;

impl PolicyEngine for AllowAllPolicyEngine {
    fn needs_approval(&self, _task: &TaskCard, risk: RiskLevel) -> CoreResult<bool> {
        Ok(matches!(risk, RiskLevel::High | RiskLevel::Critical))
    }

    fn allow_tool(&self, _tool_name: &str, risk: RiskLevel) -> CoreResult<bool> {
        Ok(!matches!(risk, RiskLevel::Critical))
    }
}

pub struct NoopPipelineEngine;

impl PipelineEngine for NoopPipelineEngine {
    fn lint(&self, spec: &PipelineSpec) -> CoreResult<()> {
        let issues = lint_pipeline(spec);
        let errors: Vec<_> = issues
            .into_iter()
            .filter(|issue| issue.severity == LintSeverity::Error)
            .collect();
        if !errors.is_empty() {
            let message = errors
                .iter()
                .map(|e| format!("{}: {}", e.code, e.message))
                .collect::<Vec<_>>()
                .join(" | ");
            return Err(CoreError::InvalidConfig(message));
        }
        Ok(())
    }

    fn execute(&self, spec: &PipelineSpec, _session: &TargetSession) -> CoreResult<()> {
        self.lint(spec)
    }
}

pub struct NoopNotificationSink;

impl NotificationSink for NoopNotificationSink {
    fn notify_done(&self, _session_id: &str, _summary: &str) -> CoreResult<()> {
        Ok(())
    }

    fn notify_idle_waiting(&self, _session_id: &str) -> CoreResult<()> {
        Ok(())
    }
}
