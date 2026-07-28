//! ARCHBP-004 — approval-gated semantic edit authority.
//!
//! A semantic edit plan is bound to a source SNAPSHOT digest. Before an
//! ISOLATED patch is materialized the plan must: contain no direct SQL mutation
//! of canonical source, match the current source (no drift), carry owner
//! approval, confine every edit to the allowed paths (no path escape), and pass
//! the repository gates. Only then is an isolated patch materialized — this gate
//! NEVER writes canonical source (can_apply_to_canonical()=false); the patch
//! awaits normal Git review. Every decision records an immutable, append-only
//! receipt.

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
    false
}

fn path_confined(path: &str, allowed: &[String]) -> bool {
    !path.contains("..") && allowed.iter().any(|p| path.starts_with(p))
}

#[derive(Default)]
pub struct EditGate {
    receipts: Vec<Decision>,
}

impl EditGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn evaluate(&mut self, plan: &EditPlan, current: &SourceState, gate: &RepoGate) -> Decision {
        let seq = self.receipts.len() as u64 + 1;
        let (outcome, reason, isolated_patch) = self.classify(plan, current, gate);
        let decision = Decision {
            plan_id: plan.plan_id.clone(),
            outcome,
            reason,
            receipt_seq: seq,
            isolated_patch,
        };
        self.receipts.push(decision.clone());
        decision
    }

    fn classify(
        &self,
        plan: &EditPlan,
        current: &SourceState,
        gate: &RepoGate,
    ) -> (Outcome, String, Option<Vec<(String, String)>>) {
        // 1. direct SQL mutation of canonical source is forbidden.
        if plan
            .actions
            .iter()
            .any(|a| matches!(a, EditAction::DirectSqlMutation { .. }))
        {
            return (Outcome::Reject, "direct-sql-mutation-forbidden".to_string(), None);
        }
        // 2. the plan must be bound to the current source (no drift).
        if plan.source_snapshot_digest != current.digest {
            return (Outcome::Reject, "stale-source-snapshot-drift".to_string(), None);
        }
        // 3. owner approval is required.
        if plan.approved_by.is_none() {
            return (Outcome::Reject, "missing-owner-approval".to_string(), None);
        }
        // 4. every edit must stay inside the allowed paths (no path escape).
        for action in &plan.actions {
            if let EditAction::EditFile { path, .. } = action {
                if !path_confined(path, &plan.allowed_paths) {
                    return (Outcome::Reject, format!("path-escape:{path}"), None);
                }
            }
        }
        // 5. the repository gates must pass before apply.
        if !gate.passed {
            return (Outcome::Reject, "unproved-apply-repository-gate-failed".to_string(), None);
        }
        // 6. approved, bounded, proven -> materialize an ISOLATED patch.
        //    Canonical source is never touched here.
        let patch: Vec<(String, String)> = plan
            .actions
            .iter()
            .filter_map(|a| match a {
                EditAction::EditFile { path, new_content } => Some((path.clone(), new_content.clone())),
                EditAction::DirectSqlMutation { .. } => None,
            })
            .collect();
        (
            Outcome::Allow,
            "approved-bounded-plan-isolated-patch-materialized".to_string(),
            Some(patch),
        )
    }

    pub fn receipts(&self) -> &[Decision] {
        &self.receipts
    }
}
