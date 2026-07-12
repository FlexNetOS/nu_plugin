#![forbid(unsafe_code)]

use std::error::Error as StdError;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

pub mod capture_policy;
pub mod merge;
pub mod store;
pub mod store_spec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Planned,
    Available,
    Observed,
    Degraded,
    Expected,
}

impl RowState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Available => "available",
            Self::Observed => "observed",
            Self::Degraded => "degraded",
            Self::Expected => "expected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl SchemaVersion {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub const fn as_tuple(self) -> (u16, u16, u16) {
        (self.major, self.minor, self.patch)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentityKey {
    pub schema_version: SchemaVersion,
    pub workspace_id: &'static str,
    pub crate_id: &'static str,
    pub module_path: &'static str,
    pub object_kind: &'static str,
    pub stable_name: &'static str,
    pub source_span: &'static str,
    pub context_hash: &'static str,
    pub source_blob_hash: &'static str,
}

impl IdentityKey {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        schema_version: SchemaVersion,
        workspace_id: &'static str,
        crate_id: &'static str,
        module_path: &'static str,
        object_kind: &'static str,
        stable_name: &'static str,
        source_span: &'static str,
        context_hash: &'static str,
        source_blob_hash: &'static str,
    ) -> Self {
        Self {
            schema_version,
            workspace_id,
            crate_id,
            module_path,
            object_kind,
            stable_name,
            source_span,
            context_hash,
            source_blob_hash,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableSpec {
    pub name: &'static str,
    pub domain: &'static str,
    pub state: RowState,
    pub row_count: u64,
    pub note: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureGap {
    pub table: &'static str,
    pub reason: &'static str,
    pub required_task: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationError {
    pub table: &'static str,
    pub code: &'static str,
    pub message: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesystemEntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

impl FilesystemEntryKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::File => "file",
            Self::Symlink => "symlink",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymlinkMaterializationStatus {
    Supported,
    MetadataOnlyFallback,
}

impl SymlinkMaterializationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::MetadataOnlyFallback => "metadata_only_fallback",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformMaterializationRow {
    pub table: &'static str,
    pub status: &'static str,
    pub rows: u64,
    pub relative_path: String,
    pub platform: &'static str,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileClassification {
    RustSource,
    CargoManifest,
    CargoLock,
    Hidden,
    Vendor,
    Generated,
    NonRustAsset,
    Directory,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceBlobMode {
    MetadataOnly,
    HashedBlob,
    RedactedExport,
    RawLocal,
}

impl SourceBlobMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MetadataOnly => "metadata-only",
            Self::HashedBlob => "hashed-blob",
            Self::RedactedExport => "redacted-export",
            Self::RawLocal => "raw-local",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncodingStatus {
    Utf8,
    Binary,
    InvalidUtf8,
}

impl TextEncodingStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Utf8 => "utf8",
            Self::Binary => "binary",
            Self::InvalidUtf8 => "invalid_utf8",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretClassificationStatus {
    NoSecretDetected,
    SecretDetected,
    Uncertain,
}

impl SecretClassificationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoSecretDetected => "no_secret_detected",
            Self::SecretDetected => "secret_detected",
            Self::Uncertain => "uncertain",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretEvidenceKind {
    SensitivePath,
    BearerToken,
    JsonWebToken,
    AwsAccessKeyId,
    AwsSecretAccessKey,
    NpmAuthToken,
    DatabaseUriCredentials,
    PrivateKeyHeader,
    CredentialAssignment,
    GenericSecretToken,
    NonTextContent,
    NonUtf8Path,
}

impl SecretEvidenceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SensitivePath => "sensitive_path",
            Self::BearerToken => "bearer_token",
            Self::JsonWebToken => "json_web_token",
            Self::AwsAccessKeyId => "aws_access_key_id",
            Self::AwsSecretAccessKey => "aws_secret_access_key",
            Self::NpmAuthToken => "npm_auth_token",
            Self::DatabaseUriCredentials => "database_uri_credentials",
            Self::PrivateKeyHeader => "private_key_header",
            Self::CredentialAssignment => "credential_assignment",
            Self::GenericSecretToken => "generic_secret_token",
            Self::NonTextContent => "non_text_content",
            Self::NonUtf8Path => "non_utf8_path",
        }
    }

    const fn is_uncertainty(self) -> bool {
        matches!(self, Self::NonTextContent | Self::NonUtf8Path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretClassification {
    pub status: SecretClassificationStatus,
    pub evidence: Vec<SecretEvidenceKind>,
}

impl SecretClassification {
    pub fn has_secret(&self) -> bool {
        self.status == SecretClassificationStatus::SecretDetected
    }

    pub fn is_uncertain(&self) -> bool {
        self.status == SecretClassificationStatus::Uncertain
    }

    /// Raw persistence is permitted only when the classifier observed valid UTF-8
    /// and found neither a sensitive path nor a supported secret form.
    pub fn raw_persistence_safe(&self) -> bool {
        self.status == SecretClassificationStatus::NoSecretDetected
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewlineStyle {
    None,
    Lf,
    CrLf,
    Mixed,
}

impl NewlineStyle {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Lf => "lf",
            Self::CrLf => "crlf",
            Self::Mixed => "mixed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBlobMetadata {
    pub relative_path: String,
    pub byte_len: u64,
    pub sha256: String,
    pub encoding_status: TextEncodingStatus,
    pub newline_style: NewlineStyle,
    pub has_utf8_bom: bool,
    pub has_secret_like_material: bool,
    pub default_mode: SourceBlobMode,
    pub export_raw_by_default: bool,
    pub policy_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePolicyRow {
    pub relative_path: String,
    pub mode: SourceBlobMode,
    pub raw_export_allowed: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoMutationStatus {
    Proven,
    Mutated,
    Degraded,
}

impl NoMutationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proven => "proven",
            Self::Mutated => "mutated",
            Self::Degraded => "degraded",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRepoSnapshot {
    pub status_porcelain: String,
    pub file_manifest_hash: String,
}

impl GitRepoSnapshot {
    pub fn is_clean(&self) -> bool {
        self.status_porcelain.trim().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoMutationProof {
    pub operation: String,
    pub status: NoMutationStatus,
    pub before: GitRepoSnapshot,
    pub after: GitRepoSnapshot,
    pub pre_existing_dirty: bool,
    pub mutation_detected: bool,
    pub degradation_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangePlanStatus {
    Draft,
    Reviewed,
    Blocked,
    ApprovedForIsolatedPatch,
    ApprovedForApply,
    Applied,
    Recovered,
}

impl ChangePlanStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Reviewed => "reviewed",
            Self::Blocked => "blocked",
            Self::ApprovedForIsolatedPatch => "approved_for_isolated_patch",
            Self::ApprovedForApply => "approved_for_apply",
            Self::Applied => "applied",
            Self::Recovered => "recovered",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Create,
    Update,
    Delete,
}

impl ChangeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePlanRoot {
    pub plan_id: &'static str,
    pub source_snapshot_id: &'static str,
    pub status: ChangePlanStatus,
    pub created_at: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePlanNode {
    pub node_id: &'static str,
    pub object_id: &'static str,
    pub change_kind: ChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePlanEdge {
    pub from_node_id: &'static str,
    pub to_node_id: &'static str,
    pub edge_kind: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePlanGraph {
    pub plan: ChangePlanRoot,
    pub nodes: Vec<ChangePlanNode>,
    pub edges: Vec<ChangePlanEdge>,
}

impl ChangePlanGraph {
    pub const fn status_allows_source_apply(&self) -> bool {
        matches!(
            self.plan.status,
            ChangePlanStatus::ApprovedForApply | ChangePlanStatus::Applied
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangePlanTableRow {
    pub table: &'static str,
    pub status: &'static str,
    pub rows: u64,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanConflictKind {
    SourceDrift,
    MissingEvidence,
}

impl PlanConflictKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceDrift => "source_drift",
            Self::MissingEvidence => "missing_evidence",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanConflict {
    pub plan_id: &'static str,
    pub source_snapshot_id: &'static str,
    pub conflict_kind: PlanConflictKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolatedPatchArtifact {
    pub path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
    pub proof_gate: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyDecision {
    Approved,
    Denied,
}

impl ApplyDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Denied => "denied",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorDecision {
    pub decision_id: &'static str,
    pub plan_id: &'static str,
    pub actor: &'static str,
    pub decided_at: &'static str,
    pub decision: ApplyDecision,
    pub evidence_ref: &'static str,
    pub manual_decision_ref: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopConditionProof {
    pub proof_id: &'static str,
    pub passed: bool,
    pub evidence_ref: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyGateReport {
    pub plan_id: &'static str,
    pub decision_id: &'static str,
    pub status: &'static str,
    pub recovery_ref: String,
    pub rows: Vec<ChangePlanTableRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyGateError {
    PlanNotApprovedForApply,
    MissingOperatorDecision,
    OperatorDecisionPlanMismatch,
    OperatorDenied,
    MissingDecisionEvidence,
    StopConditionFailed,
    MissingRecoveryRef,
    SourceDrift,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BidirectionalSyncDirection {
    SourceToStore,
    StoreToSource,
}

impl BidirectionalSyncDirection {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceToStore => "source_to_store",
            Self::StoreToSource => "store_to_source",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BidirectionalSyncStatus {
    Verified,
    Conflict,
    RecoveryRequired,
    Recovered,
}

impl BidirectionalSyncStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Conflict => "conflict",
            Self::RecoveryRequired => "recovery_required",
            Self::Recovered => "recovered",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BidirectionalSyncReport {
    pub plan_id: &'static str,
    pub direction: BidirectionalSyncDirection,
    pub status: BidirectionalSyncStatus,
    pub rows: Vec<ChangePlanTableRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailedApplyKind {
    Materialization,
    Apply,
}

impl FailedApplyKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Materialization => "materialization",
            Self::Apply => "apply",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedApplyRecoveryInput {
    pub attempt_id: &'static str,
    pub plan_id: &'static str,
    pub kind: FailedApplyKind,
    pub failure_ref: &'static str,
    pub observed_snapshot_id: &'static str,
    pub restored_snapshot_id: &'static str,
    pub quarantine_ref: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailedApplyRecoveryError {
    AttemptPlanMismatch,
    MissingAuditRef,
    SourceNotRestored,
}

#[derive(Debug)]
pub enum PatchPlanError {
    TargetInsideSource {
        source_checkout: PathBuf,
        target_worktree: PathBuf,
    },
    UnsafePatchPath {
        relative_patch_path: PathBuf,
    },
    MissingProofGate,
    Io {
        path: PathBuf,
        source: io::Error,
    },
}

impl Display for PatchPlanError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TargetInsideSource {
                source_checkout,
                target_worktree,
            } => write!(
                f,
                "patch target {} is inside source checkout {}",
                target_worktree.display(),
                source_checkout.display()
            ),
            Self::UnsafePatchPath {
                relative_patch_path,
            } => write!(
                f,
                "patch path must be relative and cannot escape target: {}",
                relative_patch_path.display()
            ),
            Self::MissingProofGate => write!(f, "isolated patch plan requires a proof gate"),
            Self::Io { path, source } => {
                write!(
                    f,
                    "failed to write patch artifact {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl StdError for PatchPlanError {}

impl FileClassification {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RustSource => "rust_source",
            Self::CargoManifest => "cargo_manifest",
            Self::CargoLock => "cargo_lock",
            Self::Hidden => "hidden",
            Self::Vendor => "vendor",
            Self::Generated => "generated",
            Self::NonRustAsset => "non_rust_asset",
            Self::Directory => "directory",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemEntry {
    pub relative_path: String,
    pub kind: FilesystemEntryKind,
    pub size_bytes: u64,
    pub readonly: bool,
    pub is_symlink: bool,
    pub symlink_target: Option<String>,
    pub classification: FileClassification,
}

impl FilesystemEntry {
    pub fn table_row_note(&self) -> String {
        format!(
            "{}:{}:{}",
            self.kind.as_str(),
            self.classification.as_str(),
            self.size_bytes
        )
    }
}

#[derive(Debug)]
pub enum ScanError {
    RootMetadata { path: PathBuf, source: io::Error },
    ReadDir { path: PathBuf, source: io::Error },
    Entry { path: PathBuf, source: io::Error },
    Metadata { path: PathBuf, source: io::Error },
    SymlinkTarget { path: PathBuf, source: io::Error },
    NonUtf8Path { path: PathBuf },
}

#[derive(Debug)]
pub enum SourceCaptureError {
    Read { path: PathBuf, source: io::Error },
    Metadata { path: PathBuf, source: io::Error },
    NonUtf8Path { path: PathBuf },
}

#[derive(Debug)]
pub enum NoMutationError {
    Snapshot { path: PathBuf, source: io::Error },
    NonUtf8Path { path: PathBuf },
}

impl Display for NoMutationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Snapshot { path, source } => {
                write!(f, "failed to snapshot repo {}: {source}", path.display())
            }
            Self::NonUtf8Path { path } => write!(f, "path is not valid UTF-8: {}", path.display()),
        }
    }
}

impl StdError for NoMutationError {}

impl Display for SourceCaptureError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(
                    f,
                    "failed to read source bytes {}: {source}",
                    path.display()
                )
            }
            Self::Metadata { path, source } => {
                write!(
                    f,
                    "failed to inspect source path {}: {source}",
                    path.display()
                )
            }
            Self::NonUtf8Path { path } => write!(f, "path is not valid UTF-8: {}", path.display()),
        }
    }
}

impl StdError for SourceCaptureError {}

impl Display for ScanError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RootMetadata { path, source } => {
                write!(
                    f,
                    "failed to inspect scan root {}: {source}",
                    path.display()
                )
            }
            Self::ReadDir { path, source } => {
                write!(f, "failed to read directory {}: {source}", path.display())
            }
            Self::Entry { path, source } => {
                write!(
                    f,
                    "failed to read directory entry under {}: {source}",
                    path.display()
                )
            }
            Self::Metadata { path, source } => {
                write!(f, "failed to inspect entry {}: {source}", path.display())
            }
            Self::SymlinkTarget { path, source } => {
                write!(
                    f,
                    "failed to read symlink target {}: {source}",
                    path.display()
                )
            }
            Self::NonUtf8Path { path } => write!(f, "path is not valid UTF-8: {}", path.display()),
        }
    }
}

impl StdError for ScanError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableRow {
    pub table: &'static str,
    pub status: &'static str,
    pub rows: u64,
    pub note: &'static str,
}

impl From<TableSpec> for TableRow {
    fn from(spec: TableSpec) -> Self {
        Self {
            table: spec.name,
            status: spec.state.as_str(),
            rows: spec.row_count,
            note: spec.note,
        }
    }
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0, 0);

pub fn workspace_identity() -> IdentityKey {
    IdentityKey::new(
        SCHEMA_VERSION,
        "workspace::unknown",
        "crate::unknown",
        "module::unknown",
        "identity",
        "workspace_identity",
        "span::unknown",
        "context::unknown",
        "blob::unknown",
    )
}

pub fn schema_tables() -> Vec<TableSpec> {
    vec![
        TableSpec {
            name: "codedb_contexts",
            domain: "core identity",
            state: RowState::Planned,
            row_count: 0,
            note: "identity context rows are not captured in CDB013",
        },
        TableSpec {
            name: "source_files",
            domain: "filesystem/source",
            state: RowState::Available,
            row_count: 0,
            note: "filesystem scanner is available after CDB017",
        },
        TableSpec {
            name: "cargo_packages",
            domain: "cargo",
            state: RowState::Planned,
            row_count: 0,
            note: "cargo metadata capture starts after CDB019",
        },
        TableSpec {
            name: "rust_items",
            domain: "rust static",
            state: RowState::Planned,
            row_count: 0,
            note: "static Rust inventory starts after CDB022",
        },
        TableSpec {
            name: "capture_gaps",
            domain: "proof/artifact",
            state: RowState::Available,
            row_count: 1,
            note: "CDB013 skeleton records unsupported observations as gaps",
        },
        TableSpec {
            name: "validation_errors",
            domain: "proof/artifact",
            state: RowState::Available,
            row_count: 0,
            note: "no validation has run yet",
        },
        TableSpec {
            name: "source_blobs",
            domain: "store/blob",
            state: RowState::Available,
            row_count: 0,
            note: "source blob metadata capture is metadata-only by default after CDB018",
        },
        TableSpec {
            name: "blob_policies",
            domain: "store/blob",
            state: RowState::Available,
            row_count: 0,
            note: "raw source export is disabled by default after CDB018",
        },
        TableSpec {
            name: "change_plans",
            domain: "bidirectional/plan",
            state: RowState::Available,
            row_count: 0,
            note: "reviewable change-plan roots are modeled after CDB073",
        },
        TableSpec {
            name: "change_plan_nodes",
            domain: "bidirectional/plan",
            state: RowState::Available,
            row_count: 0,
            note: "object-level plan nodes are modeled after CDB073",
        },
        TableSpec {
            name: "change_plan_edges",
            domain: "bidirectional/plan",
            state: RowState::Available,
            row_count: 0,
            note: "plan dependency edges are modeled after CDB073",
        },
        TableSpec {
            name: "plan_conflicts",
            domain: "bidirectional/plan",
            state: RowState::Available,
            row_count: 0,
            note: "source drift conflicts are modeled before apply after CDB073",
        },
        TableSpec {
            name: "operator_decisions",
            domain: "bidirectional/apply",
            state: RowState::Available,
            row_count: 0,
            note: "operator approval provenance is modeled after CDB075",
        },
        TableSpec {
            name: "apply_attempts",
            domain: "bidirectional/apply",
            state: RowState::Available,
            row_count: 0,
            note: "apply attempts require approval, stop proof, and recovery refs after CDB075",
        },
        TableSpec {
            name: "sync_verifications",
            domain: "bidirectional/sync",
            state: RowState::Available,
            row_count: 0,
            note: "final re-scan verification rows are modeled after CDB076",
        },
        TableSpec {
            name: "recovery_rows",
            domain: "bidirectional/sync",
            state: RowState::Available,
            row_count: 0,
            note: "failed sync/apply recovery rows are modeled after CDB076",
        },
        TableSpec {
            name: "platform_materialization_capabilities",
            domain: "bidirectional/materialization",
            state: RowState::Available,
            row_count: 0,
            note: "symlink/platform materialization limits are explicit after CDB081",
        },
        TableSpec {
            name: "agent_harness_manifests",
            domain: "agent-harness/manifest",
            state: RowState::Available,
            row_count: 0,
            note: "agent harness manifest rows summarize portable Codex and repo-local harness capture",
        },
        TableSpec {
            name: "agent_harness_sources",
            domain: "agent-harness/source",
            state: RowState::Available,
            row_count: 0,
            note: "source files for harness capture are hashed with ownership boundaries",
        },
        TableSpec {
            name: "agent_harness_files",
            domain: "agent-harness/file",
            state: RowState::Available,
            row_count: 0,
            note: "materialized file rows capture file names, byte lengths, and ownership boundaries",
        },
        TableSpec {
            name: "agent_harness_codex_settings",
            domain: "agent-harness/codex",
            state: RowState::Available,
            row_count: 0,
            note: "Codex settings rows capture redacted configuration values and provenance",
        },
        TableSpec {
            name: "agent_harness_mcp_servers",
            domain: "agent-harness/mcp",
            state: RowState::Available,
            row_count: 0,
            note: "MCP registrations are captured as bounded harness rows with validation support",
        },
        TableSpec {
            name: "agent_harness_plugins",
            domain: "agent-harness/plugins",
            state: RowState::Available,
            row_count: 0,
            note: "plugin metadata rows capture installed plugin ownership and version facts",
        },
        TableSpec {
            name: "agent_harness_plugin_skills",
            domain: "agent-harness/skills",
            state: RowState::Available,
            row_count: 0,
            note: "skill rows capture prompt/skill assets required to reproduce the harness",
        },
        TableSpec {
            name: "agent_harness_prompts",
            domain: "agent-harness/prompts",
            state: RowState::Available,
            row_count: 0,
            note: "prompt rows capture prompt markdown surfaces without exposing private secrets",
        },
        TableSpec {
            name: "agent_harness_hooks",
            domain: "agent-harness/hooks",
            state: RowState::Available,
            row_count: 0,
            note: "hook rows capture configured hook entrypoints and resolution status",
        },
        TableSpec {
            name: "agent_harness_env",
            domain: "agent-harness/env",
            state: RowState::Available,
            row_count: 0,
            note: "frontdoor environment rows declare bounded reproduction inputs",
        },
        TableSpec {
            name: "agent_harness_policy_rows",
            domain: "agent-harness/policy",
            state: RowState::Available,
            row_count: 0,
            note: "policy rows record redaction, ownership, and non-mutation rules for harness export",
        },
        TableSpec {
            name: "agent_harness_validation_errors",
            domain: "agent-harness/validation",
            state: RowState::Available,
            row_count: 0,
            note: "validation rows surface duplicate, missing, stale, or conflicting harness components",
        },
        TableSpec {
            name: "agent_harness_export_manifests",
            domain: "agent-harness/export",
            state: RowState::Available,
            row_count: 0,
            note: "export manifest rows describe bounded harness export artifacts and checksums",
        },
        TableSpec {
            name: "agent_harness_materialization_plan",
            domain: "agent-harness/materialization",
            state: RowState::Available,
            row_count: 0,
            note: "materialization planning rows remain non-mutating and approval-gated by default",
        },
        TableSpec {
            name: "nix_flake_summary",
            domain: "nix/flake",
            state: RowState::Available,
            row_count: 0,
            note: "nix flake metadata import rows are produced by codedb nix flake import",
        },
        TableSpec {
            name: "nix_flake_refs",
            domain: "nix/flake",
            state: RowState::Available,
            row_count: 0,
            note: "original/resolved/locked flake references imported from nix flake metadata --json",
        },
        TableSpec {
            name: "nix_flake_lock_nodes",
            domain: "nix/flake.lock",
            state: RowState::Available,
            row_count: 0,
            note: "flake.lock nodes imported from metadata.locks.nodes",
        },
        TableSpec {
            name: "nix_flake_lock_edges",
            domain: "nix/flake.lock",
            state: RowState::Available,
            row_count: 0,
            note: "flake input edges imported from flake.lock node inputs",
        },
        TableSpec {
            name: "nix_flake_outputs",
            domain: "nix/flake",
            state: RowState::Available,
            row_count: 0,
            note: "optional flake output rows imported from nix flake show --json --all-systems",
        },
    ]
}

pub fn table_inventory() -> Vec<TableRow> {
    schema_tables().into_iter().map(Into::into).collect()
}

pub fn capture_gaps() -> Vec<TableRow> {
    vec![TableRow {
        table: "capture_gaps",
        status: RowState::Expected.as_str(),
        rows: 1,
        note: "compiler-observable capture is intentionally not implemented before CDB014-CDB030",
    }]
}

pub fn validation_errors() -> Vec<TableRow> {
    Vec::new()
}

pub fn schema_rows() -> Vec<TableRow> {
    vec![
        TableRow {
            table: "schema_versions",
            status: RowState::Planned.as_str(),
            rows: 0,
            note: "redb schema versioning starts after CDB015",
        },
        TableRow {
            table: "store_metadata",
            status: RowState::Planned.as_str(),
            rows: 0,
            note: "redb store metadata starts after CDB015",
        },
    ]
}

pub fn doctor_rows() -> Vec<TableRow> {
    vec![
        TableRow {
            table: "host_nu",
            status: RowState::Observed.as_str(),
            rows: 1,
            note: "package skeleton targets nu-plugin/nu-protocol 0.112.2",
        },
        TableRow {
            table: "codedb_store",
            status: RowState::Degraded.as_str(),
            rows: 0,
            note: "redb store is not implemented until CDB015",
        },
    ]
}

pub fn capture_gap_specs() -> Vec<CaptureGap> {
    vec![CaptureGap {
        table: "capture_gaps",
        reason: "static capture intentionally stops before later tasks implement the scanner and cargo/static layers",
        required_task: "CDB014-CDB030",
    }]
}

pub fn validation_error_specs() -> Vec<ValidationError> {
    Vec::new()
}

pub fn capture_source_metadata(
    root: impl AsRef<Path>,
    path: impl AsRef<Path>,
) -> Result<SourceBlobMetadata, SourceCaptureError> {
    let root = root.as_ref();
    let path = path.as_ref();
    let metadata = fs::metadata(path).map_err(|source| SourceCaptureError::Metadata {
        path: path.to_path_buf(),
        source,
    })?;
    let bytes = fs::read(path).map_err(|source| SourceCaptureError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let relative_path = source_relative_path(root, path)?;
    let mut captured = capture_source_metadata_from_bytes(relative_path, &bytes);
    // Metadata and bytes came from separate pathname operations in this
    // compatibility API. New capture frontdoors must use
    // `ContainedDirectory::read_regular_file` and this byte-based constructor.
    captured.byte_len = metadata.len();
    Ok(captured)
}

/// Derive source policy metadata from bytes already read through a contained
/// file descriptor. This function never reopens a pathname.
pub fn capture_source_metadata_from_bytes(
    relative_path: impl Into<String>,
    bytes: &[u8],
) -> SourceBlobMetadata {
    let relative_path = relative_path.into();
    let secret_classification = classify_source_secret(&relative_path, bytes);
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    let encoding_status = detect_encoding_status(bytes);
    let newline_style = detect_newline_style(bytes);
    let has_utf8_bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
    let has_secret_like_material = secret_classification.has_secret();
    let default_mode = if has_secret_like_material {
        SourceBlobMode::RedactedExport
    } else {
        SourceBlobMode::MetadataOnly
    };
    let policy_reason = match secret_classification.status {
        SecretClassificationStatus::SecretDetected => {
            "secret-looking path or content detected; raw source export disabled".to_string()
        }
        SecretClassificationStatus::Uncertain => {
            "secret classification uncertain for non-text input; metadata-only capture required and raw persistence disabled"
                .to_string()
        }
        SecretClassificationStatus::NoSecretDetected => {
            "metadata-only source capture; raw source export disabled by default".to_string()
        }
    };

    SourceBlobMetadata {
        relative_path,
        byte_len: bytes.len() as u64,
        sha256,
        encoding_status,
        newline_style,
        has_utf8_bom,
        has_secret_like_material,
        default_mode,
        export_raw_by_default: false,
        policy_reason,
    }
}

pub fn source_policy_row(metadata: &SourceBlobMetadata) -> SourcePolicyRow {
    SourcePolicyRow {
        relative_path: metadata.relative_path.clone(),
        mode: metadata.default_mode,
        raw_export_allowed: metadata.export_raw_by_default,
        reason: metadata.policy_reason.clone(),
    }
}

pub fn symlink_materialization_rows(
    entries: &[FilesystemEntry],
    symlink_creation_supported: bool,
) -> Vec<PlatformMaterializationRow> {
    let status = if symlink_creation_supported {
        SymlinkMaterializationStatus::Supported
    } else {
        SymlinkMaterializationStatus::MetadataOnlyFallback
    };
    entries
        .iter()
        .filter(|entry| entry.kind == FilesystemEntryKind::Symlink || entry.is_symlink)
        .map(|entry| {
            let target = entry.symlink_target.as_deref().unwrap_or("unknown-target");
            let note = if symlink_creation_supported {
                format!(
                    "symlink can be materialized as link to {target}; target existence still validated separately"
                )
            } else {
                format!(
                    "safe fallback: preserve symlink metadata for {target}; do not materialize as regular file"
                )
            };
            PlatformMaterializationRow {
                table: "platform_materialization_capabilities",
                status: status.as_str(),
                rows: 1,
                relative_path: entry.relative_path.clone(),
                platform: std::env::consts::OS,
                note,
            }
        })
        .collect()
}

pub fn change_plan_table_rows(graph: &ChangePlanGraph) -> Vec<ChangePlanTableRow> {
    let first_node = graph
        .nodes
        .first()
        .map(|node| {
            format!(
                "{}:{}:{}",
                node.node_id,
                node.object_id,
                node.change_kind.as_str()
            )
        })
        .unwrap_or_else(|| "no_nodes".to_string());
    let first_edge = graph
        .edges
        .first()
        .map(|edge| {
            format!(
                "{}->{}:{}",
                edge.from_node_id, edge.to_node_id, edge.edge_kind
            )
        })
        .unwrap_or_else(|| "no_edges".to_string());

    vec![
        ChangePlanTableRow {
            table: "change_plans",
            status: graph.plan.status.as_str(),
            rows: 1,
            note: format!(
                "{}:{}:{}",
                graph.plan.plan_id, graph.plan.source_snapshot_id, graph.plan.created_at
            ),
        },
        ChangePlanTableRow {
            table: "change_plan_nodes",
            status: RowState::Available.as_str(),
            rows: graph.nodes.len() as u64,
            note: first_node,
        },
        ChangePlanTableRow {
            table: "change_plan_edges",
            status: RowState::Available.as_str(),
            rows: graph.edges.len() as u64,
            note: first_edge,
        },
    ]
}

pub fn detect_plan_conflicts(
    graph: &ChangePlanGraph,
    current_source_snapshot_id: &'static str,
) -> Vec<PlanConflict> {
    if graph.plan.source_snapshot_id == current_source_snapshot_id {
        return Vec::new();
    }

    vec![PlanConflict {
        plan_id: graph.plan.plan_id,
        source_snapshot_id: graph.plan.source_snapshot_id,
        conflict_kind: PlanConflictKind::SourceDrift,
        message: format!(
            "plan snapshot {} differs from current snapshot {}",
            graph.plan.source_snapshot_id, current_source_snapshot_id
        ),
    }]
}

pub fn generate_isolated_patch_artifact(
    source_checkout: impl AsRef<Path>,
    target_worktree: impl AsRef<Path>,
    relative_patch_path: impl AsRef<Path>,
    patch_bytes: &[u8],
    proof_gate: impl AsRef<str>,
) -> Result<IsolatedPatchArtifact, PatchPlanError> {
    let source_checkout = source_checkout.as_ref().to_path_buf();
    let target_worktree = target_worktree.as_ref().to_path_buf();
    let relative_patch_path = relative_patch_path.as_ref();
    let proof_gate = proof_gate.as_ref();

    if proof_gate.trim().is_empty() {
        return Err(PatchPlanError::MissingProofGate);
    }
    if relative_patch_path.is_absolute()
        || relative_patch_path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(PatchPlanError::UnsafePatchPath {
            relative_patch_path: relative_patch_path.to_path_buf(),
        });
    }
    if target_worktree == source_checkout || target_worktree.starts_with(&source_checkout) {
        return Err(PatchPlanError::TargetInsideSource {
            source_checkout,
            target_worktree,
        });
    }

    let output_path = target_worktree.join(relative_patch_path);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|source| PatchPlanError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(&output_path, patch_bytes).map_err(|source| PatchPlanError::Io {
        path: output_path.clone(),
        source,
    })?;

    Ok(IsolatedPatchArtifact {
        path: output_path,
        bytes: patch_bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(patch_bytes)),
        proof_gate: proof_gate.to_string(),
    })
}

pub fn validate_apply_gate(
    graph: &ChangePlanGraph,
    decision: Option<&OperatorDecision>,
    stop_condition: &StopConditionProof,
    recovery_ref: impl AsRef<str>,
    current_source_snapshot_id: &'static str,
) -> Result<ApplyGateReport, ApplyGateError> {
    if !matches!(graph.plan.status, ChangePlanStatus::ApprovedForApply) {
        return Err(ApplyGateError::PlanNotApprovedForApply);
    }
    if graph.plan.source_snapshot_id != current_source_snapshot_id {
        return Err(ApplyGateError::SourceDrift);
    }
    let decision = decision.ok_or(ApplyGateError::MissingOperatorDecision)?;
    if decision.plan_id != graph.plan.plan_id {
        return Err(ApplyGateError::OperatorDecisionPlanMismatch);
    }
    if !matches!(decision.decision, ApplyDecision::Approved) {
        return Err(ApplyGateError::OperatorDenied);
    }
    if decision.decision_id.trim().is_empty()
        || decision.actor.trim().is_empty()
        || decision.decided_at.trim().is_empty()
        || decision.evidence_ref.trim().is_empty()
        || decision.manual_decision_ref.trim().is_empty()
    {
        return Err(ApplyGateError::MissingDecisionEvidence);
    }
    if !stop_condition.passed || stop_condition.evidence_ref.trim().is_empty() {
        return Err(ApplyGateError::StopConditionFailed);
    }
    let recovery_ref = recovery_ref.as_ref();
    if recovery_ref.trim().is_empty() {
        return Err(ApplyGateError::MissingRecoveryRef);
    }

    Ok(ApplyGateReport {
        plan_id: graph.plan.plan_id,
        decision_id: decision.decision_id,
        status: ChangePlanStatus::ApprovedForApply.as_str(),
        recovery_ref: recovery_ref.to_string(),
        rows: vec![
            ChangePlanTableRow {
                table: "operator_decisions",
                status: decision.decision.as_str(),
                rows: 1,
                note: format!(
                    "{}:{}:{}:{}:{}:{}",
                    decision.decision_id,
                    decision.plan_id,
                    decision.actor,
                    decision.decided_at,
                    decision.evidence_ref,
                    decision.manual_decision_ref
                ),
            },
            ChangePlanTableRow {
                table: "apply_attempts",
                status: "ready_for_apply",
                rows: 1,
                note: format!(
                    "{}:{}:{}",
                    graph.plan.plan_id, stop_condition.proof_id, recovery_ref
                ),
            },
        ],
    })
}

pub fn record_failed_apply_recovery(
    graph: &ChangePlanGraph,
    input: &FailedApplyRecoveryInput,
) -> Result<BidirectionalSyncReport, FailedApplyRecoveryError> {
    if input.plan_id != graph.plan.plan_id {
        return Err(FailedApplyRecoveryError::AttemptPlanMismatch);
    }
    if input.attempt_id.trim().is_empty()
        || input.failure_ref.trim().is_empty()
        || input.quarantine_ref.trim().is_empty()
    {
        return Err(FailedApplyRecoveryError::MissingAuditRef);
    }
    if input.restored_snapshot_id != graph.plan.source_snapshot_id {
        return Err(FailedApplyRecoveryError::SourceNotRestored);
    }

    Ok(BidirectionalSyncReport {
        plan_id: graph.plan.plan_id,
        direction: BidirectionalSyncDirection::StoreToSource,
        status: BidirectionalSyncStatus::Recovered,
        rows: vec![
            ChangePlanTableRow {
                table: "apply_attempts",
                status: "failed",
                rows: 1,
                note: format!(
                    "{}:{}:{}:{}",
                    input.attempt_id,
                    graph.plan.plan_id,
                    input.kind.as_str(),
                    input.failure_ref
                ),
            },
            ChangePlanTableRow {
                table: "recovery_rows",
                status: BidirectionalSyncStatus::Recovered.as_str(),
                rows: 1,
                note: format!(
                    "{}:{}:{}:{}",
                    graph.plan.plan_id,
                    input.observed_snapshot_id,
                    input.restored_snapshot_id,
                    input.quarantine_ref
                ),
            },
        ],
    })
}

pub fn evaluate_bidirectional_sync(
    graph: &ChangePlanGraph,
    direction: BidirectionalSyncDirection,
    current_source_snapshot_id: &'static str,
    expected_rescan_snapshot_id: &'static str,
    actual_rescan_snapshot_id: &'static str,
    recovery_ref: &'static str,
) -> BidirectionalSyncReport {
    let conflicts = detect_plan_conflicts(graph, current_source_snapshot_id);
    if let Some(conflict) = conflicts.first() {
        return BidirectionalSyncReport {
            plan_id: graph.plan.plan_id,
            direction,
            status: BidirectionalSyncStatus::Conflict,
            rows: vec![ChangePlanTableRow {
                table: "plan_conflicts",
                status: conflict.conflict_kind.as_str(),
                rows: 1,
                note: format!(
                    "{}:{}:{}",
                    conflict.plan_id, current_source_snapshot_id, conflict.message
                ),
            }],
        };
    }

    if expected_rescan_snapshot_id != actual_rescan_snapshot_id {
        return BidirectionalSyncReport {
            plan_id: graph.plan.plan_id,
            direction,
            status: BidirectionalSyncStatus::RecoveryRequired,
            rows: vec![ChangePlanTableRow {
                table: "recovery_rows",
                status: BidirectionalSyncStatus::RecoveryRequired.as_str(),
                rows: 1,
                note: format!(
                    "{}:{}:{}:{}",
                    graph.plan.plan_id,
                    expected_rescan_snapshot_id,
                    actual_rescan_snapshot_id,
                    recovery_ref
                ),
            }],
        };
    }

    BidirectionalSyncReport {
        plan_id: graph.plan.plan_id,
        direction,
        status: BidirectionalSyncStatus::Verified,
        rows: vec![ChangePlanTableRow {
            table: "sync_verifications",
            status: BidirectionalSyncStatus::Verified.as_str(),
            rows: 1,
            note: format!(
                "{}:{}:{}:{}",
                graph.plan.plan_id,
                direction.as_str(),
                current_source_snapshot_id,
                actual_rescan_snapshot_id
            ),
        }],
    }
}

fn source_relative_path(root: &Path, path: &Path) -> Result<String, SourceCaptureError> {
    if root == path {
        return Ok(".".to_string());
    }
    let relative = path.strip_prefix(root).unwrap_or(path);
    let value = relative
        .to_str()
        .ok_or_else(|| SourceCaptureError::NonUtf8Path {
            path: path.to_path_buf(),
        })?
        .replace('\\', "/");
    Ok(value)
}

fn detect_encoding_status(bytes: &[u8]) -> TextEncodingStatus {
    if bytes.contains(&0) {
        TextEncodingStatus::Binary
    } else if std::str::from_utf8(bytes).is_ok() {
        TextEncodingStatus::Utf8
    } else {
        TextEncodingStatus::InvalidUtf8
    }
}

fn detect_newline_style(bytes: &[u8]) -> NewlineStyle {
    let has_crlf = bytes.windows(2).any(|window| window == b"\r\n");
    let has_lf = bytes
        .iter()
        .enumerate()
        .any(|(index, byte)| *byte == b'\n' && (index == 0 || bytes[index - 1] != b'\r'));
    match (has_lf, has_crlf) {
        (false, false) => NewlineStyle::None,
        (true, false) => NewlineStyle::Lf,
        (false, true) => NewlineStyle::CrLf,
        (true, true) => NewlineStyle::Mixed,
    }
}

pub fn classify_source_secret(
    relative_path: impl AsRef<Path>,
    bytes: &[u8],
) -> SecretClassification {
    let mut evidence = Vec::new();

    match relative_path.as_ref().to_str() {
        Some(path) if path_has_secret_form(path) => {
            push_secret_evidence(&mut evidence, SecretEvidenceKind::SensitivePath);
        }
        Some(_) => {}
        None => push_secret_evidence(&mut evidence, SecretEvidenceKind::NonUtf8Path),
    }

    if detect_encoding_status(bytes) != TextEncodingStatus::Utf8 {
        push_secret_evidence(&mut evidence, SecretEvidenceKind::NonTextContent);
    }

    let text = String::from_utf8_lossy(bytes);
    if contains_bearer_token(&text) {
        push_secret_evidence(&mut evidence, SecretEvidenceKind::BearerToken);
    }
    if contains_json_web_token(&text) {
        push_secret_evidence(&mut evidence, SecretEvidenceKind::JsonWebToken);
    }
    if contains_aws_access_key_id(&text) {
        push_secret_evidence(&mut evidence, SecretEvidenceKind::AwsAccessKeyId);
    }
    if contains_named_assignment(&text, &["awssecretaccesskey"]) {
        push_secret_evidence(&mut evidence, SecretEvidenceKind::AwsSecretAccessKey);
    }
    if contains_named_assignment(&text, &["authtoken"]) {
        push_secret_evidence(&mut evidence, SecretEvidenceKind::NpmAuthToken);
    }
    if contains_database_uri_credentials(&text) {
        push_secret_evidence(&mut evidence, SecretEvidenceKind::DatabaseUriCredentials);
    }
    if contains_private_key_header(&text) {
        push_secret_evidence(&mut evidence, SecretEvidenceKind::PrivateKeyHeader);
    }
    if contains_named_assignment(
        &text,
        &[
            "password",
            "passwd",
            "pwd",
            "passphrase",
            "secret",
            "secretkey",
            "apikey",
            "token",
            "accesstoken",
            "authtoken",
            "refreshtoken",
            "clientsecret",
            "privatekey",
            "signingkey",
            "encryptionkey",
        ],
    ) {
        push_secret_evidence(&mut evidence, SecretEvidenceKind::CredentialAssignment);
    }
    if contains_generic_secret_token(&text) {
        push_secret_evidence(&mut evidence, SecretEvidenceKind::GenericSecretToken);
    }

    let status = if evidence.iter().any(|kind| !kind.is_uncertainty()) {
        SecretClassificationStatus::SecretDetected
    } else if evidence.is_empty() {
        SecretClassificationStatus::NoSecretDetected
    } else {
        SecretClassificationStatus::Uncertain
    };

    SecretClassification { status, evidence }
}

fn push_secret_evidence(evidence: &mut Vec<SecretEvidenceKind>, kind: SecretEvidenceKind) {
    if !evidence.contains(&kind) {
        evidence.push(kind);
    }
}

fn path_has_secret_form(path: &str) -> bool {
    path.replace('\\', "/")
        .split('/')
        .filter(|component| !component.is_empty())
        .any(|component| {
            let name = component.to_ascii_lowercase();
            if matches!(name.as_str(), ".git" | ".ssh" | ".gnupg" | ".aws")
                || name == ".env"
                || name.starts_with(".env.")
                || matches!(
                    name.as_str(),
                    ".npmrc"
                        | ".pgpass"
                        | ".netrc"
                        | ".htpasswd"
                        | "credentials"
                        | ".credentials"
                        | "secrets"
                        | ".secrets"
                        | "id_rsa"
                        | "id_dsa"
                        | "id_ecdsa"
                        | "id_ed25519"
                )
                || name.ends_with(".key")
                || name.ends_with(".p12")
                || name.ends_with(".pfx")
            {
                return true;
            }

            let stem = name.split('.').next().unwrap_or(&name);
            matches!(
                stem,
                "secret" | "secrets" | "credential" | "credentials" | "private_key"
            )
        })
}

fn contains_bearer_token(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    for (index, _) in lower.match_indices("bearer") {
        let before_is_boundary = index == 0
            || !lower.as_bytes()[index - 1].is_ascii_alphanumeric()
                && lower.as_bytes()[index - 1] != b'_';
        let after_index = index + "bearer".len();
        let after_is_space = lower
            .as_bytes()
            .get(after_index)
            .is_some_and(u8::is_ascii_whitespace);
        if !before_is_boundary || !after_is_space {
            continue;
        }

        let candidate = text[after_index..]
            .trim_start()
            .trim_start_matches([':', '=', '"', '\''])
            .split(|character: char| {
                character.is_ascii_whitespace()
                    || matches!(character, '"' | '\'' | ',' | ';' | ')' | ']' | '}')
            })
            .next()
            .unwrap_or_default()
            .trim();
        let lower_candidate = candidate.to_ascii_lowercase();
        let prose_word = matches!(
            lower_candidate.as_str(),
            "auth"
                | "authentication"
                | "authorization"
                | "credential"
                | "credentials"
                | "token"
                | "tokens"
                | "is"
                | "requires"
                | "required"
                | "supported"
        );
        if !candidate.is_empty()
            && !prose_word
            && candidate.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(character, '-' | '_' | '.' | '~' | '+' | '/' | '=')
            })
        {
            return true;
        }
    }
    false
}

fn contains_json_web_token(text: &str) -> bool {
    text.split(|character: char| {
        !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '='))
    })
    .any(looks_like_json_web_token)
}

fn looks_like_json_web_token(candidate: &str) -> bool {
    let mut segments = candidate.split('.');
    let Some(header) = segments.next() else {
        return false;
    };
    let Some(payload) = segments.next() else {
        return false;
    };
    let Some(signature) = segments.next() else {
        return false;
    };
    segments.next().is_none()
        && header.starts_with("eyJ")
        && header.len() >= 8
        && payload.len() >= 8
        && signature.len() >= 8
        && [header, payload, signature].iter().all(|segment| {
            segment.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
        })
}

fn contains_aws_access_key_id(text: &str) -> bool {
    const AWS_ACCESS_KEY_PREFIXES: [&str; 8] = [
        "AKIA", "ASIA", "AIDA", "AROA", "AIPA", "ANPA", "ANVA", "ASCA",
    ];

    text.split(|character: char| !character.is_ascii_alphanumeric())
        .any(|candidate| {
            candidate.len() == 20
                && candidate
                    .chars()
                    .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
                && AWS_ACCESS_KEY_PREFIXES
                    .iter()
                    .any(|prefix| candidate.starts_with(prefix))
        })
}

fn contains_named_assignment(text: &str, accepted_keys: &[&str]) -> bool {
    text.lines().any(|line| {
        let separator = line.find('=').or_else(|| line.find(':'));
        let Some(separator) = separator else {
            return false;
        };
        let (left, right_with_separator) = line.split_at(separator);
        let value = right_with_separator[1..]
            .trim()
            .trim_matches(['"', '\'', ',', ';', '}', ']']);
        if value.is_empty() || matches!(value.to_ascii_lowercase().as_str(), "null" | "none") {
            return false;
        }

        left.split(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        })
        .map(canonical_secret_key)
        .any(|key| accepted_keys.contains(&key.as_str()))
    })
}

fn canonical_secret_key(key: &str) -> String {
    key.chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}

fn contains_database_uri_credentials(text: &str) -> bool {
    const DATABASE_SCHEMES: [&str; 9] = [
        "postgresql://",
        "postgres://",
        "cockroachdb://",
        "mongodb+srv://",
        "mongodb://",
        "mariadb://",
        "mysql://",
        "rediss://",
        "redis://",
    ];

    let lower = text.to_ascii_lowercase();
    for scheme in DATABASE_SCHEMES {
        let mut search_start = 0;
        while let Some(relative_index) = lower[search_start..].find(scheme) {
            let uri_start = search_start + relative_index;
            let uri_end = text[uri_start..]
                .find(|character: char| {
                    character.is_ascii_whitespace()
                        || matches!(character, '"' | '\'' | '<' | '>' | ')' | ']' | '}')
                })
                .map_or(text.len(), |end| uri_start + end);
            if database_uri_has_credentials(&text[uri_start..uri_end], scheme) {
                return true;
            }
            search_start = uri_start + scheme.len();
        }
    }
    false
}

fn database_uri_has_credentials(uri: &str, scheme: &str) -> bool {
    let remainder = &uri[scheme.len()..];
    let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    if let Some(at) = authority.rfind('@') {
        let user_info = percent_decode(&authority[..at]);
        if user_info
            .split_once(':')
            .is_some_and(|(user, password)| !user.is_empty() && !password.is_empty())
        {
            return true;
        }
    }

    let Some(query_start) = uri.find('?') else {
        return false;
    };
    let decoded_query =
        percent_decode(uri[query_start + 1..].split('#').next().unwrap_or_default());
    decoded_query.split(['&', ';']).any(|parameter| {
        let Some((key, value)) = parameter.split_once('=') else {
            return false;
        };
        let key = canonical_secret_key(key);
        !value.is_empty()
            && matches!(
                key.as_str(),
                "password"
                    | "passwd"
                    | "pwd"
                    | "passphrase"
                    | "secret"
                    | "token"
                    | "sslkey"
                    | "sslpassword"
            )
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            decoded.push((high << 4) | low);
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn contains_private_key_header(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim().to_ascii_uppercase();
        line.strip_prefix("-----BEGIN ")
            .and_then(|label| label.strip_suffix("-----"))
            .is_some_and(|label| label == "PRIVATE KEY" || label.ends_with(" PRIVATE KEY"))
    })
}

fn contains_generic_secret_token(text: &str) -> bool {
    text.split(|character: char| {
        !(character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    })
    .any(|candidate| {
        candidate
            .get(..3)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("sk-"))
            && candidate.len() >= 10
    })
}

pub fn scan_filesystem(root: impl AsRef<Path>) -> Result<Vec<FilesystemEntry>, ScanError> {
    let root = root.as_ref();
    fs::symlink_metadata(root).map_err(|source| ScanError::RootMetadata {
        path: root.to_path_buf(),
        source,
    })?;

    let mut entries = Vec::new();
    scan_path(root, root, &mut entries)?;
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(entries)
}

fn scan_path(
    root: &Path,
    path: &Path,
    entries: &mut Vec<FilesystemEntry>,
) -> Result<(), ScanError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| ScanError::Metadata {
        path: path.to_path_buf(),
        source,
    })?;
    let file_type = metadata.file_type();
    let kind = if file_type.is_symlink() {
        FilesystemEntryKind::Symlink
    } else if file_type.is_dir() {
        FilesystemEntryKind::Directory
    } else if file_type.is_file() {
        FilesystemEntryKind::File
    } else {
        FilesystemEntryKind::Other
    };
    let relative_path = relative_path_string(root, path)?;
    let symlink_target = if file_type.is_symlink() {
        Some(
            fs::read_link(path)
                .map_err(|source| ScanError::SymlinkTarget {
                    path: path.to_path_buf(),
                    source,
                })?
                .to_string_lossy()
                .into_owned(),
        )
    } else {
        None
    };

    entries.push(FilesystemEntry {
        relative_path,
        kind,
        size_bytes: if file_type.is_file() {
            metadata.len()
        } else {
            0
        },
        readonly: metadata.permissions().readonly(),
        is_symlink: file_type.is_symlink(),
        symlink_target,
        classification: classify_path(path, kind),
    });

    if file_type.is_dir() && !file_type.is_symlink() {
        let mut children = Vec::new();
        let read_dir = fs::read_dir(path).map_err(|source| ScanError::ReadDir {
            path: path.to_path_buf(),
            source,
        })?;
        for child in read_dir {
            let child = child.map_err(|source| ScanError::Entry {
                path: path.to_path_buf(),
                source,
            })?;
            children.push(child.path());
        }
        children.sort();
        for child in children {
            scan_path(root, &child, entries)?;
        }
    }

    Ok(())
}

fn relative_path_string(root: &Path, path: &Path) -> Result<String, ScanError> {
    if root == path {
        return Ok(".".to_string());
    }
    let relative = path.strip_prefix(root).unwrap_or(path);
    let value = relative
        .to_str()
        .ok_or_else(|| ScanError::NonUtf8Path {
            path: path.to_path_buf(),
        })?
        .replace('\\', "/");
    Ok(value)
}

fn classify_path(path: &Path, kind: FilesystemEntryKind) -> FileClassification {
    if kind == FilesystemEntryKind::Directory {
        return FileClassification::Directory;
    }

    let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
        return FileClassification::Other;
    };
    if file_name == "Cargo.toml" {
        return FileClassification::CargoManifest;
    }
    if file_name == "Cargo.lock" {
        return FileClassification::CargoLock;
    }
    if file_name.ends_with(".rs") {
        return FileClassification::RustSource;
    }
    if file_name.starts_with('.') {
        return FileClassification::Hidden;
    }

    let mut has_vendor_component = false;
    let mut has_generated_component = false;
    for component in path.components() {
        let name = component.as_os_str().to_string_lossy();
        has_vendor_component |= matches!(name.as_ref(), "vendor" | ".cargo" | "target");
        has_generated_component |= matches!(name.as_ref(), "target" | "generated" | "out");
    }
    if has_vendor_component {
        FileClassification::Vendor
    } else if has_generated_component {
        FileClassification::Generated
    } else if kind == FilesystemEntryKind::File {
        FileClassification::NonRustAsset
    } else {
        FileClassification::Other
    }
}

pub fn prove_no_mutation(
    repo_path: impl AsRef<Path>,
    operation: impl AsRef<str>,
    run_read_only_operation: impl FnOnce(),
) -> Result<NoMutationProof, NoMutationError> {
    let repo_path = repo_path.as_ref();
    let before = capture_git_repo_snapshot(repo_path)?;
    run_read_only_operation();
    let after = capture_git_repo_snapshot(repo_path)?;
    let pre_existing_dirty = !before.is_clean();
    let mutation_detected = before != after;
    let status = if git_unavailable_snapshot(&before) || git_unavailable_snapshot(&after) {
        NoMutationStatus::Degraded
    } else if mutation_detected {
        NoMutationStatus::Mutated
    } else {
        NoMutationStatus::Proven
    };
    let degradation_reason = if status == NoMutationStatus::Degraded {
        Some("git status was unavailable; proof is degraded".to_string())
    } else {
        None
    };

    Ok(NoMutationProof {
        operation: operation.as_ref().to_string(),
        status,
        before,
        after,
        pre_existing_dirty,
        mutation_detected,
        degradation_reason,
    })
}

pub fn capture_git_repo_snapshot(
    repo_path: impl AsRef<Path>,
) -> Result<GitRepoSnapshot, NoMutationError> {
    let repo_path = repo_path.as_ref();
    let status_porcelain = git_status_porcelain(repo_path);
    let file_manifest_hash = file_manifest_hash(repo_path)?;
    Ok(GitRepoSnapshot {
        status_porcelain,
        file_manifest_hash,
    })
}

fn git_status_porcelain(repo_path: &Path) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["status", "--porcelain=v1"])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        Ok(output) => format!(
            "__GIT_UNAVAILABLE__ status={} stderr={}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr)
        ),
        Err(error) => format!("__GIT_UNAVAILABLE__ error={error}"),
    }
}

fn git_unavailable_snapshot(snapshot: &GitRepoSnapshot) -> bool {
    snapshot.status_porcelain.starts_with("__GIT_UNAVAILABLE__")
}

fn file_manifest_hash(root: &Path) -> Result<String, NoMutationError> {
    let mut entries = Vec::new();
    collect_manifest_hash_entries(root, root, &mut entries)?;
    entries.sort();
    let mut hasher = Sha256::new();
    for entry in entries {
        hasher.update(entry.as_bytes());
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_manifest_hash_entries(
    root: &Path,
    path: &Path,
    entries: &mut Vec<String>,
) -> Result<(), NoMutationError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| NoMutationError::Snapshot {
        path: path.to_path_buf(),
        source,
    })?;
    if path.file_name().and_then(|value| value.to_str()) == Some(".git") {
        return Ok(());
    }
    if metadata.is_dir() {
        let mut children = Vec::new();
        for child in fs::read_dir(path).map_err(|source| NoMutationError::Snapshot {
            path: path.to_path_buf(),
            source,
        })? {
            children.push(
                child
                    .map_err(|source| NoMutationError::Snapshot {
                        path: path.to_path_buf(),
                        source,
                    })?
                    .path(),
            );
        }
        children.sort();
        for child in children {
            collect_manifest_hash_entries(root, &child, entries)?;
        }
    } else if metadata.is_file() {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_str()
            .ok_or_else(|| NoMutationError::NonUtf8Path {
                path: path.to_path_buf(),
            })?
            .replace('\\', "/");
        let bytes = fs::read(path).map_err(|source| NoMutationError::Snapshot {
            path: path.to_path_buf(),
            source,
        })?;
        let content_hash = format!("{:x}", Sha256::digest(&bytes));
        entries.push(format!("{relative}:{content_hash}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // Test lane: default
    // Defends: the core schema models stay structured and stable for the package workspace.
    #[test]
    fn schema_tables_convert_into_rows() {
        let rows = table_inventory();
        assert_eq!(rows[0].table, "codedb_contexts");
        assert_eq!(rows[4].status, "available");
        assert!(rows.iter().any(|row| row.table == "source_blobs"));
        assert!(rows.iter().any(|row| row.table == "blob_policies"));
        assert!(
            rows.iter()
                .any(|row| row.table == "agent_harness_export_manifests")
        );
        assert!(
            rows.iter()
                .any(|row| row.table == "agent_harness_materialization_plan")
        );
    }

    // Test lane: default
    // Defends: identity keys remain explicit and deterministic for later capture layers.
    #[test]
    fn workspace_identity_is_deterministic() {
        let identity = workspace_identity();
        assert_eq!(identity.schema_version.as_tuple(), (1, 0, 0));
        assert_eq!(identity.object_kind, "identity");
        assert_eq!(identity.stable_name, "workspace_identity");
    }

    // Test lane: default
    // Defends: CDB017 scanner rows must be deterministic for repeated fixture scans.
    #[test]
    fn filesystem_scan_rows_are_stable() {
        let root = temp_fixture_root();
        fs::create_dir_all(root.join("src")).expect("create src");
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"fixture\"\n")
            .expect("write manifest");
        fs::write(root.join("src/lib.rs"), "pub fn fixture() {}\n").expect("write rust source");
        fs::write(root.join("README.md"), "fixture docs\n").expect("write asset");

        let first = scan_filesystem(&root).expect("first scan");
        let second = scan_filesystem(&root).expect("second scan");

        assert_eq!(first, second);
        assert_eq!(first[0].relative_path, ".");
        assert!(first.iter().any(|entry| {
            entry.relative_path == "Cargo.toml"
                && entry.kind == FilesystemEntryKind::File
                && entry.classification == FileClassification::CargoManifest
        }));
        assert!(first.iter().any(|entry| {
            entry.relative_path == "src/lib.rs"
                && entry.kind == FilesystemEntryKind::File
                && entry.classification == FileClassification::RustSource
        }));
        assert!(first.iter().any(|entry| {
            entry.relative_path == "README.md"
                && entry.classification == FileClassification::NonRustAsset
        }));

        fs::remove_dir_all(&root).ok();
    }

    // Test lane: default
    // Defends: CDB081 platforms without symlink creation support preserve link metadata as a safe fallback.
    #[test]
    fn symlink_materialization_records_metadata_only_fallback() {
        let entries = vec![FilesystemEntry {
            relative_path: "link.txt".to_string(),
            kind: FilesystemEntryKind::Symlink,
            size_bytes: 0,
            readonly: false,
            is_symlink: true,
            symlink_target: Some("target.txt".to_string()),
            classification: FileClassification::Other,
        }];

        let rows = symlink_materialization_rows(&entries, false);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].table, "platform_materialization_capabilities");
        assert_eq!(
            rows[0].status,
            SymlinkMaterializationStatus::MetadataOnlyFallback.as_str()
        );
        assert_eq!(rows[0].relative_path, "link.txt");
        assert!(rows[0].note.contains("safe fallback"));
        assert!(rows[0].note.contains("target.txt"));
        assert!(rows[0].note.contains("do not materialize as regular file"));
    }

    // Test lane: default
    // Defends: CDB081 Unix symlink scans capture the link target without following it.
    #[cfg(unix)]
    #[test]
    fn symlink_scan_records_target_and_supported_materialization_row() {
        use std::os::unix::fs::symlink;

        let root = temp_fixture_root();
        fs::create_dir_all(&root).expect("create root");
        fs::write(root.join("target.txt"), "symlink target\n").expect("write target");
        symlink("target.txt", root.join("link.txt")).expect("create symlink");

        let entries = scan_filesystem(&root).expect("scan symlink fixture");
        let link = entries
            .iter()
            .find(|entry| entry.relative_path == "link.txt")
            .expect("link entry");
        assert_eq!(link.kind, FilesystemEntryKind::Symlink);
        assert!(link.is_symlink);
        assert_eq!(link.symlink_target.as_deref(), Some("target.txt"));

        let rows = symlink_materialization_rows(&entries, true);
        let row = rows
            .iter()
            .find(|row| row.relative_path == "link.txt")
            .expect("symlink materialization row");
        assert_eq!(row.status, SymlinkMaterializationStatus::Supported.as_str());
        assert_eq!(row.platform, std::env::consts::OS);
        assert!(row.note.contains("target.txt"));

        fs::remove_dir_all(&root).ok();
    }

    // Test lane: default
    // Defends: CDB018 source capture must record exact metadata without exporting raw source.
    #[test]
    fn source_metadata_is_metadata_only_by_default() {
        let root = temp_fixture_root();
        fs::create_dir_all(&root).expect("create root");
        let source_path = root.join("src.rs");
        fs::write(&source_path, "pub fn source() {}\n").expect("write source");

        let metadata = capture_source_metadata(&root, &source_path).expect("capture metadata");
        assert_eq!(metadata.relative_path, "src.rs");
        assert_eq!(metadata.byte_len, 19);
        assert_eq!(metadata.sha256.len(), 64);
        assert_eq!(metadata.encoding_status, TextEncodingStatus::Utf8);
        assert_eq!(metadata.newline_style, NewlineStyle::Lf);
        assert!(!metadata.has_secret_like_material);
        assert_eq!(metadata.default_mode, SourceBlobMode::MetadataOnly);
        assert!(!metadata.export_raw_by_default);

        fs::remove_dir_all(&root).ok();
    }

    // Test lane: default
    // Defends: secret-looking source must become policy metadata rather than raw export.
    #[test]
    fn secret_like_source_is_redacted_by_policy() {
        let root = temp_fixture_root();
        fs::create_dir_all(&root).expect("create root");
        let source_path = root.join("secret.rs");
        fs::write(&source_path, "const API_KEY: &str = \"sk-test-value\";\n")
            .expect("write source");

        let metadata = capture_source_metadata(&root, &source_path).expect("capture metadata");
        let policy = source_policy_row(&metadata);
        assert!(metadata.has_secret_like_material);
        assert_eq!(metadata.default_mode, SourceBlobMode::RedactedExport);
        assert!(!metadata.export_raw_by_default);
        assert!(!policy.raw_export_allowed);
        assert!(policy.reason.contains("raw source export disabled"));
        assert!(!policy.reason.contains("sk-test-value"));

        fs::remove_dir_all(&root).ok();
    }

    // Test lane: default
    // Defends: CDB073 change plans are reviewable graph rows, not source mutations.
    #[test]
    fn change_plan_graph_is_reviewable_without_apply() {
        let graph = ChangePlanGraph {
            plan: ChangePlanRoot {
                plan_id: "plan:cdb073:review",
                source_snapshot_id: "snapshot:sha256:before",
                status: ChangePlanStatus::Reviewed,
                created_at: "2026-07-02T18:00:00Z",
            },
            nodes: vec![ChangePlanNode {
                node_id: "node:docs-round-trip",
                object_id: "object:docs/ROUND_TRIP_PROOF.md",
                change_kind: ChangeKind::Update,
            }],
            edges: vec![ChangePlanEdge {
                from_node_id: "node:docs-round-trip",
                to_node_id: "node:proof-gate",
                edge_kind: "requires_validation",
            }],
        };

        let rows = change_plan_table_rows(&graph);

        assert!(!graph.status_allows_source_apply());
        assert!(rows.iter().any(|row| {
            row.table == "change_plans"
                && row.status == "reviewed"
                && row.note.contains("plan:cdb073:review")
        }));
        assert!(rows.iter().any(|row| {
            row.table == "change_plan_nodes"
                && row.rows == 1
                && row.note.contains("object:docs/ROUND_TRIP_PROOF.md")
        }));
        assert!(rows.iter().any(|row| {
            row.table == "change_plan_edges"
                && row.rows == 1
                && row.note.contains("requires_validation")
        }));
    }

    // Test lane: default
    // Defends: CDB073 detects source drift before any apply gate can mutate source.
    #[test]
    fn source_snapshot_drift_blocks_change_plan_before_apply() {
        let graph = ChangePlanGraph {
            plan: ChangePlanRoot {
                plan_id: "plan:cdb073:drift",
                source_snapshot_id: "snapshot:sha256:before",
                status: ChangePlanStatus::ApprovedForIsolatedPatch,
                created_at: "2026-07-02T18:00:00Z",
            },
            nodes: vec![ChangePlanNode {
                node_id: "node:src",
                object_id: "object:src/lib.rs",
                change_kind: ChangeKind::Update,
            }],
            edges: Vec::new(),
        };

        let conflicts = detect_plan_conflicts(&graph, "snapshot:sha256:after");

        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].plan_id, "plan:cdb073:drift");
        assert_eq!(conflicts[0].conflict_kind, PlanConflictKind::SourceDrift);
        assert!(conflicts[0].message.contains("snapshot:sha256:before"));
        assert!(conflicts[0].message.contains("snapshot:sha256:after"));
    }

    // Test lane: default
    // Defends: CDB074 patch artifacts are generated only outside the source checkout.
    #[test]
    fn isolated_patch_artifact_refuses_source_checkout_target() {
        let source_root = temp_fixture_root();
        fs::create_dir_all(&source_root).expect("create source");
        let err = generate_isolated_patch_artifact(
            &source_root,
            source_root.join("patches"),
            "codedb.patch",
            b"diff --git a/src/lib.rs b/src/lib.rs\n",
            "rescan:required",
        )
        .expect_err("source target must be rejected");

        assert!(matches!(err, PatchPlanError::TargetInsideSource { .. }));
        assert!(!source_root.join("patches/codedb.patch").exists());

        fs::remove_dir_all(&source_root).ok();
    }

    // Test lane: default
    // Defends: CDB074 writes patch bytes into an isolated worktree with a required proof gate.
    #[test]
    fn isolated_patch_artifact_writes_outside_source_checkout() {
        let source_root = temp_fixture_root();
        let isolated_root = source_root.with_file_name(format!(
            "{}_isolated",
            source_root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("codedb_core_fs_fixture")
        ));
        fs::create_dir_all(&source_root).expect("create source");
        fs::write(source_root.join("sentinel.txt"), "unchanged\n").expect("source sentinel");

        let patch_bytes = b"diff --git a/src/lib.rs b/src/lib.rs\n";
        let artifact = generate_isolated_patch_artifact(
            &source_root,
            &isolated_root,
            "patches/codedb.patch",
            patch_bytes,
            "rescan:isolated-worktree",
        )
        .expect("isolated patch artifact");

        assert_eq!(artifact.bytes, patch_bytes.len() as u64);
        assert_eq!(artifact.proof_gate, "rescan:isolated-worktree");
        assert!(artifact.path.starts_with(&isolated_root));
        assert_eq!(
            fs::read_to_string(source_root.join("sentinel.txt")).expect("source sentinel"),
            "unchanged\n"
        );
        assert!(isolated_root.join("patches/codedb.patch").exists());

        fs::remove_dir_all(&source_root).ok();
        fs::remove_dir_all(&isolated_root).ok();
    }

    // Test lane: default
    // Defends: CDB075 refuses source apply when approval provenance is incomplete.
    #[test]
    fn apply_gate_refuses_missing_operator_approval() {
        let graph = ChangePlanGraph {
            plan: ChangePlanRoot {
                plan_id: "plan:cdb075:missing-approval",
                source_snapshot_id: "snapshot:sha256:before",
                status: ChangePlanStatus::ApprovedForApply,
                created_at: "2026-07-02T18:10:00Z",
            },
            nodes: vec![ChangePlanNode {
                node_id: "node:src",
                object_id: "object:src/lib.rs",
                change_kind: ChangeKind::Update,
            }],
            edges: Vec::new(),
        };

        let err = validate_apply_gate(
            &graph,
            None,
            &StopConditionProof {
                proof_id: "stop:cdb075:clean",
                passed: true,
                evidence_ref: "logs/CDB075-apply-gate.log",
            },
            "recovery:quarantine-ready",
            "snapshot:sha256:before",
        )
        .expect_err("missing approval must refuse apply");

        assert_eq!(err, ApplyGateError::MissingOperatorDecision);
    }

    // Test lane: default
    // Defends: CDB075 allows apply intent only with approval, stop proof, recovery, and no drift.
    #[test]
    fn apply_gate_allows_only_complete_operator_provenance() {
        let graph = ChangePlanGraph {
            plan: ChangePlanRoot {
                plan_id: "plan:cdb075:approved",
                source_snapshot_id: "snapshot:sha256:before",
                status: ChangePlanStatus::ApprovedForApply,
                created_at: "2026-07-02T18:10:00Z",
            },
            nodes: vec![ChangePlanNode {
                node_id: "node:src",
                object_id: "object:src/lib.rs",
                change_kind: ChangeKind::Update,
            }],
            edges: Vec::new(),
        };
        let decision = OperatorDecision {
            decision_id: "decision:cdb075:approve",
            plan_id: "plan:cdb075:approved",
            actor: "operator:flexnetos",
            decided_at: "2026-07-02T18:10:30Z",
            decision: ApplyDecision::Approved,
            evidence_ref: "logs/CDB075-apply-gate.log",
            manual_decision_ref: "manual:cdb075:reviewed",
        };

        let report = validate_apply_gate(
            &graph,
            Some(&decision),
            &StopConditionProof {
                proof_id: "stop:cdb075:clean",
                passed: true,
                evidence_ref: "logs/CDB075-apply-gate.log",
            },
            "recovery:quarantine-ready",
            "snapshot:sha256:before",
        )
        .expect("complete provenance allows apply intent");

        assert_eq!(report.plan_id, "plan:cdb075:approved");
        assert_eq!(report.decision_id, "decision:cdb075:approve");
        assert_eq!(report.status, "approved_for_apply");
        assert_eq!(report.recovery_ref, "recovery:quarantine-ready");
        assert_eq!(report.rows.len(), 2);
        assert!(report.rows.iter().any(|row| {
            row.table == "operator_decisions"
                && row.note.contains("manual:cdb075:reviewed")
                && row.note.contains("operator:flexnetos")
                && row.note.contains("2026-07-02T18:10:30Z")
        }));
        assert!(report.rows.iter().any(|row| {
            row.table == "apply_attempts" && row.note.contains("recovery:quarantine-ready")
        }));
    }

    // Test lane: default
    // Defends: CDB089 apply approval must include decision ID, actor, timestamp, and evidence.
    #[test]
    fn apply_gate_refuses_incomplete_operator_decision_provenance() {
        let graph = ChangePlanGraph {
            plan: ChangePlanRoot {
                plan_id: "plan:cdb089:approval",
                source_snapshot_id: "snapshot:sha256:before",
                status: ChangePlanStatus::ApprovedForApply,
                created_at: "2026-07-02T18:35:00Z",
            },
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        let decision = OperatorDecision {
            decision_id: "decision:cdb089:approve",
            plan_id: "plan:cdb089:approval",
            actor: "operator:flexnetos",
            decided_at: "",
            decision: ApplyDecision::Approved,
            evidence_ref: "logs/CDB089-approval-provenance.log",
            manual_decision_ref: "manual:cdb089:reviewed",
        };

        let err = validate_apply_gate(
            &graph,
            Some(&decision),
            &StopConditionProof {
                proof_id: "stop:cdb089:clean",
                passed: true,
                evidence_ref: "logs/CDB089-approval-provenance.log",
            },
            "recovery:quarantine-ready",
            "snapshot:sha256:before",
        )
        .expect_err("missing decision timestamp must refuse apply");

        assert_eq!(err, ApplyGateError::MissingDecisionEvidence);
    }

    // Test lane: default
    // Defends: CDB087 stale approved plans cannot apply after source drift.
    #[test]
    fn stale_approved_plan_cannot_apply_silently() {
        let graph = ChangePlanGraph {
            plan: ChangePlanRoot {
                plan_id: "plan:cdb087:stale",
                source_snapshot_id: "snapshot:sha256:before",
                status: ChangePlanStatus::ApprovedForApply,
                created_at: "2026-07-02T18:25:00Z",
            },
            nodes: vec![ChangePlanNode {
                node_id: "node:src",
                object_id: "object:src/lib.rs",
                change_kind: ChangeKind::Update,
            }],
            edges: Vec::new(),
        };
        let decision = OperatorDecision {
            decision_id: "decision:cdb087:approve",
            plan_id: "plan:cdb087:stale",
            actor: "operator:flexnetos",
            decided_at: "2026-07-02T18:25:30Z",
            decision: ApplyDecision::Approved,
            evidence_ref: "logs/CDB087-source-drift.log",
            manual_decision_ref: "manual:cdb087:reviewed-before-drift",
        };

        let conflicts = detect_plan_conflicts(&graph, "snapshot:sha256:after");
        let err = validate_apply_gate(
            &graph,
            Some(&decision),
            &StopConditionProof {
                proof_id: "stop:cdb087:clean",
                passed: true,
                evidence_ref: "logs/CDB087-source-drift.log",
            },
            "recovery:quarantine-ready",
            "snapshot:sha256:after",
        )
        .expect_err("source drift must invalidate stale approved plans");

        assert_eq!(err, ApplyGateError::SourceDrift);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].plan_id, "plan:cdb087:stale");
        assert_eq!(conflicts[0].source_snapshot_id, "snapshot:sha256:before");
        assert_eq!(conflicts[0].conflict_kind, PlanConflictKind::SourceDrift);
        assert!(conflicts[0].message.contains("snapshot:sha256:after"));
    }

    // Test lane: default
    // Defends: CDB076 source drift becomes a sync conflict before store-to-source apply.
    #[test]
    fn bidirectional_sync_reports_source_drift_conflict() {
        let graph = ChangePlanGraph {
            plan: ChangePlanRoot {
                plan_id: "plan:cdb076:drift",
                source_snapshot_id: "snapshot:sha256:before",
                status: ChangePlanStatus::ApprovedForApply,
                created_at: "2026-07-02T18:15:00Z",
            },
            nodes: vec![ChangePlanNode {
                node_id: "node:src",
                object_id: "object:src/lib.rs",
                change_kind: ChangeKind::Update,
            }],
            edges: Vec::new(),
        };

        let report = evaluate_bidirectional_sync(
            &graph,
            BidirectionalSyncDirection::StoreToSource,
            "snapshot:sha256:drifted",
            "snapshot:sha256:after",
            "snapshot:sha256:after",
            "recovery:quarantine-ready",
        );

        assert_eq!(report.status, BidirectionalSyncStatus::Conflict);
        assert!(report.rows.iter().any(|row| {
            row.table == "plan_conflicts" && row.note.contains("snapshot:sha256:drifted")
        }));
    }

    // Test lane: default
    // Defends: CDB076 final re-scan must match the expected post-apply snapshot.
    #[test]
    fn bidirectional_sync_requires_matching_final_rescan() {
        let graph = ChangePlanGraph {
            plan: ChangePlanRoot {
                plan_id: "plan:cdb076:verified",
                source_snapshot_id: "snapshot:sha256:before",
                status: ChangePlanStatus::Applied,
                created_at: "2026-07-02T18:15:00Z",
            },
            nodes: vec![ChangePlanNode {
                node_id: "node:src",
                object_id: "object:src/lib.rs",
                change_kind: ChangeKind::Update,
            }],
            edges: Vec::new(),
        };

        let report = evaluate_bidirectional_sync(
            &graph,
            BidirectionalSyncDirection::SourceToStore,
            "snapshot:sha256:before",
            "snapshot:sha256:after",
            "snapshot:sha256:after",
            "recovery:quarantine-ready",
        );

        assert_eq!(report.status, BidirectionalSyncStatus::Verified);
        assert!(report.rows.iter().any(|row| {
            row.table == "sync_verifications"
                && row.status == "verified"
                && row.note.contains("source_to_store")
        }));

        let recovery = evaluate_bidirectional_sync(
            &graph,
            BidirectionalSyncDirection::SourceToStore,
            "snapshot:sha256:before",
            "snapshot:sha256:after",
            "snapshot:sha256:unexpected",
            "recovery:quarantine-ready",
        );

        assert_eq!(recovery.status, BidirectionalSyncStatus::RecoveryRequired);
        assert!(recovery.rows.iter().any(|row| {
            row.table == "recovery_rows" && row.note.contains("snapshot:sha256:unexpected")
        }));
    }

    // Test lane: default
    // Defends: CDB088 failed materialization records audit rows after source restore.
    #[test]
    fn failed_materialization_recovery_records_audit_and_restored_snapshot() {
        let graph = ChangePlanGraph {
            plan: ChangePlanRoot {
                plan_id: "plan:cdb088:materialization",
                source_snapshot_id: "snapshot:sha256:before",
                status: ChangePlanStatus::ApprovedForApply,
                created_at: "2026-07-02T18:30:00Z",
            },
            nodes: vec![ChangePlanNode {
                node_id: "node:src",
                object_id: "object:src/lib.rs",
                change_kind: ChangeKind::Update,
            }],
            edges: Vec::new(),
        };

        let report = record_failed_apply_recovery(
            &graph,
            &FailedApplyRecoveryInput {
                attempt_id: "attempt:cdb088:materialization",
                plan_id: "plan:cdb088:materialization",
                kind: FailedApplyKind::Materialization,
                failure_ref: "logs/CDB088-failed-apply-recovery.log",
                observed_snapshot_id: "snapshot:sha256:partial",
                restored_snapshot_id: "snapshot:sha256:before",
                quarantine_ref: "quarantine:cdb088:partial-output",
            },
        )
        .expect("restored source snapshot records recovery");

        assert_eq!(report.status, BidirectionalSyncStatus::Recovered);
        assert!(report.rows.iter().any(|row| {
            row.table == "apply_attempts"
                && row.status == "failed"
                && row.note.contains("materialization")
                && row.note.contains("logs/CDB088-failed-apply-recovery.log")
        }));
        assert!(report.rows.iter().any(|row| {
            row.table == "recovery_rows"
                && row.status == "recovered"
                && row.note.contains("snapshot:sha256:partial")
                && row.note.contains("snapshot:sha256:before")
                && row.note.contains("quarantine:cdb088:partial-output")
        }));
    }

    // Test lane: default
    // Defends: CDB088 recovery cannot complete while source/worktree snapshot is still changed.
    #[test]
    fn failed_apply_recovery_requires_restored_source_snapshot() {
        let graph = ChangePlanGraph {
            plan: ChangePlanRoot {
                plan_id: "plan:cdb088:apply",
                source_snapshot_id: "snapshot:sha256:before",
                status: ChangePlanStatus::ApprovedForApply,
                created_at: "2026-07-02T18:30:00Z",
            },
            nodes: Vec::new(),
            edges: Vec::new(),
        };

        let err = record_failed_apply_recovery(
            &graph,
            &FailedApplyRecoveryInput {
                attempt_id: "attempt:cdb088:apply",
                plan_id: "plan:cdb088:apply",
                kind: FailedApplyKind::Apply,
                failure_ref: "logs/CDB088-failed-apply-recovery.log",
                observed_snapshot_id: "snapshot:sha256:partial",
                restored_snapshot_id: "snapshot:sha256:still-partial",
                quarantine_ref: "quarantine:cdb088:partial-output",
            },
        )
        .expect_err("recovery must prove source/worktree restore");

        assert_eq!(err, FailedApplyRecoveryError::SourceNotRestored);
    }

    // Test lane: default
    // Defends: CDB028 proves a clean Git fixture remains unchanged by a read-only operation.
    #[test]
    fn clean_git_fixture_proves_no_mutation() {
        let root = temp_fixture_root();
        fs::create_dir_all(&root).expect("create root");
        init_git_fixture(&root);
        fs::write(root.join("src.rs"), "pub fn clean() {}\n").expect("write source");
        git(&root, ["add", "src.rs"]);
        git(&root, ["commit", "-m", "initial"]);

        let proof = prove_no_mutation(&root, "scan", || {
            let _ = scan_filesystem(&root).expect("scan fixture");
        })
        .expect("prove no mutation");

        assert_eq!(proof.status, NoMutationStatus::Proven);
        assert!(!proof.pre_existing_dirty);
        assert!(!proof.mutation_detected);
        assert!(proof.before.is_clean());
        assert_eq!(proof.before, proof.after);

        fs::remove_dir_all(&root).ok();
    }

    // Test lane: default
    // Defends: CDB028 records pre-existing dirty state without blaming the read-only operation.
    #[test]
    fn dirty_git_fixture_proves_no_new_mutation() {
        let root = temp_fixture_root();
        fs::create_dir_all(&root).expect("create root");
        init_git_fixture(&root);
        fs::write(root.join("src.rs"), "pub fn clean() {}\n").expect("write source");
        git(&root, ["add", "src.rs"]);
        git(&root, ["commit", "-m", "initial"]);
        fs::write(root.join("src.rs"), "pub fn dirty() {}\n").expect("dirty source");

        let proof = prove_no_mutation(&root, "scan", || {
            let _ = scan_filesystem(&root).expect("scan fixture");
        })
        .expect("prove no mutation");

        assert_eq!(proof.status, NoMutationStatus::Proven);
        assert!(proof.pre_existing_dirty);
        assert!(!proof.mutation_detected);
        assert!(!proof.before.is_clean());
        assert_eq!(proof.before, proof.after);

        fs::remove_dir_all(&root).ok();
    }

    // Defends: parallel CodeDB core tests reserve fixture roots before use so
    // one test can never delete another test's live source checkout.
    #[test]
    fn temp_fixture_roots_are_reserved_and_collision_free() {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| (0..16).map(|_| temp_fixture_root()).collect::<Vec<_>>())
            })
            .collect();
        let roots: Vec<PathBuf> = handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("fixture allocator thread"))
            .collect();
        let mut unique = roots.clone();
        unique.sort();
        unique.dedup();

        assert_eq!(unique.len(), roots.len(), "fixture roots must be unique");
        assert!(
            roots.iter().all(|root| root.is_dir()),
            "the allocator must reserve every returned root before exposing it"
        );

        for root in roots {
            fs::remove_dir_all(root).expect("remove reserved fixture root");
        }
    }

    fn temp_fixture_root() -> PathBuf {
        static NEXT_FIXTURE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        loop {
            let sequence = NEXT_FIXTURE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "codedb_core_fs_fixture_{}_{}",
                std::process::id(),
                sequence
            ));
            match fs::create_dir(&root) {
                Ok(()) => return root,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("reserve unique fixture root {}: {error}", root.display()),
            }
        }
    }

    fn init_git_fixture(root: &Path) {
        git(root, ["init"]);
        git(root, ["config", "user.email", "codedb@example.invalid"]);
        git(root, ["config", "user.name", "CodeDB Test"]);
    }

    fn git<const N: usize>(root: &Path, args: [&str; N]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
