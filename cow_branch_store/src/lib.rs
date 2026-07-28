//! ARCHBP-005 — database-native copy-on-write semantic code branches and
//! identity-bound rollback.
//!
//! Branches form an in-memory graph over an append-only fork log — NOT a
//! filesystem replay (has_source_overwrite()=false). Each fork gets a
//! content-and-lineage-bound identity that also folds in a monotonic sequence,
//! so a restored A after an A->B->A edit sequence has a DISTINCT identity from
//! the original A (ABA-safe). Concurrent replacements require EXPLICIT
//! deterministic selection (never an implicit winner). Merge of divergent
//! content is a Conflict, not an auto-resolution. Rollback is identity-bound: it
//! removes only the artifacts of the named task and can never delete another
//! task's concurrent replacement, and it is non-destructive to the append-only
//! log.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;

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

/// Whether this store overwrites filesystem source. Always false — branches are
/// a database-native in-memory graph, not file mutation or filesystem replay.
pub fn has_source_overwrite() -> bool {
    false
}

fn fnv1a(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Content-and-lineage-bound identity, folding in the fork sequence for
/// ABA-safety.
fn branch_id(parent: Option<&str>, task_id: &str, content: &str, seq: u64) -> String {
    format!(
        "{:016x}",
        fnv1a(&format!("{}|{}|{}|{}", parent.unwrap_or("root"), task_id, seq, content))
    )
}

#[derive(Default)]
pub struct CowBranchStore {
    log: Vec<Branch>,
    rolled_back: BTreeSet<String>,
}

impl CowBranchStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fork(&mut self, parent_id: Option<&str>, task_id: &str, content: &str, metadata: &str) -> Branch {
        let seq = self.log.len() as u64 + 1;
        let id = branch_id(parent_id, task_id, content, seq);
        let branch = Branch {
            id,
            parent_id: parent_id.map(str::to_string),
            task_id: task_id.to_string(),
            content: content.to_string(),
            metadata: metadata.to_string(),
            seq,
        };
        self.log.push(branch.clone());
        branch
    }

    pub fn get(&self, id: &str) -> Option<&Branch> {
        if self.rolled_back.contains(id) {
            return None;
        }
        self.log.iter().find(|b| b.id == id)
    }

    /// Reconstruct the exact source content and metadata of a branch.
    pub fn reconstruct(&self, id: &str) -> Option<(String, String)> {
        self.get(id).map(|b| (b.content.clone(), b.metadata.clone()))
    }

    /// Explicit, deterministic winner selection among concurrent branches — the
    /// lexicographically smallest identity, independent of input order.
    pub fn deterministic_select(&self, ids: &[String]) -> Option<String> {
        ids.iter().filter(|id| !self.rolled_back.contains(*id)).min().cloned()
    }

    pub fn merge(&self, a: &str, b: &str) -> MergeResult {
        match (self.get(a), self.get(b)) {
            (Some(x), Some(y)) if x.content == y.content => MergeResult::Merged(x.content.clone()),
            (Some(_), Some(_)) => MergeResult::Conflict,
            _ => MergeResult::Conflict,
        }
    }

    /// Identity-bound rollback: removes only the named task's branches from the
    /// active set, never another task's concurrent replacement, and leaves the
    /// append-only log intact.
    pub fn rollback(&mut self, task_id: &str) -> Vec<String> {
        let removed: Vec<String> = self
            .log
            .iter()
            .filter(|b| b.task_id == task_id && !self.rolled_back.contains(&b.id))
            .map(|b| b.id.clone())
            .collect();
        for id in &removed {
            self.rolled_back.insert(id.clone());
        }
        removed
    }

    pub fn log(&self) -> &[Branch] {
        &self.log
    }

    /// Restart: deterministically reconstruct the store from its append-only log.
    pub fn replay(log: &[Branch]) -> Self {
        Self { log: log.to_vec(), rolled_back: BTreeSet::new() }
    }
}
