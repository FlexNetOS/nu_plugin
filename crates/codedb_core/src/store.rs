//! Backend-agnostic blob-store surface.
//!
//! [`BlobStore`] is the synchronous storage contract CodeDB captures/materializes
//! through. It mirrors the semantics of the redb free functions in
//! `codedb_store_redb` so any backend (redb file, PostgreSQL, …) is drop-in:
//! content-addressed (sha256) blob persistence, a resume skip-set, byte-exact
//! read-back, and metadata-aware materialization that restores unix modes.

use std::collections::BTreeSet;
#[cfg(target_os = "linux")]
use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt::{Display, Formatter};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::SchemaVersion;
pub use crate::store_spec::StoreBackend;

#[cfg(unix)]
use std::ffi::{OsStr, OsString};
#[cfg(unix)]
use std::fs::File;
#[cfg(unix)]
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(target_os = "linux")]
use std::sync::{Mutex, OnceLock};

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

/// Exact authority to roll back one successful atomic publication.
///
/// The destination parent descriptor remains bound even if its pathname is
/// renamed, while the device/inode pair identifies only the entry published by
/// this attempt. Fields are private so callers cannot forge rollback authority.
pub struct MaterializedFileRollback {
    path: PathBuf,
    #[cfg(target_os = "linux")]
    parent: rustix::fd::OwnedFd,
    #[cfg(target_os = "linux")]
    final_name: OsString,
    #[cfg(target_os = "linux")]
    parent_device: u64,
    #[cfg(target_os = "linux")]
    parent_inode: u64,
    #[cfg(target_os = "linux")]
    file_device: u64,
    #[cfg(target_os = "linux")]
    file_inode: u64,
}

impl std::fmt::Debug for MaterializedFileRollback {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MaterializedFileRollback")
            .field("path", &self.path)
            .field(
                "identity",
                &"<bound destination parent and file device/inode>",
            )
            .finish()
    }
}

#[cfg(target_os = "linux")]
fn retained_materialization_rollbacks() -> &'static Mutex<HashMap<PathBuf, MaterializedFileRollback>>
{
    static ROLLBACKS: OnceLock<Mutex<HashMap<PathBuf, MaterializedFileRollback>>> = OnceLock::new();
    ROLLBACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMetadataRow {
    pub table: String,
    pub key: String,
    pub value: String,
}

/// The schema emitted by new CodeDB stores.
pub const CURRENT_STORE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0, 0);
pub const CURRENT_STORE_SCHEMA_VERSION_TEXT: &str = "1.0.0";

