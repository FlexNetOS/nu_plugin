// ARCHBP-005 — database-native copy-on-write branches and identity-bound rollback.
//
// Covers fork, concurrent replacement, ABA identity, merge conflict, branch
// deletion, and restart; selected branches reconstruct exact source and
// metadata; rollback removes only identity-matched task artifacts and cannot
// delete a concurrent replacement.

use cow_branch_store::{has_source_overwrite, CowBranchStore, MergeResult};

#[test]
fn is_database_native_not_filesystem_replay() {
    assert!(!has_source_overwrite());
}

#[test]
fn fork_creates_isolated_cow_branch_with_lineage() {
    let mut s = CowBranchStore::new();
    let root = s.fork(None, "T1", "fn a(){}", "meta-root");
    let child = s.fork(Some(&root.id), "T1", "fn a(){ b(); }", "meta-child");
    assert_eq!(child.parent_id.as_deref(), Some(root.id.as_str()));
    assert_ne!(child.id, root.id);
    // reconstruct exact source + metadata
    let (content, meta) = s.reconstruct(&child.id).expect("reconstruct");
    assert_eq!(content, "fn a(){ b(); }");
    assert_eq!(meta, "meta-child");
}

#[test]
fn concurrent_replacement_needs_explicit_deterministic_selection() {
    let mut s = CowBranchStore::new();
    let base = s.fork(None, "T1", "base", "m");
    let edit1 = s.fork(Some(&base.id), "T1", "edit-one", "m1");
    let edit2 = s.fork(Some(&base.id), "T1", "edit-two", "m2");
    assert_ne!(edit1.id, edit2.id);
    // deterministic, explicit winner selection (not implicit)
    let winner_a = s.deterministic_select(&[edit1.id.clone(), edit2.id.clone()]);
    let winner_b = s.deterministic_select(&[edit2.id.clone(), edit1.id.clone()]);
    assert_eq!(winner_a, winner_b, "selection must be deterministic regardless of input order");
    assert!(winner_a.is_some());
}

#[test]
fn aba_identity_is_distinct() {
    let mut s = CowBranchStore::new();
    let base = s.fork(None, "T1", "base", "m");
    let a = s.fork(Some(&base.id), "T1", "content-A", "m");
    let _b = s.fork(Some(&base.id), "T1", "content-B", "m");
    let a_again = s.fork(Some(&base.id), "T1", "content-A", "m");
    // same content, but a distinct branch identity — the restored A is not
    // confused with the original A (ABA-safe).
    assert_ne!(a.id, a_again.id);
}

#[test]
fn merge_conflict_is_not_auto_resolved() {
    let mut s = CowBranchStore::new();
    let base = s.fork(None, "T1", "base", "m");
    let x = s.fork(Some(&base.id), "T1", "edit-x", "m");
    let y = s.fork(Some(&base.id), "T1", "edit-y", "m");
    assert_eq!(s.merge(&x.id, &y.id), MergeResult::Conflict);
    // identical content merges cleanly
    let z = s.fork(Some(&base.id), "T2", "edit-x", "m");
    assert!(matches!(s.merge(&x.id, &z.id), MergeResult::Merged(_)));
}

#[test]
fn rollback_is_identity_bound_and_spares_concurrent_replacement() {
    let mut s = CowBranchStore::new();
    let base = s.fork(None, "T1", "base", "m");
    let t1_edit = s.fork(Some(&base.id), "T1", "t1-edit", "m");
    let t2_replacement = s.fork(Some(&base.id), "T2", "t2-replacement", "m");
    let removed = s.rollback("T1");
    // only T1's own artifacts are removed (base + t1_edit), never T2's replacement
    assert!(removed.contains(&t1_edit.id));
    assert!(!removed.contains(&t2_replacement.id));
    assert!(s.get(&t2_replacement.id).is_some(), "concurrent replacement must survive");
    assert!(s.get(&t1_edit.id).is_none());
}

#[test]
fn restart_replays_the_log_deterministically() {
    let mut s = CowBranchStore::new();
    let base = s.fork(None, "T1", "base", "m");
    let _c = s.fork(Some(&base.id), "T1", "child", "m2");
    let replayed = CowBranchStore::replay(s.log());
    assert_eq!(replayed.log(), s.log(), "restart must reconstruct the identical branch log");
}
