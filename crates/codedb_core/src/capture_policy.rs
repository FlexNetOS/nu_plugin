//! Backend-neutral authorization for persisting raw source bytes.
//!
//! Secret classification is a deny-only guard. A clear classifier result does
//! not authorize persistence: the caller must also select a core-owned safe
//! source policy or load an operator policy from outside the repository.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error as StdError;
use std::fmt::{Debug, Display, Formatter};
#[cfg(target_os = "linux")]
use std::fs::{self, File};
use std::io;
#[cfg(target_os = "linux")]
use std::io::Read;
#[cfg(target_os = "linux")]
use std::path::Component;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::{SecretClassificationStatus, SecretEvidenceKind, classify_source_secret};

pub const EXTERNAL_POLICY_VERSION: &str = "codedb.raw-persistence-policy.v1";

const MAX_EXTERNAL_POLICY_BYTES: u64 = 64 * 1024;
const DEFAULT_DENY_POLICY: &str = "\
version=codedb.raw-persistence-policy.v1\n\
policy_id=codedb-default-deny-v1\n\
authority=codedb-core\n\
allow=\n";
const BUILT_IN_SAFE_SOURCE_POLICY: &str = "\
version=codedb.raw-persistence-policy.v1\n\
policy_id=codedb-safe-source-classes-v1\n\
authority=codedb-core\n\
allow=source-code,documentation\n\
hard_deny=configuration,sensitive,unknown,classifier-secret,classifier-uncertain\n";

/// Deterministic race boundaries for security regression tests.
///
/// This is not an authorization extension point. Production callers should use
/// [`load_external_policy`], which executes the same loader without a hook.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalPolicyLoadStage {
    RootsOpened,
    PolicyOpened,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceClass {
    SourceCode,
    Documentation,
    Configuration,
    Sensitive,
    Unknown,
}

impl SourceClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceCode => "source-code",
            Self::Documentation => "documentation",
            Self::Configuration => "configuration",
            Self::Sensitive => "sensitive",
            Self::Unknown => "unknown",
        }
    }

    const fn is_core_safe(self) -> bool {
        matches!(self, Self::SourceCode | Self::Documentation)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawPersistenceDisposition {
    PersistRaw,
    MetadataOnly,
}

impl RawPersistenceDisposition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PersistRaw => "persist-raw",
            Self::MetadataOnly => "metadata-only",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawPersistenceReason {
    AuthorizedSafeSourceClass,
    AuthorizedExternalPolicy,
    MissingAuthorization,
    ClassifierSecretDetected,
    ClassifierUncertain,
    HardDeniedSourceClass,
    ExternalPolicyClassNotAllowed,
    RepositoryBindingMismatch,
}

impl RawPersistenceReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthorizedSafeSourceClass => "authorized-safe-source-class",
            Self::AuthorizedExternalPolicy => "authorized-external-policy",
            Self::MissingAuthorization => "missing-authorization",
            Self::ClassifierSecretDetected => "classifier-secret-detected",
            Self::ClassifierUncertain => "classifier-uncertain",
            Self::HardDeniedSourceClass => "hard-denied-source-class",
            Self::ExternalPolicyClassNotAllowed => "external-policy-class-not-allowed",
            Self::RepositoryBindingMismatch => "repository-binding-mismatch",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyAuthoritySource {
    DefaultDeny,
    BuiltInSafeSourceClasses,
    ExternalOperatorPolicy,
}

impl PolicyAuthoritySource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DefaultDeny => "default-deny",
            Self::BuiltInSafeSourceClasses => "built-in-safe-source-classes",
            Self::ExternalOperatorPolicy => "external-operator-policy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyProvenance {
    pub policy_id: String,
    pub policy_digest: String,
    pub binding_digest: String,
    pub authority: String,
    pub authority_source: PolicyAuthoritySource,
    pub repository_binding: String,
    pub external_policy_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactSourceRequirement {
    pub relative_path: String,
    pub byte_len: u64,
    pub sha256: String,
}

impl ExactSourceRequirement {
    /// Verify operator-supplied source bytes without persisting or formatting
    /// them. The returned value borrows the caller's bytes and is safe to pass
    /// directly to a contained materialization operation.
    pub fn verify<'a>(
        &self,
        bytes: &'a [u8],
    ) -> Result<VerifiedExactSource<'a>, ExactSourceVerificationError> {
        let actual_len = bytes.len() as u64;
        if actual_len != self.byte_len {
            return Err(ExactSourceVerificationError::LengthMismatch {
                relative_path: self.relative_path.clone(),
                expected: self.byte_len,
                actual: actual_len,
            });
        }
        let actual_sha256 = hex_sha256(bytes);
        if actual_sha256 != self.sha256 {
            return Err(ExactSourceVerificationError::DigestMismatch {
                relative_path: self.relative_path.clone(),
                expected_sha256: self.sha256.clone(),
                actual_sha256,
            });
        }
        Ok(VerifiedExactSource {
            bytes,
            sha256: self.sha256.clone(),
        })
    }
}

