// ARCHBP-004 — approval-gated semantic edit authority gate.
//
// Source-snapshot-bound edit plans must have owner approval, confine edits to
// allowed paths, match the current source (no drift), and pass repository gates
// before an ISOLATED patch is materialized. Direct SQL mutation of canonical
// source is forbidden, and this gate never changes canonical source.

use semantic_edit_gate::{
    can_apply_to_canonical, EditAction, EditGate, EditPlan, Outcome, RepoGate, SourceState,
};

const SNAP: &str = "snap-digest-abc";

fn base_plan() -> EditPlan {
    EditPlan {
        plan_id: "P1".to_string(),
        source_snapshot_digest: SNAP.to_string(),
        actions: vec![EditAction::EditFile {
            path: "src/lib.rs".to_string(),
            new_content: "// edited".to_string(),
        }],
        allowed_paths: vec!["src/".to_string()],
        approved_by: Some("owner".to_string()),
    }
}
fn fresh() -> SourceState {
    SourceState { digest: SNAP.to_string() }
}
fn passing() -> RepoGate {
    RepoGate { passed: true }
}

#[test]
fn gate_never_applies_to_canonical_source() {
    assert!(!can_apply_to_canonical());
}

#[test]
fn rejects_direct_sql_mutation_of_canonical_source() {
    let mut plan = base_plan();
    plan.actions.push(EditAction::DirectSqlMutation {
        statement: "UPDATE code SET body='x'".to_string(),
    });
    let d = EditGate::new().evaluate(&plan, &fresh(), &passing());
    assert_eq!(d.outcome, Outcome::Reject);
    assert!(d.reason.contains("direct-sql"));
}

#[test]
fn rejects_stale_source_snapshot_drift() {
    let d = EditGate::new().evaluate(&base_plan(), &SourceState { digest: "drifted".to_string() }, &passing());
    assert_eq!(d.outcome, Outcome::Reject);
    assert!(d.reason.contains("stale") || d.reason.contains("drift"));
}

#[test]
fn rejects_missing_owner_approval() {
    let mut plan = base_plan();
    plan.approved_by = None;
    let d = EditGate::new().evaluate(&plan, &fresh(), &passing());
    assert_eq!(d.outcome, Outcome::Reject);
    assert!(d.reason.contains("approval"));
}

#[test]
fn rejects_path_escape() {
    let mut plan = base_plan();
    plan.actions = vec![EditAction::EditFile {
        path: "../../etc/passwd".to_string(),
        new_content: "x".to_string(),
    }];
    let d = EditGate::new().evaluate(&plan, &fresh(), &passing());
    assert_eq!(d.outcome, Outcome::Reject);
    assert!(d.reason.contains("path-escape"));
}

#[test]
fn rejects_unproved_apply() {
    let d = EditGate::new().evaluate(&base_plan(), &fresh(), &RepoGate { passed: false });
    assert_eq!(d.outcome, Outcome::Reject);
    assert!(d.reason.contains("unproved") || d.reason.contains("gate"));
}

#[test]
fn approved_bounded_plan_materializes_isolated_patch() {
    let d = EditGate::new().evaluate(&base_plan(), &fresh(), &passing());
    assert_eq!(d.outcome, Outcome::Allow);
    let patch = d.isolated_patch.expect("isolated patch materialized");
    assert_eq!(patch.len(), 1);
    assert_eq!(patch[0].0, "src/lib.rs");
    // canonical source is never touched — the gate only produces an isolated patch
    assert!(!can_apply_to_canonical());
}

#[test]
fn every_decision_records_an_immutable_append_only_receipt() {
    let mut gate = EditGate::new();
    let _ = gate.evaluate(&base_plan(), &fresh(), &passing());
    let mut bad = base_plan();
    bad.approved_by = None;
    let _ = gate.evaluate(&bad, &fresh(), &passing());
    assert_eq!(gate.receipts().len(), 2);
    // monotonically increasing receipt sequence
    assert_eq!(gate.receipts()[0].receipt_seq, 1);
    assert_eq!(gate.receipts()[1].receipt_seq, 2);
    assert_eq!(gate.receipts()[0].outcome, Outcome::Allow);
    assert_eq!(gate.receipts()[1].outcome, Outcome::Reject);
}