/// The only pre-versioned/legacy layout currently accepted by explicit
/// migration entrypoints. Read-only opens still refuse this version.
pub const LEGACY_STORE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(0, 9, 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreMigrationStep {
    pub id: &'static str,
    pub from: SchemaVersion,
    pub to: SchemaVersion,
}

impl StoreMigrationStep {
    pub const fn new(id: &'static str, from: SchemaVersion, to: SchemaVersion) -> Self {
        Self { id, from, to }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMigrationPlan {
    pub backend: StoreBackend,
    pub observed_version: SchemaVersion,
    pub target_version: SchemaVersion,
    pub steps: Vec<StoreMigrationStep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreBackupKind {
    FileCopy,
    TransactionalTableSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMigrationBackup {
    pub kind: StoreBackupKind,
    /// Backend-safe backup locator. PostgreSQL locators are validated relation
    /// identifiers; file locators are display paths, never connection strings.
    pub reference: String,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMigrationReport {
    pub plan: StoreMigrationPlan,
    pub backup: Option<StoreMigrationBackup>,
    pub applied_steps: Vec<&'static str>,
    pub rolled_back: bool,
}

/// Strictly parse the persisted `major.minor.patch` store schema format.
pub fn parse_schema_version(value: &str) -> Result<SchemaVersion, StoreError> {
    let mut fields = value.split('.');
    let parse_field = |field: Option<&str>| -> Result<u16, StoreError> {
        let field = field
            .ok_or_else(|| StoreError::new(format!("invalid store schema version {value:?}")))?;
        if field.is_empty() || !field.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(StoreError::new(format!(
                "invalid store schema version {value:?}"
            )));
        }
        field
            .parse::<u16>()
            .map_err(|_| StoreError::new(format!("invalid store schema version {value:?}")))
    };
    let version = SchemaVersion::new(
        parse_field(fields.next())?,
        parse_field(fields.next())?,
        parse_field(fields.next())?,
    );
    if fields.next().is_some() {
        return Err(StoreError::new(format!(
            "invalid store schema version {value:?}"
        )));
    }
    Ok(version)
}

impl Display for SchemaVersion {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Resolve an ordered migration path from backend-provided steps.
///
/// The planner knows no backend layout details. It rejects downgrades,
/// ambiguity, cycles/non-advancing steps, paths that overshoot the requested
/// target, and unknown versions before a backend performs backup or mutation.
pub fn plan_store_migration(
    backend: StoreBackend,
    observed_version: SchemaVersion,
    target_version: SchemaVersion,
    supported_steps: &[StoreMigrationStep],
) -> Result<StoreMigrationPlan, StoreError> {
    if version_tuple(observed_version) > version_tuple(target_version) {
        return Err(StoreError::new(format!(
            "{} store schema {} would require a downgrade to {}; refusing",
            backend_name(backend),
            observed_version,
            target_version
        )));
    }

    let mut current = observed_version;
    let mut steps = Vec::new();
    while current != target_version {
        let candidates = supported_steps
            .iter()
            .copied()
            .filter(|step| step.from == current)
            .collect::<Vec<_>>();
        let step = match candidates.as_slice() {
            [] => {
                return Err(StoreError::new(format!(
                    "no supported migration for {} store schema {} toward {}",
                    backend_name(backend),
                    current,
                    target_version
                )));
            }
            [step] => *step,
            _ => {
                return Err(StoreError::new(format!(
                    "ambiguous migration routes for {} store schema {}",
                    backend_name(backend),
                    current
                )));
            }
        };
        if version_tuple(step.to) <= version_tuple(current) {
            return Err(StoreError::new(format!(
                "migration step {:?} does not advance schema {}",
                step.id, current
            )));
        }
        if version_tuple(step.to) > version_tuple(target_version) {
            return Err(StoreError::new(format!(
                "migration step {:?} overshoots target schema {}",
                step.id, target_version
            )));
        }
        steps.push(step);
        current = step.to;
        if steps.len() > supported_steps.len() {
            return Err(StoreError::new("store migration route contains a cycle"));
        }
    }

    Ok(StoreMigrationPlan {
        backend,
        observed_version,
        target_version,
        steps,
    })
}

const fn version_tuple(version: SchemaVersion) -> (u16, u16, u16) {
    (version.major, version.minor, version.patch)
}

const fn backend_name(backend: StoreBackend) -> &'static str {
    match backend {
        StoreBackend::Redb => "redb",
        StoreBackend::PostgreSql => "PostgreSQL",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainedRegularFile {
    pub bytes: Vec<u8>,
    pub unix_mode: Option<u32>,
}

pub struct ContainedRegularFileHandle {
    #[cfg(unix)]
    file: File,
    unix_mode: Option<u32>,
}

impl std::fmt::Debug for ContainedRegularFileHandle {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ContainedRegularFileHandle")
            .field("descriptor", &"<opened regular file>")
            .field("unix_mode", &self.unix_mode)
            .finish()
    }
}

impl ContainedRegularFileHandle {
    /// Return a Linux procfs path that resolves to this already-open file.
    ///
    /// This is intended only for an in-process library that requires a path to
    /// reopen a file. The handle must remain alive until that library has
    /// completed its open.
    #[cfg(target_os = "linux")]
    pub fn proc_fd_path(&self) -> PathBuf {
        use std::os::fd::AsRawFd;

        PathBuf::from(format!("/proc/self/fd/{}", self.file.as_raw_fd()))
    }

    pub fn read_all_consistent(mut self) -> Result<ContainedRegularFile, StoreError> {
        read_all_from_contained_handle(&mut self)
    }
}

/// An opened repository/directory root used for race-resistant reads.
///
/// The root descriptor remains bound to the directory inode even if its
/// pathname is renamed or replaced. Reads resolve beneath that descriptor with
/// `openat2(RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS)` and
/// derive bytes and metadata from the same opened file.
pub struct ContainedDirectory {
    #[cfg(unix)]
    descriptor: rustix::fd::OwnedFd,
}

impl std::fmt::Debug for ContainedDirectory {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ContainedDirectory")
            .field("descriptor", &"<opened directory>")
            .finish()
    }
}

impl ContainedDirectory {
    pub fn open_existing(path: &Path) -> Result<Self, StoreError> {
        open_contained_directory(path)
    }

    pub fn read_regular_file(
        &self,
        relative_path: &str,
    ) -> Result<ContainedRegularFile, StoreError> {
        self.open_regular_file_handle(relative_path)?
            .read_all_consistent()
    }

    pub fn open_regular_file_handle(
        &self,
        relative_path: &str,
    ) -> Result<ContainedRegularFileHandle, StoreError> {
        open_contained_regular_file(self, relative_path)
    }
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

/// Prepare a safe output path using descriptor-relative directory traversal.
///
/// On Unix, every existing component is opened with `O_DIRECTORY|O_NOFOLLOW`
/// and every missing component is created with `mkdirat` relative to the
/// already-open parent. A concurrent path replacement therefore cannot redirect
/// directory creation outside the selected root. The returned path is only a
/// display/selection value; publication must still use [`atomic_materialize_file`]
/// so the final open/write/rename sequence remains handle-bound.
///
/// Platforms without descriptor-relative no-follow primitives fail closed.
pub fn prepare_materialization_path(
    output_root: &Path,
    relative_path: &str,
) -> Result<PathBuf, StoreError> {
    let lexical = safe_materialization_path(output_root, relative_path)?;
    prepare_materialization_path_impl(output_root, relative_path)?;
    Ok(lexical)
}

#[cfg(unix)]
fn prepare_materialization_path_impl(
    output_root: &Path,
    relative_path: &str,
) -> Result<(), StoreError> {
    let mut directory = open_or_create_directory_chain(output_root)?;
    let relative = Path::new(relative_path);
    let components = relative.components().collect::<Vec<_>>();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        let Component::Normal(segment) = component else {
            return Err(StoreError::new(
                "non-normal component reached descriptor-relative materialization",
            ));
        };
        directory = open_or_create_child_directory(&directory, segment)?;
    }
    let final_name = components
        .last()
        .and_then(|component| match component {
            Component::Normal(segment) => Some(*segment),
            _ => None,
        })
        .ok_or_else(|| StoreError::new("materialization file name is invalid"))?;
    reject_existing_final_symlink(&directory, final_name)
}

#[cfg(not(unix))]
fn prepare_materialization_path_impl(
    _output_root: &Path,
    _relative_path: &str,
) -> Result<(), StoreError> {
    Err(StoreError::new(
        "descriptor-relative materialization is unavailable on this platform",
    ))
}

/// Atomically publish checksum-bound bytes to an exact output path.
///
/// Unix publication is entirely descriptor-relative:
///
/// 1. Open/create every parent component with `O_NOFOLLOW`.
/// 2. Create a private `O_EXCL` temporary file in the destination directory.
/// 3. Write, restore mode, `fsync`, rewind, and checksum the temporary file.
/// 4. Publish with `renameat2(RENAME_NOREPLACE)`.
/// 5. `fsync` the destination directory.
///
/// The destination is never overwritten. Any checksum, write, permission,
/// rename, or durability failure removes the unpublished temporary file (or the
/// just-published file when directory durability fails) before returning.
pub fn atomic_materialize_file(
    output_path: &Path,
    bytes: &[u8],
    expected_sha256: &str,
    unix_mode: Option<u32>,
) -> Result<MaterializedFile, StoreError> {
    let actual_sha256 = format!("{:x}", Sha256::digest(bytes));
    if actual_sha256 != expected_sha256 {
        return Err(StoreError::new(format!(
            "materialization checksum mismatch before publication: expected {expected_sha256}, observed {actual_sha256}"
        )));
    }
    atomic_materialize_file_impl(
        output_path,
        bytes,
        expected_sha256,
        unix_mode,
        &actual_sha256,
    )
}

/// Take the exact rollback authority retained by the most recent successful
/// publication to `output_path`.
///
/// Backends all publish through [`atomic_materialize_file`], so this preserves
/// backend neutrality without changing the `BlobStore` materialization report.
pub fn take_materialized_file_rollback(
    output_path: &Path,
) -> Result<MaterializedFileRollback, StoreError> {
    take_materialized_file_rollback_impl(output_path)
}

#[cfg(target_os = "linux")]
fn take_materialized_file_rollback_impl(
    output_path: &Path,
) -> Result<MaterializedFileRollback, StoreError> {
    retained_materialization_rollbacks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(output_path)
        .ok_or_else(|| {
            StoreError::new(format!(
                "materialization rollback identity is unavailable for {}",
                output_path.display()
            ))
        })
}

#[cfg(not(target_os = "linux"))]
fn take_materialized_file_rollback_impl(
    _output_path: &Path,
) -> Result<MaterializedFileRollback, StoreError> {
    Err(StoreError::new(
        "identity-bound materialization rollback is unavailable on this platform",
    ))
}

/// Roll back only the exact entry published by this attempt.
///
/// The current destination is first moved to a private name within the bound
/// parent. It is deleted only after its device/inode identity matches. A
/// replacement is restored and reported as an explicit conflict/residual.
pub fn rollback_materialized_file(rollback: MaterializedFileRollback) -> Result<(), StoreError> {
    rollback_materialized_file_impl(rollback)
}

#[cfg(target_os = "linux")]
fn rollback_materialized_file_impl(rollback: MaterializedFileRollback) -> Result<(), StoreError> {
    use rustix::fs::{AtFlags, RenameFlags};

    let parent_identity = rustix::fs::fstat(&rollback.parent).map_err(|error| {
        StoreError::new(format!(
            "inspect bound materialization rollback parent failed: {error}"
        ))
    })?;
    if parent_identity.st_dev != rollback.parent_device
        || parent_identity.st_ino != rollback.parent_inode
    {
        return Err(StoreError::new(format!(
            "materialization rollback identity conflict: bound parent changed for {}; residual preserved",
            rollback.path.display()
        )));
    }

    static ROLLBACK_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let quarantine = loop {
        let sequence = ROLLBACK_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = OsString::from(format!(
            ".codedb-rollback-{}-{sequence}.tmp",
            std::process::id()
        ));
        match rustix::fs::renameat_with(
            &rollback.parent,
            &rollback.final_name,
            &rollback.parent,
            &candidate,
            RenameFlags::NOREPLACE,
        ) {
            Ok(()) => break candidate,
            Err(error) if error == rustix::io::Errno::EXIST => continue,
            Err(error) if error == rustix::io::Errno::NOENT => {
                return Err(StoreError::new(format!(
                    "materialization rollback identity conflict: published entry is missing for {}; residual state requires audit",
                    rollback.path.display()
                )));
            }
            Err(error) => {
                return Err(StoreError::new(format!(
                    "materialization rollback could not isolate {} for identity verification: {error}; residual state requires audit",
                    rollback.path.display()
                )));
            }
        }
    };

    let observed = rustix::fs::statat(&rollback.parent, &quarantine, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|error| {
            StoreError::new(format!(
                "inspect isolated materialization rollback entry failed: {error}; residual {} requires audit",
                quarantine.to_string_lossy()
            ))
        })?;
    if observed.st_dev == rollback.file_device && observed.st_ino == rollback.file_inode {
        rustix::fs::unlinkat(&rollback.parent, &quarantine, AtFlags::empty()).map_err(
            |error| {
                StoreError::new(format!(
                    "remove identity-matched materialization rollback entry failed: {error}; residual {} requires audit",
                    quarantine.to_string_lossy()
                ))
            },
        )?;
        rustix::fs::fsync(&rollback.parent).map_err(|error| {
            StoreError::new(format!(
                "fsync identity-bound materialization rollback directory failed: {error}"
            ))
        })?;
        return Ok(());
    }

    match rustix::fs::renameat_with(
        &rollback.parent,
        &quarantine,
        &rollback.parent,
        &rollback.final_name,
        RenameFlags::NOREPLACE,
    ) {
        Ok(()) => {
            let _ = rustix::fs::fsync(&rollback.parent);
            Err(StoreError::new(format!(
                "materialization rollback identity conflict for {}: expected device/inode {}/{}, observed {}/{}; replacement preserved and residual requires audit",
                rollback.path.display(),
                rollback.file_device,
                rollback.file_inode,
                observed.st_dev,
                observed.st_ino
            )))
        }
        Err(error) => Err(StoreError::new(format!(
            "materialization rollback identity conflict for {} and replacement restore failed: {error}; replacement retained as residual {} for audit",
            rollback.path.display(),
            quarantine.to_string_lossy()
        ))),
    }
}

#[cfg(not(target_os = "linux"))]
fn rollback_materialized_file_impl(_rollback: MaterializedFileRollback) -> Result<(), StoreError> {
    Err(StoreError::new(
        "identity-bound materialization rollback is unavailable on this platform",
    ))
}

/// Remove a file previously published by [`atomic_materialize_file`] without
/// reopening any symlinked ancestor.
///
/// This legacy path-only cleanup API cannot prove publication identity. Batch
/// rollback must use [`take_materialized_file_rollback`] followed by
/// [`rollback_materialized_file`].
pub fn remove_materialized_file(output_path: &Path) -> Result<(), StoreError> {
    remove_materialized_file_impl(output_path)
}

#[cfg(target_os = "linux")]
fn remove_materialized_file_impl(output_path: &Path) -> Result<(), StoreError> {
    use rustix::fs::AtFlags;

    let parent_path = output_path
        .parent()
        .ok_or_else(|| StoreError::new("materialization cleanup path has no parent"))?;
    let final_name = output_path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| StoreError::new("materialization cleanup path has no file name"))?;
    let parent = open_existing_directory_chain(parent_path)?;
    match rustix::fs::unlinkat(&parent, final_name, AtFlags::empty()) {
        Ok(()) => {}
        Err(error) if error == rustix::io::Errno::NOENT => return Ok(()),
        Err(error) => {
            return Err(StoreError::new(format!(
                "descriptor-relative materialization cleanup failed: {error}"
            )));
        }
    }
    rustix::fs::fsync(&parent).map_err(|error| {
        StoreError::new(format!(
            "fsync materialization cleanup directory failed: {error}"
        ))
    })
}

#[cfg(not(target_os = "linux"))]
fn remove_materialized_file_impl(_output_path: &Path) -> Result<(), StoreError> {
    Err(StoreError::new(
        "descriptor-relative materialization cleanup is unavailable on this platform",
    ))
}

#[cfg(unix)]
fn atomic_materialize_file_impl(
    output_path: &Path,
    bytes: &[u8],
    expected_sha256: &str,
    unix_mode: Option<u32>,
    actual_sha256: &str,
) -> Result<MaterializedFile, StoreError> {
    use rustix::fs::{AtFlags, Mode, OFlags, RenameFlags};

    let parent_path = output_path
        .parent()
        .ok_or_else(|| StoreError::new("materialization output has no parent directory"))?;
    let final_name = output_path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| StoreError::new("materialization output has no file name"))?;
    let parent = open_or_create_directory_chain(parent_path)?;
    reject_existing_final_symlink(&parent, final_name)?;

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let mut temp_name = None;
    let mut temp_fd = None;
    for _ in 0..128 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = OsString::from(format!(
            ".codedb-materialize-{}-{sequence}.tmp",
            std::process::id()
        ));
        match rustix::fs::openat(
            &parent,
            &candidate,
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from(0o600),
        ) {
            Ok(fd) => {
                temp_name = Some(candidate);
                temp_fd = Some(fd);
                break;
            }
            Err(error) if error == rustix::io::Errno::EXIST => continue,
            Err(error) => {
                return Err(StoreError::new(format!(
                    "create descriptor-relative materialization temporary file: {error}"
                )));
            }
        }
    }
    let temp_name = temp_name
        .ok_or_else(|| StoreError::new("could not allocate materialization temporary file"))?;
    let mut file =
        File::from(temp_fd.expect("temporary descriptor is present when its name is present"));
    let mut published = false;
    #[cfg(target_os = "linux")]
    let mut rollback_identity = None;
    let result = (|| -> Result<(), StoreError> {
        file.write_all(bytes).map_err(|error| {
            StoreError::new(format!("write materialization temporary: {error}"))
        })?;
        if let Some(mode) = unix_mode {
            rustix::fs::fchmod(&file, Mode::from(mode & 0o7777)).map_err(|error| {
                StoreError::new(format!("restore materialization mode: {error}"))
            })?;
        }
        file.sync_all().map_err(|error| {
            StoreError::new(format!("fsync materialization temporary: {error}"))
        })?;
        file.seek(SeekFrom::Start(0)).map_err(|error| {
            StoreError::new(format!("rewind materialization temporary: {error}"))
        })?;
        let mut persisted = Vec::with_capacity(bytes.len());
        file.read_to_end(&mut persisted).map_err(|error| {
            StoreError::new(format!("re-read materialization temporary: {error}"))
        })?;
        let persisted_sha256 = format!("{:x}", Sha256::digest(&persisted));
        if persisted_sha256 != expected_sha256 {
            return Err(StoreError::new(format!(
                "materialization checksum mismatch before atomic publication: expected {expected_sha256}, observed {persisted_sha256}"
            )));
        }
        #[cfg(target_os = "linux")]
        {
            let file_identity = rustix::fs::fstat(&file).map_err(|error| {
                StoreError::new(format!(
                    "inspect materialization publication identity failed: {error}"
                ))
            })?;
            let parent_identity = rustix::fs::fstat(&parent).map_err(|error| {
                StoreError::new(format!(
                    "inspect materialization destination parent identity failed: {error}"
                ))
            })?;
            rollback_identity = Some(MaterializedFileRollback {
                path: output_path.to_path_buf(),
                parent: rustix::io::dup(&parent).map_err(|error| {
                    StoreError::new(format!(
                        "retain bound materialization destination parent failed: {error}"
                    ))
                })?,
                final_name: final_name.to_os_string(),
                parent_device: parent_identity.st_dev,
                parent_inode: parent_identity.st_ino,
                file_device: file_identity.st_dev,
                file_inode: file_identity.st_ino,
            });
        }
        drop(file);
        rustix::fs::renameat_with(
            &parent,
            &temp_name,
            &parent,
            final_name,
            RenameFlags::NOREPLACE,
        )
        .map_err(|error| {
            if error == rustix::io::Errno::EXIST {
                StoreError::new(format!(
                    "materialization destination exists; no-replace publication refused: {}",
                    output_path.display()
                ))
            } else {
                StoreError::new(format!(
                    "atomic no-replace materialization publication failed: {error}"
                ))
            }
        })?;
        published = true;
        rustix::fs::fsync(&parent).map_err(|error| {
            StoreError::new(format!(
                "fsync materialization destination directory failed: {error}"
            ))
        })?;
        Ok(())
    })();

    if let Err(error) = result {
        let cleanup = if published {
            #[cfg(target_os = "linux")]
            {
                rollback_identity
                    .take()
                    .map(rollback_materialized_file)
                    .transpose()
                    .map(|_| ())
            }
            #[cfg(not(target_os = "linux"))]
            {
                rustix::fs::unlinkat(&parent, final_name, AtFlags::empty())
                    .map_err(|cleanup_error| StoreError::new(cleanup_error.to_string()))
            }
        } else {
            rustix::fs::unlinkat(&parent, &temp_name, AtFlags::empty())
                .map_err(|cleanup_error| StoreError::new(cleanup_error.to_string()))
        };
        let _ = rustix::fs::fsync(&parent);
        if let Err(cleanup_error) = cleanup {
            return Err(StoreError::new(format!(
                "{error}; atomic publication rollback conflict/residual audit: {cleanup_error}"
            )));
        }
        return Err(error);
    }

    #[cfg(target_os = "linux")]
    retained_materialization_rollbacks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(
            output_path.to_path_buf(),
            rollback_identity.expect("successful Linux publication retains rollback identity"),
        );

    Ok(MaterializedFile {
        path: output_path.to_path_buf(),
        blob_ref: format!("sha256:{actual_sha256}"),
        sha256: actual_sha256.to_string(),
        bytes: bytes.len() as u64,
    })
}

#[cfg(not(unix))]
fn atomic_materialize_file_impl(
    _output_path: &Path,
    _bytes: &[u8],
    _expected_sha256: &str,
    _unix_mode: Option<u32>,
    _actual_sha256: &str,
) -> Result<MaterializedFile, StoreError> {
    Err(StoreError::new(
        "atomic descriptor-relative materialization is unavailable on this platform",
    ))
}

#[cfg(unix)]
fn open_or_create_directory_chain(path: &Path) -> Result<rustix::fd::OwnedFd, StoreError> {
    use rustix::fs::{Mode, OFlags};

    let start = if path.is_absolute() { "/" } else { "." };
    let mut current = rustix::fs::open(
        start,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| {
        StoreError::new(format!(
            "open descriptor-relative materialization anchor {start}: {error}"
        ))
    })?;

    for component in path.components() {
        match component {
            Component::RootDir if path.is_absolute() => {}
            Component::CurDir => {}
            Component::Normal(segment) => {
                current = open_or_create_child_directory(&current, segment)?;
            }
            _ => {
                return Err(StoreError::new(format!(
                    "non-normal descriptor-relative directory component is forbidden: {}",
                    path.display()
                )));
            }
        }
    }
    Ok(current)
}

#[cfg(unix)]
fn open_existing_directory_chain(path: &Path) -> Result<rustix::fd::OwnedFd, StoreError> {
    use rustix::fs::{Mode, OFlags};

    let start = if path.is_absolute() { "/" } else { "." };
    let mut current = rustix::fs::open(
        start,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| {
        StoreError::new(format!(
            "open descriptor-relative containment anchor {start}: {error}"
        ))
    })?;
    for component in path.components() {
        match component {
            Component::RootDir if path.is_absolute() => {}
            Component::CurDir => {}
            Component::Normal(segment) => {
                current = openat_beneath_no_symlinks(
                    &current,
                    segment,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                )
                .map_err(|error| {
                    StoreError::new(format!(
                        "open contained directory component {segment:?}: {error}"
                    ))
                })?;
            }
            _ => {
                return Err(StoreError::new(format!(
                    "non-normal contained directory component is forbidden: {}",
                    path.display()
                )));
            }
        }
    }
    Ok(current)
}

#[cfg(unix)]
fn open_contained_directory(path: &Path) -> Result<ContainedDirectory, StoreError> {
    Ok(ContainedDirectory {
        descriptor: open_existing_directory_chain(path)?,
    })
}

#[cfg(not(unix))]
fn open_contained_directory(_path: &Path) -> Result<ContainedDirectory, StoreError> {
    Err(StoreError::new(
        "descriptor-relative contained reads are unavailable on this platform",
    ))
}

#[cfg(target_os = "linux")]
fn open_contained_regular_file(
    root: &ContainedDirectory,
    relative_path: &str,
) -> Result<ContainedRegularFileHandle, StoreError> {
    use rustix::fs::{Mode, OFlags, ResolveFlags};
    use std::os::unix::fs::PermissionsExt;

    // Reuse the portable grammar without ever joining/reopening the result.
    safe_materialization_path(Path::new(""), relative_path)?;
    let descriptor = rustix::fs::openat2(
        &root.descriptor,
        relative_path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
    )
    .map_err(|error| {
        StoreError::new(format!(
            "contained regular-file open refused {relative_path:?}: {error}"
        ))
    })?;
    let file = File::from(descriptor);
    let before = file.metadata().map_err(|error| {
        StoreError::new(format!(
            "inspect contained regular file {relative_path:?}: {error}"
        ))
    })?;
    if !before.is_file() {
        return Err(StoreError::new(format!(
            "contained path is not a regular file: {relative_path:?}"
        )));
    }
    Ok(ContainedRegularFileHandle {
        file,
        unix_mode: Some(before.permissions().mode() & 0o7777),
    })
}

#[cfg(target_os = "linux")]
fn read_all_from_contained_handle(
    handle: &mut ContainedRegularFileHandle,
) -> Result<ContainedRegularFile, StoreError> {
    let before = handle
        .file
        .metadata()
        .map_err(|error| StoreError::new(format!("inspect contained regular file: {error}")))?;
    let mut bytes = Vec::with_capacity(usize::try_from(before.len()).unwrap_or(0));
    handle
        .file
        .read_to_end(&mut bytes)
        .map_err(|error| StoreError::new(format!("read contained regular file: {error}")))?;
    let after = handle
        .file
        .metadata()
        .map_err(|error| StoreError::new(format!("reinspect contained regular file: {error}")))?;
    if before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || bytes.len() as u64 != after.len()
    {
        return Err(StoreError::new("contained source changed during capture"));
    }
    Ok(ContainedRegularFile {
        bytes,
        unix_mode: handle.unix_mode,
    })
}

#[cfg(all(unix, not(target_os = "linux")))]
fn open_contained_regular_file(
    _root: &ContainedDirectory,
    _relative_path: &str,
) -> Result<ContainedRegularFileHandle, StoreError> {
    Err(StoreError::new(
        "openat2 contained reads are unavailable on this platform",
    ))
}

#[cfg(not(unix))]
fn open_contained_regular_file(
    _root: &ContainedDirectory,
    _relative_path: &str,
) -> Result<ContainedRegularFileHandle, StoreError> {
    Err(StoreError::new(
        "descriptor-relative contained reads are unavailable on this platform",
    ))
}

#[cfg(not(target_os = "linux"))]
fn read_all_from_contained_handle(
    _handle: &mut ContainedRegularFileHandle,
) -> Result<ContainedRegularFile, StoreError> {
    Err(StoreError::new(
        "descriptor-relative contained reads are unavailable on this platform",
    ))
}

#[cfg(unix)]
fn open_or_create_child_directory(
    parent: &rustix::fd::OwnedFd,
    component: &OsStr,
) -> Result<rustix::fd::OwnedFd, StoreError> {
    use rustix::fs::{Mode, OFlags};

    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    match openat_beneath_no_symlinks(parent, component, flags) {
        Ok(directory) => Ok(directory),
        Err(error) if error == rustix::io::Errno::NOENT => {
            match rustix::fs::mkdirat(parent, component, Mode::from(0o700)) {
                Ok(()) => {}
                Err(error) if error == rustix::io::Errno::EXIST => {}
                Err(error) => {
                    return Err(StoreError::new(format!(
                        "create descriptor-relative materialization directory {component:?}: {error}"
                    )));
                }
            }
            openat_beneath_no_symlinks(parent, component, flags).map_err(|error| {
                StoreError::new(format!(
                    "descriptor-relative directory open refused {component:?} (symlink or non-directory): {error}"
                ))
            })
        }
        Err(error) => Err(StoreError::new(format!(
            "descriptor-relative directory open refused {component:?} (symlink or non-directory): {error}"
        ))),
    }
}

#[cfg(unix)]
fn reject_existing_final_symlink(
    parent: &rustix::fd::OwnedFd,
    final_name: &OsStr,
) -> Result<(), StoreError> {
    use rustix::fs::OFlags;

    match openat_beneath_no_symlinks(
        parent,
        final_name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
    ) {
        Ok(_) => Ok(()),
        Err(error) if error == rustix::io::Errno::NOENT => Ok(()),
        Err(error) => Err(StoreError::new(format!(
            "descriptor-relative final-path inspection refused {final_name:?} (symlink or inaccessible): {error}"
        ))),
    }
}

#[cfg(target_os = "linux")]
fn openat_beneath_no_symlinks(
    parent: &rustix::fd::OwnedFd,
    component: &OsStr,
    flags: rustix::fs::OFlags,
) -> rustix::io::Result<rustix::fd::OwnedFd> {
    use rustix::fs::{Mode, ResolveFlags};

    rustix::fs::openat2(
        parent,
        component,
        flags,
        Mode::empty(),
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
    )
}

#[cfg(all(unix, not(target_os = "linux")))]
fn openat_beneath_no_symlinks(
    _parent: &rustix::fd::OwnedFd,
    _component: &OsStr,
    _flags: rustix::fs::OFlags,
) -> rustix::io::Result<rustix::fd::OwnedFd> {
    // `openat2`-equivalent containment is not proved on this target. Materialization
    // therefore fails closed rather than silently falling back to pathname checks.
    Err(rustix::io::Errno::NOSYS)
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
