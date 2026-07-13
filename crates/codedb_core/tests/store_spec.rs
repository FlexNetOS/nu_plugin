use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use codedb_core::store_spec::{StoreBackend, StoreSpec};

#[test]
fn parses_filesystem_paths_and_redb_urls_as_redb() {
    let plain = StoreSpec::parse("state/codedb.redb", None).expect("plain path is a redb store");
    assert_eq!(plain.backend(), StoreBackend::Redb);
    assert_eq!(plain.redb_path(), Some(Path::new("state/codedb.redb")));

    let url =
        StoreSpec::parse("redb:///var/lib/codedb/store.redb", None).expect("redb URL is valid");
    assert_eq!(url.backend(), StoreBackend::Redb);
    assert_eq!(
        url.redb_path(),
        Some(Path::new("/var/lib/codedb/store.redb"))
    );
}

#[test]
fn parses_postgresql_and_postgres_urls_as_postgresql() {
    let postgresql = StoreSpec::parse(
        "postgresql://codedb:super-secret@db.example.test/codedb?sslmode=require",
        None,
    )
    .expect("postgresql URL is valid");
    assert_eq!(postgresql.backend(), StoreBackend::PostgreSql);
    assert_eq!(
        postgresql.connection_string(),
        Some("postgresql://codedb:super-secret@db.example.test/codedb?sslmode=require")
    );
    assert_eq!(
        postgresql.to_string(),
        "postgresql://codedb:***@db.example.test/codedb?sslmode=require"
    );
    assert!(!format!("{postgresql:?}").contains("super-secret"));

    let postgres =
        StoreSpec::parse("postgres://db.example.test/codedb", None).expect("postgres URL is valid");
    assert_eq!(postgres.backend(), StoreBackend::PostgreSql);
    assert_eq!(
        postgres.connection_string(),
        Some("postgres://db.example.test/codedb")
    );
}

#[test]
fn bare_pg_requires_an_explicit_external_postgresql_dsn() {
    let error = StoreSpec::parse("pg", None).expect_err("bare pg without a DSN must fail");
    assert!(
        error.to_string().contains("explicit PostgreSQL DSN"),
        "unexpected error: {error}"
    );

    let spec = StoreSpec::parse(
        "pg",
        Some("postgresql://codedb:super-secret@db.example.test/codedb?password=query-secret"),
    )
    .expect("bare pg uses the caller-provided PostgreSQL DSN");
    assert_eq!(spec.backend(), StoreBackend::PostgreSql);
    assert_eq!(
        spec.connection_string(),
        Some("postgresql://codedb:super-secret@db.example.test/codedb?password=query-secret")
    );
    let displayed = spec.to_string();
    assert!(!displayed.contains("super-secret"));
    assert!(!displayed.contains("query-secret"));
    assert_eq!(
        displayed,
        "postgresql://codedb:***@db.example.test/codedb?password=***"
    );
}

#[test]
fn rejects_every_unknown_uri_scheme_without_filesystem_writes() {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is after the Unix epoch")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let untouched_path = std::env::temp_dir().join(format!(
        "codedb-core-store-spec-{}-{nonce}-{seq}/store",
        std::process::id()
    ));
    assert!(
        !untouched_path.exists(),
        "test path unexpectedly exists: {}",
        untouched_path.display()
    );

    for spec in [
        format!("sqlite://{}", untouched_path.display()),
        "mysql://codedb:super-secret@db.example.test/codedb".to_string(),
        "pg://db.example.test/codedb".to_string(),
        "https://db.example.test/codedb".to_string(),
    ] {
        let error = StoreSpec::parse(&spec, None).expect_err("unknown scheme must fail");
        assert!(
            error.to_string().contains("unsupported store URI scheme"),
            "unexpected error for {spec:?}: {error}"
        );
        assert!(
            !error.to_string().contains("super-secret"),
            "error leaked credentials: {error}"
        );
        assert!(
            !untouched_path.exists(),
            "parsing an unknown URI scheme wrote to {}",
            untouched_path.display()
        );
    }
}
