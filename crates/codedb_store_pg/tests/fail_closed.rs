//! Offline safety regressions for the PostgreSQL backend.
//!
//! These tests intentionally require no PostgreSQL service. Service-backed
//! parity remains fail-closed behind the `pg-integration` feature and its
//! mandatory `CODEDB_PG_CONN` setting.

use codedb_store_pg::PgStore;

fn require_error<T>(result: Result<T, codedb_core::store::StoreError>) -> String {
    match result {
        Ok(_) => panic!("operation unexpectedly succeeded"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn every_lifecycle_entrypoint_rejects_an_empty_dsn() {
    for error in [
        require_error(PgStore::initialize("", "offline_safety")),
        require_error(PgStore::open_existing("", "offline_safety")),
        require_error(PgStore::migrate("", "offline_safety")),
    ] {
        assert_eq!(error, "PostgreSQL DSN is required");
    }
}

#[test]
fn unsafe_table_names_are_rejected_before_connection_and_do_not_echo_credentials() {
    let secret = "must-not-appear-in-diagnostics";
    let dsn = format!("postgresql://codedb:{secret}@localhost/codedb");

    for error in [
        require_error(PgStore::initialize(&dsn, "bad; drop table x")),
        require_error(PgStore::open_existing(&dsn, "public.codebase")),
        require_error(PgStore::migrate(&dsn, "9invalid")),
    ] {
        assert!(error.contains("invalid table name"));
        assert!(!error.contains(secret));
        assert!(!error.contains(&dsn));
    }
}