pub struct VerifiedExactSource<'a> {
    bytes: &'a [u8],
    sha256: String,
}

impl<'a> VerifiedExactSource<'a> {
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

impl Debug for VerifiedExactSource<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedExactSource")
            .field("byte_len", &self.bytes.len())
            .field("sha256", &self.sha256)
            .field("bytes", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExactSourceVerificationError {
    LengthMismatch {
        relative_path: String,
        expected: u64,
        actual: u64,
    },
    DigestMismatch {
        relative_path: String,
        expected_sha256: String,
        actual_sha256: String,
    },
}

impl Display for ExactSourceVerificationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LengthMismatch {
                relative_path,
                expected,
                actual,
            } => write!(
                formatter,
                "exact source length mismatch for {relative_path}: expected {expected}, got {actual}"
            ),
            Self::DigestMismatch {
                relative_path,
                expected_sha256,
                actual_sha256,
            } => write!(
                formatter,
                "exact source digest mismatch for {relative_path}: expected {expected_sha256}, got {actual_sha256}"
            ),
        }
    }
}

impl StdError for ExactSourceVerificationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPersistenceDecision {
    pub disposition: RawPersistenceDisposition,
    pub reason: RawPersistenceReason,
    pub source_class: SourceClass,
    pub classifier_status: SecretClassificationStatus,
    pub classifier_evidence: Vec<SecretEvidenceKind>,
    pub policy: PolicyProvenance,
    pub exact_source: ExactSourceRequirement,
}

impl RawPersistenceDecision {
    pub const fn raw_persistence_allowed(&self) -> bool {
        matches!(self.disposition, RawPersistenceDisposition::PersistRaw)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawPersistenceAuthorization {
    BuiltInSafeSourceClasses,
    External(ExternalPolicyBinding),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalPolicyBinding {
    policy_id: String,
    authority: String,
    repository_binding: String,
    policy_digest: String,
    binding_digest: String,
    policy_path: PathBuf,
    allowed_classes: BTreeSet<SourceClass>,
}

impl ExternalPolicyBinding {
    pub fn policy_id(&self) -> &str {
        &self.policy_id
    }

    pub fn authority(&self) -> &str {
        &self.authority
    }

    pub fn repository_binding(&self) -> &str {
        &self.repository_binding
    }

    pub fn policy_digest(&self) -> &str {
        &self.policy_digest
    }

    pub fn binding_digest(&self) -> &str {
        &self.binding_digest
    }

    pub fn policy_path(&self) -> &Path {
        &self.policy_path
    }

    pub fn allows(&self, source_class: SourceClass) -> bool {
        self.allowed_classes.contains(&source_class)
    }
}

#[derive(Debug)]
pub enum CapturePolicyError {
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    PolicyPathMustBeAbsolute {
        path: PathBuf,
    },
    PolicyPathIsSymlink {
        path: PathBuf,
    },
    PolicyPathIsNotRegularFile {
        path: PathBuf,
    },
    RepositoryControlledPolicy {
        path: PathBuf,
    },
    PolicyDocumentTooLarge {
        path: PathBuf,
        bytes: u64,
        maximum: u64,
    },
    InvalidPolicyDocument {
        reason: String,
    },
    HardDeniedClassInPolicy {
        class: String,
    },
    InvalidRepositoryBinding,
    RepositoryBindingMismatch {
        expected: String,
        actual: String,
    },
}

impl Display for CapturePolicyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                formatter,
                "{operation} capture policy {}: {source}",
                path.display()
            ),
            Self::PolicyPathMustBeAbsolute { path } => {
                write!(
                    formatter,
                    "capture policy path must be absolute: {}",
                    path.display()
                )
            }
            Self::PolicyPathIsSymlink { path } => {
                write!(
                    formatter,
                    "capture policy path must not be a symlink: {}",
                    path.display()
                )
            }
            Self::PolicyPathIsNotRegularFile { path } => write!(
                formatter,
                "capture policy path must be a regular file: {}",
                path.display()
            ),
            Self::RepositoryControlledPolicy { path } => write!(
                formatter,
                "capture policy must be external to the repository: {}",
                path.display()
            ),
            Self::PolicyDocumentTooLarge {
                path,
                bytes,
                maximum,
            } => write!(
                formatter,
                "capture policy {} is {bytes} bytes; maximum is {maximum}",
                path.display()
            ),
            Self::InvalidPolicyDocument { reason } => {
                write!(formatter, "invalid capture policy document: {reason}")
            }
            Self::HardDeniedClassInPolicy { class } => write!(
                formatter,
                "capture policy cannot authorize hard-denied source class {class}"
            ),
            Self::InvalidRepositoryBinding => write!(
                formatter,
                "capture policy repository_binding must be a sha256 digest"
            ),
            Self::RepositoryBindingMismatch { expected, actual } => write!(
                formatter,
                "capture policy repository binding mismatch: expected {expected}, got {actual}"
            ),
        }
    }
}

