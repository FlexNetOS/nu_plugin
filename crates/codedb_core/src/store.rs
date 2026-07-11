//! Backend-agnostic blob-store surface.
//!
//! [`BlobStore`] is the synchronous storage contract CodeDB captures/materializes
//! through. It mirrors the semantics of the redb free functions in
//! `codedb_store_redb` so any backend (redb file, PostgreSQL, …) is drop-in:
//! content-addressed (sha256) blob persistence, a resume skip-set, byte-exact
//! read-back, and metadata-aware materialization that restores unix modes.

use std::collections::BTreeSet;
use std::error::Error as StdError;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(String);

impl StoreError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn message(&self) -> &str {
        &self.0
    }
}

impl Display for StoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl StdError for StoreError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFileRow {
    pub relative_path: String,
    pub blob_ref: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedFile {
    pub path: PathBuf,
    pub blob_ref: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMetadataRow {
    pub table: String,
    pub key: String,
    pub value: String,
}

/// Validate an untrusted stored path before joining it to a materialization root.
///
/// The accepted grammar is deliberately portable: non-empty `/`-separated normal
/// components only. Windows separators/prefixes, absolute paths, repeated
/// separators, `.`/`..`, and NUL are rejected on every host so a database created
/// on one platform cannot escape when restored on another.
pub fn safe_materialization_path(
    output_root: &Path,
    relative_path: &str,
) -> Result<PathBuf, StoreError> {
    if relative_path.is_empty()
        || relative_path.contains('\0')
        || relative_path.contains('\\')
        || relative_path.starts_with('/')
        || relative_path.ends_with('/')
        || relative_path.contains("//")
        || (relative_path.as_bytes().get(1) == Some(&b':')
            && relative_path
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic))
    {
        return Err(StoreError::new(format!(
            "unsafe materialization path: {relative_path:?}"
        )));
    }

    let path = Path::new(relative_path);
    if path.is_absolute() {
        return Err(StoreError::new(format!(
            "absolute materialization path is forbidden: {relative_path:?}"
        )));
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) if !value.is_empty() => normalized.push(value),
            _ => {
                return Err(StoreError::new(format!(
                    "non-normal materialization component is forbidden: {relative_path:?}"
                )));
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(StoreError::new("empty materialization path is forbidden"));
    }
    Ok(output_root.join(normalized))
}

/// Prepare a safe output path and reject existing symlink ancestors/final paths.
/// This closes the gap left by lexical `..` validation when an attacker plants a
/// symlink below the chosen output root.
pub fn prepare_materialization_path(
    output_root: &Path,
    relative_path: &str,
) -> Result<PathBuf, StoreError> {
    let lexical = safe_materialization_path(output_root, relative_path)?;
    fs::create_dir_all(output_root).map_err(|error| {
        StoreError::new(format!(
            "create materialization root {}: {error}",
            output_root.display()
        ))
    })?;
    let canonical_root = fs::canonicalize(output_root).map_err(|error| {
        StoreError::new(format!(
            "canonicalize materialization root {}: {error}",
            output_root.display()
        ))
    })?;
    let relative = lexical
        .strip_prefix(output_root)
        .map_err(|_| StoreError::new("materialization path escaped output root"))?;
    let mut current = canonical_root.clone();
    let components = relative.components().collect::<Vec<_>>();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(StoreError::new(format!(
                    "symlink ancestor is forbidden during materialization: {}",
                    current.display()
                )));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(StoreError::new(format!(
                    "non-directory ancestor blocks materialization: {}",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|error| {
                    StoreError::new(format!("create {}: {error}", current.display()))
                })?;
            }
            Err(error) => {
                return Err(StoreError::new(format!(
                    "inspect {}: {error}",
                    current.display()
                )));
            }
        }
        let canonical = fs::canonicalize(&current).map_err(|error| {
            StoreError::new(format!("canonicalize {}: {error}", current.display()))
        })?;
        if !canonical.starts_with(&canonical_root) {
            return Err(StoreError::new(format!(
                "materialization ancestor escaped output root: {}",
                current.display()
            )));
        }
    }
    let output = canonical_root.join(relative);
    if fs::symlink_metadata(&output)
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(StoreError::new(format!(
            "symlink output is forbidden during materialization: {}",
            output.display()
        )));
    }
    Ok(output)
}

pub trait BlobStore {
    fn persist_batch(
        &mut self,
        files: &[(String, Vec<u8>)],
    ) -> Result<Vec<SourceFileRow>, StoreError>;

    fn captured_paths(&self) -> Result<BTreeSet<String>, StoreError>;

    fn read_source_file_blob(&self, relative_path: &str) -> Result<Option<Vec<u8>>, StoreError>;

    fn list_source_files(&self) -> Result<Vec<SourceFileRow>, StoreError>;

    fn materialize_source_file(
        &self,
        relative_path: &str,
        output_path: &Path,
    ) -> Result<MaterializedFile, StoreError>;

    fn store_metadata_rows(&self) -> Result<Vec<StoreMetadataRow>, StoreError>;
}
