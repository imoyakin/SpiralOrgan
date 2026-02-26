use crate::domain::{ModelRequest, TargetSession};
use crate::traits::{CoreResult, MemoryStore, Provider, SessionEngine, Tool};

pub struct ExecutionService<P, M> {
    provider: P,
    memory: M,
}

impl<P, M> ExecutionService<P, M> {
    pub fn new(provider: P, memory: M) -> Self {
        Self { provider, memory }
    }

    pub fn call_tool<T: Tool>(&self, tool: &T, task_id: &str, payload: &str) -> CoreResult<String> {
        let result = tool.execute(crate::domain::ToolRequest {
            task_id: task_id.to_string(),
            payload: payload.to_string(),
        })?;
        Ok(result.output)
    }
}

impl<P, M> SessionEngine for ExecutionService<P, M>
where
    P: Provider + Send + Sync,
    M: MemoryStore + Send + Sync,
{
    fn run_turn(&self, session: &TargetSession, input: &str) -> CoreResult<String> {
        let recall = self.memory.search(input, 3)?;
        let prompt = if recall.is_empty() {
            format!("session:{}\n{}", session.id, input)
        } else {
            format!(
                "session:{}\ncontext:{}\n{}",
                session.id,
                recall
                    .iter()
                    .map(|m| m.content.clone())
                    .collect::<Vec<_>>()
                    .join("\n---\n"),
                input
            )
        };
        let response = self.provider.complete(ModelRequest { prompt })?;
        Ok(response.output)
    }
}
