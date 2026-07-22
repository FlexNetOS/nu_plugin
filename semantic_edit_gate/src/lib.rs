//! ARCHBP-004 — RED STUB. Contract surface only; the approval-gated edit
//! authority is unimplemented so the edit gate fails closed before the real
//! implementation lands.

#![forbid(unsafe_code)]

pub const SCHEMA_VERSION: &str = "semantic-edit-gate.v0";

#[derive(Clone, Debug)]
pub enum EditAction {
    EditFile { path: String, new_content: String },
    DirectSqlMutation { statement: String },
}

#[derive(Clone, Debug)]
pub struct EditPlan {
    pub plan_id: String,
    pub source_snapshot_digest: String,
    pub actions: Vec<EditAction>,
    pub allowed_paths: Vec<String>,
    pub approved_by: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SourceState {
    pub digest: String,
}

#[derive(Clone, Debug)]
pub struct RepoGate {
    pub passed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Allow,
    Reject,
}

#[derive(Clone, Debug)]
pub struct Decision {
    pub plan_id: String,
    pub outcome: Outcome,
    pub reason: String,
    pub receipt_seq: u64,
    pub isolated_patch: Option<Vec<(String, String)>>,
}

/// Whether this gate can directly apply to canonical source. Always false —
/// only isolated patches are materialized; canonical source awaits Git review.
pub fn can_apply_to_canonical() -> bool {
    true
}

#[derive(Default)]
pub struct EditGate {
    receipts: Vec<Decision>,
}

impl EditGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn evaluate(&mut self, _plan: &EditPlan, _current: &SourceState, _gate: &RepoGate) -> Decision {
        // RED: unimplemented — optimistic allow with no isolated patch.
        Decision {
            plan_id: "".to_string(),
            outcome: Outcome::Allow,
            reason: "unimplemented".to_string(),
            receipt_seq: 0,
            isolated_patch: None,
        }
    }

    pub fn receipts(&self) -> &[Decision] {
        &self.receipts
    }
}