impl StdError for CapturePolicyError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Load and validate an operator policy that cannot be controlled by files in
/// the repository being captured.
///
/// The policy document is a small, fail-closed `key=value` format:
///
/// ```text
/// version=codedb.raw-persistence-policy.v1
/// policy_id=operator-reviewed-source
/// authority=operator:local-user
/// repository_binding=sha256:source-snapshot
/// allow=source-code,documentation
/// ```
pub fn load_external_policy(
    repository_root: impl AsRef<Path>,
    policy_path: impl AsRef<Path>,
    expected_repository_binding: &str,
) -> Result<ExternalPolicyBinding, CapturePolicyError> {
    load_external_policy_with_hook(
        repository_root,
        policy_path,
        expected_repository_binding,
        |_| {},
    )
}

/// Testable form of [`load_external_policy`] with hooks at held-descriptor race
/// boundaries.
#[doc(hidden)]
pub fn load_external_policy_with_hook(
    repository_root: impl AsRef<Path>,
    policy_path: impl AsRef<Path>,
    expected_repository_binding: &str,
    mut hook: impl FnMut(ExternalPolicyLoadStage),
) -> Result<ExternalPolicyBinding, CapturePolicyError> {
    let repository_root = repository_root.as_ref();
    let policy_path = policy_path.as_ref();
    if !policy_path.is_absolute() {
        return Err(CapturePolicyError::PolicyPathMustBeAbsolute {
            path: policy_path.to_path_buf(),
        });
    }

    let (document, held_policy_path) =
        read_external_policy_from_held_handle(repository_root, policy_path, &mut hook)?;
    let fields = parse_policy_document(&document)?;
    let version = required_field(&fields, "version")?;
    if version != EXTERNAL_POLICY_VERSION {
        return Err(CapturePolicyError::InvalidPolicyDocument {
            reason: format!("unsupported version {version}"),
        });
    }
    let policy_id = required_field(&fields, "policy_id")?.to_string();
    let authority = required_field(&fields, "authority")?.to_string();
    let repository_binding = required_field(&fields, "repository_binding")?.to_string();
    validate_public_identifier("policy_id", &policy_id)?;
    validate_public_identifier("authority", &authority)?;
    if !is_sha256_binding(expected_repository_binding) || !is_sha256_binding(&repository_binding) {
        return Err(CapturePolicyError::InvalidRepositoryBinding);
    }
    if repository_binding != expected_repository_binding {
        return Err(CapturePolicyError::RepositoryBindingMismatch {
            expected: expected_repository_binding.to_string(),
            actual: repository_binding,
        });
    }
    let allowed_classes = parse_allowed_classes(required_field(&fields, "allow")?)?;
    if allowed_classes.is_empty() {
        return Err(CapturePolicyError::InvalidPolicyDocument {
            reason: "allow must contain at least one core-safe source class".to_string(),
        });
    }

    let policy_digest = prefixed_sha256(&document);
    let binding_digest = policy_binding_digest(
        &policy_digest,
        &policy_id,
        &authority,
        &repository_binding,
        PolicyAuthoritySource::ExternalOperatorPolicy,
    );
    Ok(ExternalPolicyBinding {
        policy_id,
        authority,
        repository_binding,
        policy_digest,
        binding_digest,
        policy_path: held_policy_path,
        allowed_classes,
    })
}

