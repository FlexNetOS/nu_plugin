#![forbid(unsafe_code)]

//! PostgreSQL blob-store backend for CodeDB.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use codedb_core::store::{
    BlobStore, MaterializedFile, SourceFileRow, StoreError, StoreMetadataRow,
};
use postgres::{Client, NoTls};
use sha2::{Digest, Sha256};

pub const DEFAULT_TABLE: &str = "codebase_codedb";
const ORIGIN: &str = "codedb";

pub struct PgStore {
    client: RefCell<Client>,
    table: String,
}

impl PgStore {
    /// Explicit mutating initialization path. Capture setup may call this; read
    /// operations must use [`Self::open_existing`].
    pub fn initialize(conn: &str, table: &str) -> Result<Self, StoreError> {
        let table = sanitize_table(table)?;
        let mut client = connect_client(conn)?;
        let ddl = format!(
            "CREATE TABLE IF NOT EXISTS {table} (\
                block_id bigserial PRIMARY KEY,\
                module_path text NOT NULL UNIQUE,\
                block_type text NOT NULL DEFAULT 'file',\
                origin text NOT NULL DEFAULT '{ORIGIN}',\
                content bytea NOT NULL,\
                sha256 text NOT NULL,\
                metadata jsonb\
            );\
            ALTER TABLE {table} ADD COLUMN IF NOT EXISTS metadata jsonb;"
        );
        client
            .batch_execute(&ddl)
            .map_err(|error| StoreError::new(format!("initialize table {table}: {error}")))?;
        Ok(Self {
            client: RefCell::new(client),
            table,
        })
    }

    /// Non-mutating open for materialization/report/query paths. It verifies the
    /// table exists using `to_regclass` and never issues DDL.
    pub fn open_existing(conn: &str, table: &str) -> Result<Self, StoreError> {
        let table = sanitize_table(table)?;
        let mut client = connect_client(conn)?;
        let existing: Option<String> = client
            .query_one("SELECT to_regclass($1)::text", &[&table])
            .map_err(|error| StoreError::new(format!("inspect table {table}: {error}")))?
            .get(0);
        if existing.is_none() {
            return Err(StoreError::new(format!(
                "PostgreSQL CodeDB table does not exist: {table}; run an explicit capture initialization first"
            )));
        }
        Ok(Self {
            client: RefCell::new(client),
            table,
        })
    }

    pub fn table(&self) -> &str {
        &self.table
    }
}

fn connect_client(conn: &str) -> Result<Client, StoreError> {
    if conn.trim().is_empty() {
        return Err(StoreError::new("PostgreSQL DSN is required"));
    }
    Client::connect(conn, NoTls).map_err(|error| StoreError::new(format!("connect: {error}")))
}

