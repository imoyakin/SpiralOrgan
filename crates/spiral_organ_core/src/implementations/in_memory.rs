use std::sync::Mutex;

use crate::domain::{ChangeFile, ChangeView, MemoryEntry};
use crate::error::CoreError;
use crate::traits::{ChangeStore, CoreResult, MemoryStore};

#[derive(Default)]
pub struct InMemoryMemoryStore {
    data: Mutex<Vec<MemoryEntry>>,
}

impl MemoryStore for InMemoryMemoryStore {
    fn save(&self, entry: MemoryEntry) -> CoreResult<()> {
        let mut guard = self
            .data
            .lock()
            .map_err(|_| CoreError::Internal("memory store mutex poisoned".to_string()))?;
        guard.push(entry);
        Ok(())
    }

    fn search(&self, query: &str, top_k: usize) -> CoreResult<Vec<MemoryEntry>> {
        let guard = self
            .data
            .lock()
            .map_err(|_| CoreError::Internal("memory store mutex poisoned".to_string()))?;
        let mut hits: Vec<_> = guard
            .iter()
            .filter(|entry| entry.title.contains(query) || entry.content.contains(query))
            .cloned()
            .collect();
        hits.truncate(top_k);
        Ok(hits)
    }
}

#[derive(Default)]
pub struct InMemoryChangeStore {
    files: Mutex<Vec<ChangeFile>>,
}

impl ChangeStore for InMemoryChangeStore {
    fn append(&self, changed: ChangeFile) -> CoreResult<()> {
        let mut guard = self
            .files
            .lock()
            .map_err(|_| CoreError::Internal("change store mutex poisoned".to_string()))?;
        guard.push(changed);
        Ok(())
    }

    fn list(&self, task_id: &str) -> CoreResult<Vec<ChangeFile>> {
        let guard = self
            .files
            .lock()
            .map_err(|_| CoreError::Internal("change store mutex poisoned".to_string()))?;
        Ok(guard
            .iter()
            .filter(|f| f.task_id == task_id)
            .cloned()
            .collect())
    }

    fn view(&self, path: &str, view: ChangeView) -> CoreResult<String> {
        let kind = match view {
            ChangeView::Raw => "raw",
            ChangeView::Patch => "patch",
            ChangeView::Diff => "diff",
        };
        Ok(format!("mock-{kind}-view:{path}"))
    }
}