#[cfg(target_os = "linux")]
fn read_external_policy_from_held_handle(
    repository_root: &Path,
    policy_path: &Path,
    hook: &mut impl FnMut(ExternalPolicyLoadStage),
) -> Result<(Vec<u8>, PathBuf), CapturePolicyError> {
    use rustix::fs::{Mode, OFlags, ResolveFlags};
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;

    let repository_root =
        absolute_loader_path(repository_root).map_err(|source| CapturePolicyError::Io {
            operation: "resolving repository root for",
            path: repository_root.to_path_buf(),
            source,
        })?;
    let policy_parent = policy_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| CapturePolicyError::Io {
            operation: "resolving parent of",
            path: policy_path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "policy path has no parent"),
        })?;
    let policy_name = policy_path
        .file_name()
        .ok_or_else(|| CapturePolicyError::Io {
            operation: "resolving final component of",
            path: policy_path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "policy path has no final component",
            ),
        })?;

    let repository_descriptor =
        open_absolute_directory_no_symlinks(&repository_root).map_err(|source| {
            path_open_error(
                "opening repository root for",
                repository_root.clone(),
                source,
            )
        })?;
    let policy_parent_descriptor =
        open_absolute_directory_no_symlinks(policy_parent).map_err(|source| {
            path_open_error(
                "opening policy parent for",
                policy_parent.to_path_buf(),
                source,
            )
        })?;
    hook(ExternalPolicyLoadStage::RootsOpened);

    let policy_descriptor = rustix::fs::openat2(
        &policy_parent_descriptor,
        policy_name,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
    )
    .map_err(|error| {
        let source = errno_to_io(error);
        if source.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error()) {
            CapturePolicyError::PolicyPathIsSymlink {
                path: policy_path.to_path_buf(),
            }
        } else if source.raw_os_error() == Some(rustix::io::Errno::NXIO.raw_os_error()) {
            CapturePolicyError::PolicyPathIsNotRegularFile {
                path: policy_path.to_path_buf(),
            }
        } else {
            CapturePolicyError::Io {
                operation: "opening",
                path: policy_path.to_path_buf(),
                source,
            }
        }
    })?;
    let mut policy_file = File::from(policy_descriptor);

    let held_repository_path =
        held_descriptor_path(repository_descriptor.as_raw_fd()).map_err(|source| {
            CapturePolicyError::Io {
                operation: "proving held repository path for",
                path: repository_root.clone(),
                source,
            }
        })?;
    let held_policy_path =
        held_descriptor_path(policy_file.as_raw_fd()).map_err(|source| CapturePolicyError::Io {
            operation: "proving held policy path for",
            path: policy_path.to_path_buf(),
            source,
        })?;
    if held_policy_path.starts_with(&held_repository_path) {
        return Err(CapturePolicyError::RepositoryControlledPolicy {
            path: held_policy_path,
        });
    }

    hook(ExternalPolicyLoadStage::PolicyOpened);
    let before = policy_file
        .metadata()
        .map_err(|source| CapturePolicyError::Io {
            operation: "inspecting held",
            path: held_policy_path.clone(),
            source,
        })?;
    if !before.file_type().is_file() {
        return Err(CapturePolicyError::PolicyPathIsNotRegularFile {
            path: held_policy_path,
        });
    }
    if before.len() > MAX_EXTERNAL_POLICY_BYTES {
        return Err(CapturePolicyError::PolicyDocumentTooLarge {
            path: held_policy_path,
            bytes: before.len(),
            maximum: MAX_EXTERNAL_POLICY_BYTES,
        });
    }

    let mut document = Vec::with_capacity(before.len() as usize);
    (&mut policy_file)
        .take(MAX_EXTERNAL_POLICY_BYTES + 1)
        .read_to_end(&mut document)
        .map_err(|source| CapturePolicyError::Io {
            operation: "reading held",
            path: held_policy_path.clone(),
            source,
        })?;
    if document.len() as u64 > MAX_EXTERNAL_POLICY_BYTES {
        return Err(CapturePolicyError::PolicyDocumentTooLarge {
            path: held_policy_path,
            bytes: document.len() as u64,
            maximum: MAX_EXTERNAL_POLICY_BYTES,
        });
    }
    let after = policy_file
        .metadata()
        .map_err(|source| CapturePolicyError::Io {
            operation: "reinspecting held",
            path: held_policy_path.clone(),
            source,
        })?;
    if before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || document.len() as u64 != after.len()
    {
        return Err(CapturePolicyError::Io {
            operation: "reading stable held",
            path: held_policy_path,
            source: io::Error::new(
                io::ErrorKind::InvalidData,
                "capture policy changed while held for reading",
            ),
        });
    }

    Ok((document, held_policy_path))
}

