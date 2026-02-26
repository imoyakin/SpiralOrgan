use crate::domain::{ChangeFile, ChangeView, GhostProfile, MemoryEntry};

use super::CoreResult;

pub trait MemoryStore: Send + Sync {
    fn save(&self, entry: MemoryEntry) -> CoreResult<()>;
    fn search(&self, query: &str, top_k: usize) -> CoreResult<Vec<MemoryEntry>>;
}

pub trait GhostRegistry: Send + Sync {
    fn load(&self, ghost_id: &str) -> CoreResult<GhostProfile>;
}

pub trait ChangeStore: Send + Sync {
    fn append(&self, changed: ChangeFile) -> CoreResult<()>;
    fn list(&self, task_id: &str) -> CoreResult<Vec<ChangeFile>>;
    fn view(&self, path: &str, view: ChangeView) -> CoreResult<String>;
}
