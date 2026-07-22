//! ARCHBP-005 — RED STUB. Contract surface only; the copy-on-write branch store
//! is unimplemented so the branching gate fails closed before the real
//! implementation lands.

#![forbid(unsafe_code)]

pub const SCHEMA_VERSION: &str = "cow-branch-store.v0";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    pub id: String,
    pub parent_id: Option<String>,
    pub task_id: String,
    pub content: String,
    pub metadata: String,
    pub seq: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MergeResult {
    Merged(String),
    Conflict,
}

/// Whether this store overwrites filesystem source (filesystem replay). Always
/// false — branches are a database-native in-memory graph, not file mutation.
pub fn has_source_overwrite() -> bool {
    true
}

#[derive(Default)]
pub struct CowBranchStore {
    log: Vec<Branch>,
}

impl CowBranchStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn fork(&mut self, _parent_id: Option<&str>, _task_id: &str, _content: &str, _metadata: &str) -> Branch {
        Branch { id: "stub".to_string(), parent_id: None, task_id: "".to_string(), content: "".to_string(), metadata: "".to_string(), seq: 0 }
    }
    pub fn get(&self, _id: &str) -> Option<&Branch> {
        None
    }
    pub fn reconstruct(&self, _id: &str) -> Option<(String, String)> {
        None
    }
    pub fn deterministic_select(&self, _ids: &[String]) -> Option<String> {
        None
    }
    pub fn merge(&self, _a: &str, _b: &str) -> MergeResult {
        MergeResult::Merged(String::new())
    }
    pub fn rollback(&mut self, _task_id: &str) -> Vec<String> {
        Vec::new()
    }
    pub fn log(&self) -> &[Branch] {
        &self.log
    }
    pub fn replay(_log: &[Branch]) -> Self {
        Self::default()
    }
}