#[cfg(not(target_os = "linux"))]
fn read_external_policy_from_held_handle(
    repository_root: &Path,
    policy_path: &Path,
    _hook: &mut impl FnMut(ExternalPolicyLoadStage),
) -> Result<(Vec<u8>, PathBuf), CapturePolicyError> {
    Err(CapturePolicyError::Io {
        operation: "opening descriptor-relative policy for",
        path: policy_path.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "race-resistant external policy containment is unavailable for repository {}",
                repository_root.display()
            ),
        ),
    })
}

#[cfg(target_os = "linux")]
fn absolute_loader_path(path: &Path) -> io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

#[cfg(target_os = "linux")]
fn open_absolute_directory_no_symlinks(path: &Path) -> io::Result<rustix::fd::OwnedFd> {
    use rustix::fs::{Mode, OFlags, ResolveFlags};

    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "descriptor-relative root must be absolute",
        ));
    }
    let mut current = rustix::fs::open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(errno_to_io)?;
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(segment) => {
                current = rustix::fs::openat2(
                    &current,
                    segment,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                    ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS,
                )
                .map_err(errno_to_io)?;
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "non-normal descriptor-relative root component in {}",
                        path.display()
                    ),
                ));
            }
        }
    }
    Ok(current)
}

#[cfg(target_os = "linux")]
fn held_descriptor_path(raw_fd: std::os::fd::RawFd) -> io::Result<PathBuf> {
    fs::read_link(format!("/proc/self/fd/{raw_fd}"))
}

#[cfg(target_os = "linux")]
fn errno_to_io(error: rustix::io::Errno) -> io::Error {
    io::Error::from_raw_os_error(error.raw_os_error())
}

#[cfg(target_os = "linux")]
fn path_open_error(
    operation: &'static str,
    path: PathBuf,
    source: io::Error,
) -> CapturePolicyError {
    if matches!(
        source.raw_os_error(),
        Some(code)
            if code == rustix::io::Errno::LOOP.raw_os_error()
                || code == rustix::io::Errno::NOTDIR.raw_os_error()
    ) {
        CapturePolicyError::PolicyPathIsSymlink { path }
    } else {
        CapturePolicyError::Io {
            operation,
            path,
            source,
        }
    }
}

/// Decide whether raw bytes may cross the persistence boundary.
///
/// This function is backend-neutral and owns all positive authorization.
/// Stores should only receive bytes when this returns `PersistRaw`.
pub fn authorize_raw_persistence(
    relative_path: &str,
    bytes: &[u8],
    repository_binding: &str,
    authorization: Option<&RawPersistenceAuthorization>,
) -> RawPersistenceDecision {
    let repository_binding = normalized_repository_binding(repository_binding);
    let source_class = classify_source_class(relative_path);
    let classification = classify_source_secret(relative_path, bytes);
    let exact_source = ExactSourceRequirement {
        relative_path: relative_path.to_string(),
        byte_len: bytes.len() as u64,
        sha256: hex_sha256(bytes),
    };

    let policy = provenance_for(authorization, &repository_binding);
    let reason = if classification.status == SecretClassificationStatus::SecretDetected {
        RawPersistenceReason::ClassifierSecretDetected
    } else if classification.status == SecretClassificationStatus::Uncertain {
        RawPersistenceReason::ClassifierUncertain
    } else if !source_class.is_core_safe() {
        RawPersistenceReason::HardDeniedSourceClass
    } else {
        match authorization {
            None => RawPersistenceReason::MissingAuthorization,
            Some(RawPersistenceAuthorization::BuiltInSafeSourceClasses) => {
                RawPersistenceReason::AuthorizedSafeSourceClass
            }
            Some(RawPersistenceAuthorization::External(binding))
                if binding.repository_binding != repository_binding =>
            {
                RawPersistenceReason::RepositoryBindingMismatch
            }
            Some(RawPersistenceAuthorization::External(binding))
                if binding.allows(source_class) =>
            {
                RawPersistenceReason::AuthorizedExternalPolicy
            }
            Some(RawPersistenceAuthorization::External(_)) => {
                RawPersistenceReason::ExternalPolicyClassNotAllowed
            }
        }
    };
    let disposition = match reason {
        RawPersistenceReason::AuthorizedSafeSourceClass
        | RawPersistenceReason::AuthorizedExternalPolicy => RawPersistenceDisposition::PersistRaw,
        _ => RawPersistenceDisposition::MetadataOnly,
    };

    RawPersistenceDecision {
        disposition,
        reason,
        source_class,
        classifier_status: classification.status,
        classifier_evidence: classification.evidence,
        policy,
        exact_source,
    }
}

