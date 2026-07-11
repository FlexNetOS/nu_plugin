#!/usr/bin/env python3
"""Idempotently wire explicit PostgreSQL initialization and read-only open."""

from pathlib import Path

PATH = Path("crates/codedb/src/main.rs")


def complete(text: str) -> bool:
    return (
        "PgStore::initialize" in text
        and "PgStore::open_existing" in text
        and "PostgreSQL DSN is required" in text
        and "codedb_store_pg::DEFAULT_CONN" not in text
        and "PgStore::connect" not in text
    )


def main() -> None:
    text = PATH.read_text()
    if complete(text):
        return
    old = """fn pg_conn_string(spec: &str, args: &[String]) -> String {
    if spec.starts_with("postgres://") || spec.starts_with("postgresql://") {
        return spec.to_string();
    }
    if let Some(rest) = spec.strip_prefix("pg://").filter(|rest| !rest.is_empty()) {
        return format!("postgres://{rest}");
    }
    option_value(args, "--pg-conn")
        .map(str::to_string)
        .or_else(|| env::var("CODEDB_PG_CONN").ok())
        .or_else(|| env::var("DATABASE_URL").ok())
        .unwrap_or_else(|| codedb_store_pg::DEFAULT_CONN.to_string())
}
"""
    new = """fn pg_conn_string(spec: &str, args: &[String]) -> Result<String, CliError> {
    if spec.starts_with("postgres://") || spec.starts_with("postgresql://") {
        return Ok(spec.to_string());
    }
    if let Some(rest) = spec.strip_prefix("pg://").filter(|rest| !rest.is_empty()) {
        return Ok(format!("postgres://{rest}"));
    }
    option_value(args, "--pg-conn")
        .map(str::to_string)
        .or_else(|| env::var("CODEDB_PG_CONN").ok())
        .or_else(|| env::var("DATABASE_URL").ok())
        .filter(|dsn| !dsn.trim().is_empty())
        .ok_or_else(|| CliError::Message(
            "PostgreSQL DSN is required: pass --pg-conn, CODEDB_PG_CONN, DATABASE_URL, or a postgres:// store URL".to_string()
        ))
}
"""
    if text.count(old) != 1:
        raise SystemExit("PostgreSQL DSN resolver anchor drifted")
    text = text.replace(old, new, 1)
    call = "let conn = pg_conn_string(store_spec, args);"
    if text.count(call) != 2:
        raise SystemExit("PostgreSQL DSN call-site anchors drifted")
    text = text.replace(call, "let conn = pg_conn_string(store_spec, args)?;")
    if text.count("codedb_store_pg::PgStore::connect") != 2:
        raise SystemExit("PostgreSQL open call-site anchors drifted")
    text = text.replace(
        "codedb_store_pg::PgStore::connect",
        "codedb_store_pg::PgStore::initialize",
        1,
    )
    text = text.replace(
        "codedb_store_pg::PgStore::connect",
        "codedb_store_pg::PgStore::open_existing",
        1,
    )
    if not complete(text):
        raise SystemExit("PostgreSQL open postconditions incomplete")
    PATH.write_text(text)


if __name__ == "__main__":
    main()
