use crate::domain::{RiskLevel, TaskCard};

use super::CoreResult;

pub trait PolicyEngine: Send + Sync {
    fn needs_approval(&self, task: &TaskCard, risk: RiskLevel) -> CoreResult<bool>;
    fn allow_tool(&self, tool_name: &str, risk: RiskLevel) -> CoreResult<bool>;
}