pub fn classify_source_class(relative_path: &str) -> SourceClass {
    let normalized = relative_path.replace('\\', "/");
    let original_path = Path::new(&normalized);
    if normalized.is_empty()
        || normalized.contains('\0')
        || original_path.is_absolute()
        || original_path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return SourceClass::Unknown;
    }
    let lowercase = normalized.to_ascii_lowercase();
    let path = Path::new(&lowercase);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let extension = path
        .extension()
        .and_then(|item| item.to_str())
        .unwrap_or("");
    let components = lowercase.split('/').collect::<Vec<_>>();

    if file_name == ".env"
        || file_name.starts_with(".env.")
        || components
            .iter()
            .any(|item| matches!(*item, ".git" | ".ssh" | ".aws" | ".gnupg"))
        || matches!(
            file_name,
            "credentials" | "credentials.json" | "id_rsa" | "id_dsa" | "id_ecdsa" | "id_ed25519"
        )
        || matches!(extension, "key" | "pem" | "p12" | "pfx")
    {
        return SourceClass::Sensitive;
    }

    if matches!(
        file_name,
        "cargo.toml"
            | "cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "gemfile"
            | "gemfile.lock"
            | "go.mod"
            | "go.sum"
            | "flake.nix"
            | "flake.lock"
    ) || matches!(
        extension,
        "toml"
            | "json"
            | "jsonc"
            | "yaml"
            | "yml"
            | "ini"
            | "cfg"
            | "conf"
            | "config"
            | "env"
            | "lock"
            | "nix"
    ) {
        return SourceClass::Configuration;
    }

    if matches!(
        extension,
        "rs" | "py"
            | "pyi"
            | "rb"
            | "js"
            | "jsx"
            | "mjs"
            | "cjs"
            | "ts"
            | "tsx"
            | "go"
            | "c"
            | "h"
            | "cc"
            | "cpp"
            | "cxx"
            | "hpp"
            | "java"
            | "kt"
            | "kts"
            | "swift"
            | "scala"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "nu"
            | "sql"
            | "css"
            | "scss"
            | "sass"
            | "less"
            | "html"
            | "htm"
            | "vue"
            | "svelte"
    ) {
        return SourceClass::SourceCode;
    }

    if matches!(extension, "md" | "mdx" | "rst" | "adoc")
        || file_name.starts_with("readme")
        || file_name.starts_with("license")
        || file_name.starts_with("changelog")
        || file_name.starts_with("contributing")
    {
        return SourceClass::Documentation;
    }

    SourceClass::Unknown
}

fn provenance_for(
    authorization: Option<&RawPersistenceAuthorization>,
    repository_binding: &str,
) -> PolicyProvenance {
    match authorization {
        None => built_in_provenance(
            "codedb-default-deny-v1",
            DEFAULT_DENY_POLICY,
            PolicyAuthoritySource::DefaultDeny,
            repository_binding,
        ),
        Some(RawPersistenceAuthorization::BuiltInSafeSourceClasses) => built_in_provenance(
            "codedb-safe-source-classes-v1",
            BUILT_IN_SAFE_SOURCE_POLICY,
            PolicyAuthoritySource::BuiltInSafeSourceClasses,
            repository_binding,
        ),
        Some(RawPersistenceAuthorization::External(binding)) => PolicyProvenance {
            policy_id: binding.policy_id.clone(),
            policy_digest: binding.policy_digest.clone(),
            binding_digest: binding.binding_digest.clone(),
            authority: binding.authority.clone(),
            authority_source: PolicyAuthoritySource::ExternalOperatorPolicy,
            repository_binding: binding.repository_binding.clone(),
            external_policy_path: Some(binding.policy_path.clone()),
        },
    }
}

