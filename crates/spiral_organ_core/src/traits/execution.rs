use crate::domain::{
    ModelRequest, ModelResponse, RiskLevel, TargetSession, ToolOutput, ToolRequest,
};

use super::CoreResult;

pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn complete(&self, request: ModelRequest) -> CoreResult<ModelResponse>;
    fn supports_vision(&self) -> bool {
        false
    }
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn execute(&self, request: ToolRequest) -> CoreResult<ToolOutput>;
    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }
}

pub trait SessionEngine: Send + Sync {
    fn run_turn(&self, session: &TargetSession, input: &str) -> CoreResult<String>;
}

pub trait RuntimeAdapter: Send + Sync {
    fn kind(&self) -> &str;
    fn run_command(&self, command: &str) -> CoreResult<String>;
}