fn sanitize_table(table: &str) -> Result<String, StoreError> {
    let ok = !table.is_empty()
        && table
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && table.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok {
        Ok(table.to_string())
    } else {
        Err(StoreError::new(format!(
            "invalid table name {table:?}: expected [A-Za-z_][A-Za-z0-9_]*"
        )))
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn batch_metadata_json() -> String {
    "{\"artifact_kind\":\"raw_blob\",\"permission_capture\":\"gap_not_available_for_raw_blob\"}"
        .to_string()
}

impl BlobStore for PgStore {
    fn persist_batch(
        &mut self,
        files: &[(String, Vec<u8>)],
    ) -> Result<Vec<SourceFileRow>, StoreError> {
        let mut client = self.client.borrow_mut();
        let mut tx = client
            .transaction()
            .map_err(|e| StoreError::new(format!("begin transaction: {e}")))?;
        let sql = format!(
            "INSERT INTO {} (module_path, block_type, origin, content, sha256, metadata) \
             VALUES ($1, 'file', '{ORIGIN}', $2, $3, $4::text::jsonb) \
             ON CONFLICT (module_path) DO UPDATE SET \
                content = EXCLUDED.content, \
                sha256 = EXCLUDED.sha256, \
                origin = EXCLUDED.origin, \
                block_type = EXCLUDED.block_type, \
                metadata = EXCLUDED.metadata",
            self.table
        );
        let metadata = batch_metadata_json();
        let mut out = Vec::with_capacity(files.len());
        for (relative_path, bytes) in files {
            let sha256 = sha256_hex(bytes);
            let content: &[u8] = bytes.as_slice();
            tx.execute(sql.as_str(), &[relative_path, &content, &sha256, &metadata])
                .map_err(|e| StoreError::new(format!("insert {relative_path}: {e}")))?;
            out.push(SourceFileRow {
                relative_path: relative_path.clone(),
                blob_ref: format!("sha256:{sha256}"),
                sha256,
                bytes: bytes.len() as u64,
            });
        }
        tx.commit()
            .map_err(|e| StoreError::new(format!("commit batch: {e}")))?;
        Ok(out)
    }

    fn captured_paths(&self) -> Result<BTreeSet<String>, StoreError> {
        let mut client = self.client.borrow_mut();
        let sql = format!("SELECT module_path FROM {}", self.table);
        let rows = client
            .query(sql.as_str(), &[])
            .map_err(|e| StoreError::new(format!("captured_paths: {e}")))?;
        Ok(rows.iter().map(|row| row.get::<_, String>(0)).collect())
    }

    fn read_source_file_blob(&self, relative_path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let mut client = self.client.borrow_mut();
        let sql = format!("SELECT content FROM {} WHERE module_path = $1", self.table);
        let rows = client
            .query(sql.as_str(), &[&relative_path])
            .map_err(|e| StoreError::new(format!("read_source_file_blob {relative_path}: {e}")))?;
        Ok(rows.first().map(|row| row.get::<_, Vec<u8>>(0)))
    }

    fn list_source_files(&self) -> Result<Vec<SourceFileRow>, StoreError> {
        let mut client = self.client.borrow_mut();
        let sql = format!(
            "SELECT module_path, sha256, octet_length(content) FROM {} ORDER BY module_path",
            self.table
        );
        let rows = client
            .query(sql.as_str(), &[])
            .map_err(|e| StoreError::new(format!("list_source_files: {e}")))?;
        Ok(rows
            .iter()
            .map(|row| {
                let relative_path: String = row.get(0);
                let sha256: String = row.get(1);
                let bytes: i32 = row.get(2);
                SourceFileRow {
                    relative_path,
                    blob_ref: format!("sha256:{sha256}"),
                    sha256,
                    bytes: bytes.max(0) as u64,
                }
            })
            .collect())
    }

    fn materialize_source_file(
        &self,
        relative_path: &str,
        output_path: &Path,
    ) -> Result<MaterializedFile, StoreError> {
        let mut client = self.client.borrow_mut();
        let sql = format!(
            "SELECT content, sha256, metadata::text FROM {} WHERE module_path = $1",
            self.table
        );
        let rows = client
            .query(sql.as_str(), &[&relative_path])
            .map_err(|e| StoreError::new(format!("materialize {relative_path}: {e}")))?;
        let row = rows
            .first()
            .ok_or_else(|| StoreError::new(format!("missing source file: {relative_path}")))?;
        let content: Vec<u8> = row.get(0);
        let sha256: String = row.get(1);
        let metadata_text: Option<String> = row.get(2);

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| StoreError::new(format!("create dir for {relative_path}: {e}")))?;
        }
        fs::write(output_path, &content)
            .map_err(|e| StoreError::new(format!("write {}: {e}", output_path.display())))?;

        #[cfg(unix)]
        if let Some(mode) = metadata_text.as_deref().and_then(parse_unix_mode) {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(output_path, fs::Permissions::from_mode(mode))
                .map_err(|e| StoreError::new(format!("chmod {}: {e}", output_path.display())))?;
        }
        #[cfg(not(unix))]
        let _ = metadata_text;

        let materialized_sha256 = sha256_file(output_path)?;
        Ok(MaterializedFile {
            path: output_path.to_path_buf(),
            blob_ref: format!("sha256:{sha256}"),
            sha256: materialized_sha256,
            bytes: content.len() as u64,
        })
    }

    fn store_metadata_rows(&self) -> Result<Vec<StoreMetadataRow>, StoreError> {
        let mut client = self.client.borrow_mut();
        let count_sql = format!("SELECT count(*) FROM {}", self.table);
        let count: i64 = client
            .query_one(count_sql.as_str(), &[])
            .map_err(|e| StoreError::new(format!("store_metadata_rows count: {e}")))?
            .get(0);
        let meta = |key: &str, value: String| StoreMetadataRow {
            table: "store_metadata".to_string(),
            key: key.to_string(),
            value,
        };
        Ok(vec![
            meta("store_backend", "postgresql".to_string()),
            meta("store_status", "initialized".to_string()),
            meta("checksum_algorithm", "sha256".to_string()),
            meta("table", self.table.clone()),
            meta("origin", ORIGIN.to_string()),
            meta("source_files", count.to_string()),
        ])
    }
}

#[cfg(unix)]
fn parse_unix_mode(metadata_text: &str) -> Option<u32> {
    let value = serde_json::from_str::<serde_json::Value>(metadata_text).ok()?;
    let field = value.get("unix_mode")?;
    if let Some(s) = field.as_str() {
        u32::from_str_radix(s, 8).ok()
    } else {
        field.as_u64().map(|v| v as u32)
    }
}

fn sha256_file(path: &Path) -> Result<String, StoreError> {
    let bytes = fs::read(path)
        .map_err(|e| StoreError::new(format!("re-checksum {}: {e}", path.display())))?;
    Ok(sha256_hex(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_table_accepts_bare_identifier() {
        assert_eq!(sanitize_table("codebase_codedb").unwrap(), "codebase_codedb");
        assert_eq!(sanitize_table("_x9").unwrap(), "_x9");
    }

    #[test]
    fn sanitize_table_rejects_injection() {
        assert!(sanitize_table("a; drop table x").is_err());
        assert!(sanitize_table("public.codebase").is_err());
        assert!(sanitize_table("").is_err());
        assert!(sanitize_table("9abc").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn parse_unix_mode_reads_octal_string_and_decimal() {
        assert_eq!(parse_unix_mode("{\"unix_mode\":\"755\"}"), Some(0o755));
        assert_eq!(parse_unix_mode("{\"unix_mode\":493}"), Some(0o755));
        assert_eq!(parse_unix_mode("{\"artifact_kind\":\"raw_blob\"}"), None);
    }
}