fn built_in_provenance(
    policy_id: &str,
    policy_document: &str,
    authority_source: PolicyAuthoritySource,
    repository_binding: &str,
) -> PolicyProvenance {
    let policy_digest = prefixed_sha256(policy_document.as_bytes());
    PolicyProvenance {
        policy_id: policy_id.to_string(),
        binding_digest: policy_binding_digest(
            &policy_digest,
            policy_id,
            "codedb-core",
            repository_binding,
            authority_source,
        ),
        policy_digest,
        authority: "codedb-core".to_string(),
        authority_source,
        repository_binding: repository_binding.to_string(),
        external_policy_path: None,
    }
}

fn parse_policy_document(document: &[u8]) -> Result<BTreeMap<String, String>, CapturePolicyError> {
    let text =
        std::str::from_utf8(document).map_err(|_| CapturePolicyError::InvalidPolicyDocument {
            reason: "document must be valid UTF-8".to_string(),
        })?;
    let mut fields = BTreeMap::new();
    for (index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) =
            line.split_once('=')
                .ok_or_else(|| CapturePolicyError::InvalidPolicyDocument {
                    reason: format!("line {} must use key=value", index + 1),
                })?;
        let key = key.trim();
        let value = value.trim();
        if !matches!(
            key,
            "version" | "policy_id" | "authority" | "repository_binding" | "allow"
        ) {
            return Err(CapturePolicyError::InvalidPolicyDocument {
                reason: format!("unknown field {key}"),
            });
        }
        if value.is_empty() && key != "allow" {
            return Err(CapturePolicyError::InvalidPolicyDocument {
                reason: format!("{key} must not be empty"),
            });
        }
        if fields.insert(key.to_string(), value.to_string()).is_some() {
            return Err(CapturePolicyError::InvalidPolicyDocument {
                reason: format!("duplicate field {key}"),
            });
        }
    }
    Ok(fields)
}

fn required_field<'a>(
    fields: &'a BTreeMap<String, String>,
    field: &str,
) -> Result<&'a str, CapturePolicyError> {
    fields
        .get(field)
        .map(String::as_str)
        .ok_or_else(|| CapturePolicyError::InvalidPolicyDocument {
            reason: format!("missing field {field}"),
        })
}

fn parse_allowed_classes(value: &str) -> Result<BTreeSet<SourceClass>, CapturePolicyError> {
    let mut allowed = BTreeSet::new();
    for raw_class in value.split(',') {
        let class = raw_class.trim();
        let parsed = match class {
            "source-code" => SourceClass::SourceCode,
            "documentation" => SourceClass::Documentation,
            "configuration" | "sensitive" | "unknown" => {
                return Err(CapturePolicyError::HardDeniedClassInPolicy {
                    class: class.to_string(),
                });
            }
            "" => continue,
            _ => {
                return Err(CapturePolicyError::InvalidPolicyDocument {
                    reason: format!("unknown source class {class}"),
                });
            }
        };
        allowed.insert(parsed);
    }
    Ok(allowed)
}

fn validate_public_identifier(field: &str, value: &str) -> Result<(), CapturePolicyError> {
    if value.len() > 128
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-' | b'@' | b'/')
        })
    {
        return Err(CapturePolicyError::InvalidPolicyDocument {
            reason: format!("{field} must be a bounded public identifier"),
        });
    }
    Ok(())
}

fn is_sha256_binding(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    })
}

fn normalized_repository_binding(value: &str) -> String {
    if is_sha256_binding(value) {
        value.to_ascii_lowercase()
    } else {
        format!("opaque-sha256:{}", hex_sha256(value.as_bytes()))
    }
}

fn policy_binding_digest(
    policy_digest: &str,
    policy_id: &str,
    authority: &str,
    repository_binding: &str,
    authority_source: PolicyAuthoritySource,
) -> String {
    let mut hasher = Sha256::new();
    for value in [
        EXTERNAL_POLICY_VERSION,
        policy_digest,
        policy_id,
        authority,
        repository_binding,
        authority_source.as_str(),
    ] {
        hasher.update(value.len().to_be_bytes());
        hasher.update(value.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn prefixed_sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", hex_sha256(bytes))
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
