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
        require_error(PgStore::rollback_last_migration("", "offline_safety")),
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
        require_error(PgStore::rollback_last_migration(&dsn, "bad-name")),
    ] {
        assert!(error.contains("invalid table name"));
        assert!(!error.contains(secret));
        assert!(!error.contains(&dsn));
    }
}

#[test]
fn remote_tcp_without_verified_tls_is_refused_before_authentication() {
    let secret = "must-not-reach-postgresql-auth";
    for dsn in [
        format!("host=db.example.invalid user=codedb password={secret} dbname=codedb"),
        format!("postgresql://codedb:{secret}@db.example.invalid/codedb?sslmode=disable"),
        format!("postgresql://codedb:{secret}@db.example.invalid/codedb?sslmode=require"),
    ] {
        let error = require_error(PgStore::open_existing(&dsn, "offline_safety"));
        assert!(error.contains("verified TLS"));
        assert!(!error.contains(secret));
        assert!(!error.contains(&dsn));
        assert!(!error.contains("db.example.invalid"));
    }
}

#[test]
fn remote_tcp_requires_an_explicit_ca_before_connection() {
    let secret = "missing-ca-must-stay-redacted";
    let dsn = format!("postgresql://codedb:{secret}@db.example.invalid/codedb?sslmode=verify-full");
    let error = require_error(PgStore::open_existing(&dsn, "offline_safety"));
    assert!(error.contains("CA certificate"));
    assert!(!error.contains(secret));
    assert!(!error.contains(&dsn));
    assert!(!error.contains("db.example.invalid"));
}

#[cfg(unix)]
#[test]
fn explicit_unix_socket_path_is_the_only_plaintext_connection_policy() {
    let secret = "unix-socket-secret";
    let dsn = format!(
        "host=/definitely/missing/codedb-postgresql-socket user=codedb password={secret} dbname=codedb sslmode=disable"
    );
    let error = require_error(PgStore::open_existing(&dsn, "offline_safety"));
    assert_eq!(
        error,
        "PostgreSQL connection failed; connection details redacted"
    );
    assert!(!error.contains(secret));
    assert!(!error.contains(&dsn));
}
