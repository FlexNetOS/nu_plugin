#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the crate-private trusted compiler broker boundary is exercised only by in-crate security and compiler-observed tests"
    )
)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error as StdError;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use syn::{Attribute, Block, Expr, Item, Lit, Meta, Stmt, Visibility};

pub const STATUS: &str = "static_rust_item_inventory_available";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustItemRow {
    pub stable_id: String,
    pub context_id: String,
    pub relative_path: String,
    pub module_path: String,
    pub item_kind: RustItemKind,
    pub name: String,
    pub identity_kind: RustIdentityKind,
    pub identity_note: String,
    pub visibility: RustVisibility,
    pub confidence: StaticCaptureConfidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroInventory {
    pub definitions: Vec<MacroDefinitionRow>,
    pub invocations: Vec<MacroInvocationRow>,
    pub gaps: Vec<MacroCaptureGap>,
    pub expansion_gates: Vec<MacroExpansionGateRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcMacroInventory {
    pub crate_exports: Vec<ProcMacroCrateRow>,
    pub invocations: Vec<ProcMacroInvocationRow>,
    pub gaps: Vec<ProcMacroCaptureGap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildScriptInventory {
    pub scripts: Vec<BuildScriptRow>,
    pub instructions: Vec<BuildScriptInstructionRow>,
    pub gaps: Vec<BuildScriptCaptureGap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticIncludeInventory {
    pub edges: Vec<StaticIncludeEdgeRow>,
    pub gaps: Vec<StaticIncludeGap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeLinkInventory {
    pub libraries: Vec<NativeLibraryRow>,
    pub link_args: Vec<LinkArgRow>,
    pub link_search_paths: Vec<LinkSearchPathRow>,
    pub gaps: Vec<NativeLinkGap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticHashReport {
    pub semantic_hash: String,
    pub public_api_hash: String,
    pub semantic_inputs: Vec<String>,
    pub public_api_inputs: Vec<String>,
    pub limitation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustItemScan {
    pub source_sha256: String,
    pub rows: Vec<RustItemRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustStableIdentityMatch {
    pub stable_id: String,
    pub name: String,
    pub item_kind: RustItemKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustIdentityConflict {
    pub stable_id: String,
    pub kind: RustIdentityConflictKind,
    pub previous_source_sha256: String,
    pub current_source_sha256: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustIdentityConflictKind {
    UnstableAnonymousSourceShift,
    SameSourceScanMismatch,
}

impl RustIdentityConflictKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnstableAnonymousSourceShift => "unstable_anonymous_source_shift",
            Self::SameSourceScanMismatch => "same_source_scan_mismatch",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustIdentityComparisonStatus {
    RepeatScanVerified,
    SourceShiftStableNamedOnly,
    SourceShiftConflict,
    SameSourceConflict,
}

impl RustIdentityComparisonStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RepeatScanVerified => "repeat_scan_verified",
            Self::SourceShiftStableNamedOnly => "source_shift_stable_named_only",
            Self::SourceShiftConflict => "source_shift_conflict",
            Self::SameSourceConflict => "same_source_conflict",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustIdentityComparison {
    pub status: RustIdentityComparisonStatus,
    pub source_shifted: bool,
    pub stable_matches: Vec<RustStableIdentityMatch>,
    pub conflicts: Vec<RustIdentityConflict>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerEvidenceOptions {
    /// Records explicit execution intent, but is never sufficient authorization.
    ///
    /// Compiler execution additionally requires a crate-private, request-bound
    /// capability and the mandatory Linux sandbox.
    pub enabled: bool,
    pub rustc: PathBuf,
    pub rustdoc: PathBuf,
    pub edition: String,
    /// Overrides the derived crate name used for every compiler/rustdoc command.
    pub crate_name: Option<String>,
    /// Defaults to the host reported by the exact configured rustc.
    pub target: Option<String>,
    /// Explicit non-feature cfg values supplied to rustc and rustdoc.
    pub cfgs: Vec<String>,
    /// Cargo-style feature names supplied as `--cfg feature="..."`.
    pub features: Vec<String>,
    /// Prebuilt dependencies, including proc-macro dynamic libraries, whose
    /// bytes are hashed before compiler execution.
    pub externs: Vec<CompilerExtern>,
    /// Additional dependency search paths supplied to rustc and rustdoc.
    pub library_search_paths: Vec<PathBuf>,
    /// Mandatory process-isolation backend. Arbitrary executables are rejected;
    /// only a trusted bubblewrap installation is accepted.
    pub sandbox: CompilerSandboxOptions,
}

impl Default for CompilerEvidenceOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            rustc: std::env::var_os("RUSTC")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("rustc")),
            rustdoc: std::env::var_os("RUSTDOC")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("rustdoc")),
            edition: "2024".to_string(),
            crate_name: None,
            target: None,
            cfgs: Vec::new(),
            features: Vec::new(),
            externs: Vec::new(),
            library_search_paths: Vec::new(),
            sandbox: CompilerSandboxOptions::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerSandboxOptions {
    pub executable: PathBuf,
}

impl Default for CompilerSandboxOptions {
    fn default() -> Self {
        Self {
            executable: default_bwrap_path(),
        }
    }
}

/// Host-held authority for minting opaque capabilities for one exact compiler
/// evidence request.
///
/// The authority is intentionally separate from [`CompilerEvidenceOptions`]:
/// booleans, paths, approver strings, or other request-shaped data cannot be
/// converted into execution authorization.
struct CompilerExecutionApprovalAuthority {
    secret: [u8; 32],
    authority_id: String,
}

/// Opaque, request-bound execution authorization.
///
/// Clones share one atomic use state so a copied/replayed capability is refused.
#[derive(Clone)]
struct CompilerExecutionCapability {
    authority_id: String,
    request_digest: String,
    nonce: [u8; 32],
    authenticator: [u8; 32],
    used: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompilerExternKind {
    Library,
    ProcMacro,
}

impl CompilerExternKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::ProcMacro => "proc_macro",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerExtern {
    pub name: String,
    pub path: PathBuf,
    pub kind: CompilerExternKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerExternProvenance {
    pub name: String,
    pub path: PathBuf,
    pub kind: CompilerExternKind,
    pub artifact_sha256: String,
    pub artifact_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilerEvidenceCollectionStatus {
    CompilerObserved,
    EvidenceUnavailable,
}

impl CompilerEvidenceCollectionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CompilerObserved => "compiler_observed",
            Self::EvidenceUnavailable => "evidence_unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilerArtifactStatus {
    CompilerObserved,
    EvidenceUnavailable,
}

impl CompilerArtifactStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CompilerObserved => "compiler_observed",
            Self::EvidenceUnavailable => "evidence_unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompilerEvidenceArtifactKind {
    MacroExpansion,
    MacroResolution,
    MacroHygiene,
    Hir,
    Mir,
    RustdocPublicApi,
}

impl CompilerEvidenceArtifactKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MacroExpansion => "macro_expansion",
            Self::MacroResolution => "macro_resolution",
            Self::MacroHygiene => "macro_hygiene",
            Self::Hir => "hir",
            Self::Mir => "mir",
            Self::RustdocPublicApi => "rustdoc_public_api",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerArtifactEvidence {
    pub kind: CompilerEvidenceArtifactKind,
    pub status: CompilerArtifactStatus,
    pub command: Vec<String>,
    /// Full UTF-8 compiler or rustdoc evidence. Binary metadata is represented only
    /// by `evidence_sha256` and `evidence_bytes`.
    pub output: Option<String>,
    pub evidence_sha256: Option<String>,
    pub evidence_bytes: Option<usize>,
    /// Hash of the complete compiler input context used to produce this artifact.
    pub context_sha256: Option<String>,
    /// Hash of exact compiler/rustdoc binaries' identities and Nix sysroot paths.
    pub toolchain_sha256: Option<String>,
    /// Hash binding artifact kind, evidence bytes, context, and toolchain.
    pub pin_sha256: Option<String>,
    pub diagnostic: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerEvidenceGap {
    pub artifact: Option<CompilerEvidenceArtifactKind>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerToolchainProvenance {
    pub rustc_path: PathBuf,
    pub rustc_version: String,
    pub rustdoc_path: PathBuf,
    pub rustdoc_version: String,
    pub sysroot: PathBuf,
    pub target_libdir: PathBuf,
    pub host: String,
    pub toolchain_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerContextProvenance {
    pub source_path: PathBuf,
    pub source_sha256: String,
    pub crate_name: String,
    pub crate_type: String,
    pub edition: String,
    pub target: String,
    pub cfgs: Vec<String>,
    pub features: Vec<String>,
    /// The exact cfg surface printed by rustc for this target and explicit cfg set.
    pub compiler_cfg: Vec<String>,
    pub externs: Vec<CompilerExternProvenance>,
    pub library_search_paths: Vec<PathBuf>,
    /// Minimal, secret-free process environment used for every compiler command.
    pub environment: BTreeMap<String, String>,
    pub context_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerEvidenceReport {
    pub collection_status: CompilerEvidenceCollectionStatus,
    pub source_path: PathBuf,
    pub source_sha256: Option<String>,
    pub toolchain: Option<CompilerToolchainProvenance>,
    pub context: Option<CompilerContextProvenance>,
    pub artifacts: Vec<CompilerArtifactEvidence>,
    pub semantic_hash: Option<String>,
    pub public_api_hash: Option<String>,
    pub semantic_inputs: Vec<String>,
    pub public_api_inputs: Vec<String>,
    pub gaps: Vec<CompilerEvidenceGap>,
    /// Exact positive-path commands retained even when collection fails closed.
    pub operator_instructions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedCompilerEvidenceRequest {
    pub repo_path: PathBuf,
    pub source_path: PathBuf,
    pub evidence_dir: PathBuf,
    pub approver: String,
    pub task_id: String,
    pub before_state: String,
    pub cleanup_plan: String,
    pub options: CompilerEvidenceOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedCompilerEvidenceOutcome {
    pub approval_id: String,
    pub repo_path: PathBuf,
    pub evidence_dir: PathBuf,
    pub approver: String,
    pub task_id: String,
    pub before_state: String,
    pub cleanup_plan: String,
    pub report: CompilerEvidenceReport,
}

impl CompilerEvidenceReport {
    pub fn artifact(
        &self,
        kind: CompilerEvidenceArtifactKind,
    ) -> Option<&CompilerArtifactEvidence> {
        self.artifacts.iter().find(|artifact| artifact.kind == kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeLibraryRow {
    pub stable_id: String,
    pub context_id: String,
    pub relative_path: String,
    pub library: String,
    pub library_kind: Option<String>,
    pub confidence: StaticCaptureConfidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkArgRow {
    pub stable_id: String,
    pub context_id: String,
    pub relative_path: String,
    pub arg: String,
    pub confidence: StaticCaptureConfidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkSearchPathRow {
    pub stable_id: String,
    pub context_id: String,
    pub relative_path: String,
    pub path: String,
    pub search_kind: Option<String>,
    pub confidence: StaticCaptureConfidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeLinkGap {
    pub context_id: String,
    pub relative_path: String,
    pub missing_truth: NativeLinkMissingTruth,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NativeLinkMissingTruth {
    LinkerTool,
    LibraryAvailability,
    LinkResult,
}

impl NativeLinkMissingTruth {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LinkerTool => "linker_tool",
            Self::LibraryAvailability => "library_availability",
            Self::LinkResult => "link_result",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticIncludeEdgeRow {
    pub stable_id: String,
    pub context_id: String,
    pub relative_path: String,
    pub module_path: String,
    pub edge_kind: StaticIncludeEdgeKind,
    pub target_path: String,
    pub confidence: StaticCaptureConfidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StaticIncludeEdgeKind {
    Include,
    IncludeStr,
    IncludeBytes,
    PathAttribute,
}

impl StaticIncludeEdgeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Include => "include",
            Self::IncludeStr => "include_str",
            Self::IncludeBytes => "include_bytes",
            Self::PathAttribute => "path_attribute",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticIncludeGap {
    pub context_id: String,
    pub relative_path: String,
    pub module_path: String,
    pub edge_kind: StaticIncludeEdgeKind,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildScriptRow {
    pub stable_id: String,
    pub context_id: String,
    pub relative_path: String,
    pub is_canonical_build_rs: bool,
    pub confidence: StaticCaptureConfidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildScriptInstructionRow {
    pub stable_id: String,
    pub context_id: String,
    pub relative_path: String,
    pub function_name: String,
    pub macro_path: String,
    pub directive: String,
    pub value: String,
    pub raw_instruction: String,
    pub confidence: StaticCaptureConfidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildScriptCaptureGap {
    pub context_id: String,
    pub relative_path: String,
    pub missing_truth: BuildScriptMissingTruth,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BuildScriptMissingTruth {
    Execution,
    Environment,
    Stdout,
    Stderr,
    OutDirArtifacts,
}

impl BuildScriptMissingTruth {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Execution => "execution",
            Self::Environment => "environment",
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::OutDirArtifacts => "out_dir_artifacts",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcMacroCrateRow {
    pub stable_id: String,
    pub context_id: String,
    pub relative_path: String,
    pub module_path: String,
    pub name: String,
    pub export_kind: ProcMacroExportKind,
    pub confidence: StaticCaptureConfidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProcMacroExportKind {
    FunctionLike,
    Attribute,
    Derive,
}

impl ProcMacroExportKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FunctionLike => "function_like",
            Self::Attribute => "attribute",
            Self::Derive => "derive",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcMacroInvocationRow {
    pub stable_id: String,
    pub context_id: String,
    pub relative_path: String,
    pub module_path: String,
    pub macro_path: String,
    pub invocation_kind: ProcMacroInvocationKind,
    pub token_summary: String,
    pub confidence: StaticCaptureConfidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProcMacroInvocationKind {
    Attribute,
    Derive,
    FunctionLikeCandidate,
}

impl ProcMacroInvocationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Attribute => "attribute",
            Self::Derive => "derive",
            Self::FunctionLikeCandidate => "function_like_candidate",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcMacroCaptureGap {
    pub context_id: String,
    pub relative_path: String,
    pub module_path: String,
    pub macro_name: String,
    pub missing_truth: ProcMacroMissingTruth,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProcMacroMissingTruth {
    OutputTokenStream,
    Panic,
    Environment,
    FileAccess,
}

impl ProcMacroMissingTruth {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OutputTokenStream => "output_token_stream",
            Self::Panic => "panic",
            Self::Environment => "environment",
            Self::FileAccess => "file_access",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroDefinitionRow {
    pub stable_id: String,
    pub context_id: String,
    pub relative_path: String,
    pub module_path: String,
    pub name: String,
    pub matcher_summary: String,
    pub transcriber_summary: String,
    pub confidence: StaticCaptureConfidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroInvocationRow {
    pub stable_id: String,
    pub context_id: String,
    pub relative_path: String,
    pub module_path: String,
    pub macro_path: String,
    pub invocation_kind: MacroInvocationKind,
    pub token_summary: String,
    pub confidence: StaticCaptureConfidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MacroInvocationKind {
    Item,
    Statement,
    Expression,
}

impl MacroInvocationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Statement => "statement",
            Self::Expression => "expression",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroCaptureGap {
    pub context_id: String,
    pub relative_path: String,
    pub module_path: String,
    pub macro_name: String,
    pub missing_truth: MacroMissingTruth,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroExpansionGateRow {
    pub context_id: String,
    pub relative_path: String,
    pub module_path: String,
    pub macro_name: String,
    pub gate_status: MacroExpansionGateStatus,
    pub evidence_kind: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MacroExpansionGateStatus {
    Gap,
    CompilerObserved,
}

impl MacroExpansionGateStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gap => "gap",
            Self::CompilerObserved => "compiler_observed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MacroMissingTruth {
    Expansion,
    Hygiene,
}

impl MacroMissingTruth {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Expansion => "expansion",
            Self::Hygiene => "hygiene",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RustItemKind {
    Module,
    Function,
    Struct,
    Enum,
    Trait,
    TypeAlias,
    Const,
    Static,
    Impl,
    Use,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustIdentityKind {
    StableNamed,
    UnstableAnonymous,
}

impl RustIdentityKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StableNamed => "stable_named",
            Self::UnstableAnonymous => "unstable_anonymous",
        }
    }
}

impl RustItemKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Function => "function",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::TypeAlias => "type_alias",
            Self::Const => "const",
            Self::Static => "static",
            Self::Impl => "impl",
            Self::Use => "use",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RustVisibility {
    Public,
    Crate,
    Restricted,
    Private,
}

impl RustVisibility {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Crate => "crate",
            Self::Restricted => "restricted",
            Self::Private => "private",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticCaptureConfidence {
    SyntaxOnly,
}

impl StaticCaptureConfidence {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SyntaxOnly => "syntax_only",
        }
    }
}

#[derive(Debug)]
pub enum RustStaticError {
    Read { path: PathBuf, source: io::Error },
    Parse { path: PathBuf, source: syn::Error },
    NonUtf8Path { path: PathBuf },
}

impl Display for RustStaticError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(f, "failed to read Rust source {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(
                    f,
                    "failed to parse Rust source {}: {source}",
                    path.display()
                )
            }
            Self::NonUtf8Path { path } => write!(f, "path is not valid UTF-8: {}", path.display()),
        }
    }
}

impl StdError for RustStaticError {}

pub fn capture_rust_items(
    root: impl AsRef<Path>,
    source_path: impl AsRef<Path>,
    context_id: impl AsRef<str>,
) -> Result<Vec<RustItemRow>, RustStaticError> {
    let root = root.as_ref();
    let source_path = source_path.as_ref();
    let source = fs::read_to_string(source_path).map_err(|source| RustStaticError::Read {
        path: source_path.to_path_buf(),
        source,
    })?;
    let syntax = syn::parse_file(&source).map_err(|source| RustStaticError::Parse {
        path: source_path.to_path_buf(),
        source,
    })?;
    let relative_path = relative_path(root, source_path)?;
    let mut rows = Vec::new();
    collect_items(
        &syntax.items,
        context_id.as_ref(),
        &relative_path,
        "",
        &mut rows,
    );
    rows.sort_by(|left, right| {
        left.module_path
            .cmp(&right.module_path)
            .then_with(|| left.item_kind.cmp(&right.item_kind))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    Ok(rows)
}

/// Captures the source fingerprint beside the static rows so identity claims can
/// distinguish repeat scans from source-shift comparisons.
pub fn capture_rust_item_scan(
    root: impl AsRef<Path>,
    source_path: impl AsRef<Path>,
    context_id: impl AsRef<str>,
) -> Result<RustItemScan, RustStaticError> {
    let source_path = source_path.as_ref();
    let source = fs::read(source_path).map_err(|source| RustStaticError::Read {
        path: source_path.to_path_buf(),
        source,
    })?;
    let rows = capture_rust_items(root, source_path, context_id)?;
    Ok(RustItemScan {
        source_sha256: sha256_bytes(&source),
        rows,
    })
}

/// Compares two static scans without treating scan-order anonymous rows as stable
/// identities after any source-byte shift.
pub fn compare_rust_item_scans(
    previous: &RustItemScan,
    current: &RustItemScan,
) -> RustIdentityComparison {
    let source_shifted = previous.source_sha256 != current.source_sha256;
    if !source_shifted {
        if previous.rows == current.rows {
            return RustIdentityComparison {
                status: RustIdentityComparisonStatus::RepeatScanVerified,
                source_shifted: false,
                stable_matches: stable_identity_matches(previous, current),
                conflicts: Vec::new(),
            };
        }

        return RustIdentityComparison {
            status: RustIdentityComparisonStatus::SameSourceConflict,
            source_shifted: false,
            stable_matches: stable_identity_matches(previous, current),
            conflicts: vec![RustIdentityConflict {
                stable_id: String::new(),
                kind: RustIdentityConflictKind::SameSourceScanMismatch,
                previous_source_sha256: previous.source_sha256.clone(),
                current_source_sha256: current.source_sha256.clone(),
                reason: "identical source fingerprints produced different static rows".to_string(),
            }],
        };
    }

    let anonymous_ids = previous
        .rows
        .iter()
        .chain(&current.rows)
        .filter(|row| row.identity_kind == RustIdentityKind::UnstableAnonymous)
        .map(|row| row.stable_id.clone())
        .collect::<BTreeSet<_>>();
    let conflicts = anonymous_ids
        .into_iter()
        .map(|stable_id| RustIdentityConflict {
            stable_id,
            kind: RustIdentityConflictKind::UnstableAnonymousSourceShift,
            previous_source_sha256: previous.source_sha256.clone(),
            current_source_sha256: current.source_sha256.clone(),
            reason: "anonymous scan-order identity is not matched across a source shift"
                .to_string(),
        })
        .collect::<Vec<_>>();
    let status = if conflicts.is_empty() {
        RustIdentityComparisonStatus::SourceShiftStableNamedOnly
    } else {
        RustIdentityComparisonStatus::SourceShiftConflict
    };

    RustIdentityComparison {
        status,
        source_shifted: true,
        stable_matches: stable_identity_matches(previous, current),
        conflicts,
    }
}

fn stable_identity_matches(
    previous: &RustItemScan,
    current: &RustItemScan,
) -> Vec<RustStableIdentityMatch> {
    let previous_rows = previous
        .rows
        .iter()
        .filter(|row| row.identity_kind == RustIdentityKind::StableNamed)
        .map(|row| (row.stable_id.as_str(), row))
        .collect::<BTreeMap<_, _>>();
    let mut matches = current
        .rows
        .iter()
        .filter(|row| row.identity_kind == RustIdentityKind::StableNamed)
        .filter_map(|row| {
            previous_rows
                .get(row.stable_id.as_str())
                .map(|_| RustStableIdentityMatch {
                    stable_id: row.stable_id.clone(),
                    name: row.name.clone(),
                    item_kind: row.item_kind,
                })
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        left.item_kind
            .cmp(&right.item_kind)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.stable_id.cmp(&right.stable_id))
    });
    matches
}

pub fn capture_rust_macros(
    root: impl AsRef<Path>,
    source_path: impl AsRef<Path>,
    context_id: impl AsRef<str>,
) -> Result<MacroInventory, RustStaticError> {
    let root = root.as_ref();
    let source_path = source_path.as_ref();
    let source = fs::read_to_string(source_path).map_err(|source| RustStaticError::Read {
        path: source_path.to_path_buf(),
        source,
    })?;
    let syntax = syn::parse_file(&source).map_err(|source| RustStaticError::Parse {
        path: source_path.to_path_buf(),
        source,
    })?;
    let relative_path = relative_path(root, source_path)?;
    let mut inventory = MacroInventory {
        definitions: Vec::new(),
        invocations: Vec::new(),
        gaps: Vec::new(),
        expansion_gates: Vec::new(),
    };
    collect_macros(
        &syntax.items,
        context_id.as_ref(),
        &relative_path,
        "",
        &mut inventory,
    );
    inventory.definitions.sort_by(|left, right| {
        left.module_path
            .cmp(&right.module_path)
            .then_with(|| left.name.cmp(&right.name))
    });
    inventory.invocations.sort_by(|left, right| {
        left.module_path
            .cmp(&right.module_path)
            .then_with(|| left.macro_path.cmp(&right.macro_path))
            .then_with(|| left.invocation_kind.cmp(&right.invocation_kind))
            .then_with(|| left.token_summary.cmp(&right.token_summary))
    });
    inventory.gaps.sort_by(|left, right| {
        left.module_path
            .cmp(&right.module_path)
            .then_with(|| left.macro_name.cmp(&right.macro_name))
            .then_with(|| left.missing_truth.cmp(&right.missing_truth))
    });
    inventory.expansion_gates.sort_by(|left, right| {
        left.module_path
            .cmp(&right.module_path)
            .then_with(|| left.macro_name.cmp(&right.macro_name))
            .then_with(|| left.gate_status.cmp(&right.gate_status))
    });
    Ok(inventory)
}

pub fn capture_proc_macro_static(
    root: impl AsRef<Path>,
    source_path: impl AsRef<Path>,
    context_id: impl AsRef<str>,
) -> Result<ProcMacroInventory, RustStaticError> {
    let root = root.as_ref();
    let source_path = source_path.as_ref();
    let source = fs::read_to_string(source_path).map_err(|source| RustStaticError::Read {
        path: source_path.to_path_buf(),
        source,
    })?;
    let syntax = syn::parse_file(&source).map_err(|source| RustStaticError::Parse {
        path: source_path.to_path_buf(),
        source,
    })?;
    let relative_path = relative_path(root, source_path)?;
    let mut inventory = ProcMacroInventory {
        crate_exports: Vec::new(),
        invocations: Vec::new(),
        gaps: Vec::new(),
    };
    collect_proc_macros(
        &syntax.items,
        context_id.as_ref(),
        &relative_path,
        "",
        &mut inventory,
    );
    inventory.crate_exports.sort_by(|left, right| {
        left.module_path
            .cmp(&right.module_path)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.export_kind.cmp(&right.export_kind))
    });
    inventory.invocations.sort_by(|left, right| {
        left.module_path
            .cmp(&right.module_path)
            .then_with(|| left.macro_path.cmp(&right.macro_path))
            .then_with(|| left.invocation_kind.cmp(&right.invocation_kind))
            .then_with(|| left.token_summary.cmp(&right.token_summary))
    });
    inventory.gaps.sort_by(|left, right| {
        left.module_path
            .cmp(&right.module_path)
            .then_with(|| left.macro_name.cmp(&right.macro_name))
            .then_with(|| left.missing_truth.cmp(&right.missing_truth))
    });
    Ok(inventory)
}

pub fn capture_build_script_static(
    root: impl AsRef<Path>,
    source_path: impl AsRef<Path>,
    context_id: impl AsRef<str>,
) -> Result<BuildScriptInventory, RustStaticError> {
    let root = root.as_ref();
    let source_path = source_path.as_ref();
    let source = fs::read_to_string(source_path).map_err(|source| RustStaticError::Read {
        path: source_path.to_path_buf(),
        source,
    })?;
    let syntax = syn::parse_file(&source).map_err(|source| RustStaticError::Parse {
        path: source_path.to_path_buf(),
        source,
    })?;
    let relative_path = relative_path(root, source_path)?;
    let mut inventory = BuildScriptInventory {
        scripts: vec![BuildScriptRow {
            stable_id: stable_macro_id(
                context_id.as_ref(),
                &relative_path,
                "",
                "build_script",
                "build.rs",
                "",
            ),
            context_id: context_id.as_ref().to_string(),
            is_canonical_build_rs: source_path.file_name().and_then(|name| name.to_str())
                == Some("build.rs"),
            relative_path: relative_path.clone(),
            confidence: StaticCaptureConfidence::SyntaxOnly,
        }],
        instructions: Vec::new(),
        gaps: Vec::new(),
    };
    collect_build_script_instructions(
        &syntax.items,
        context_id.as_ref(),
        &relative_path,
        &mut inventory,
    );
    push_build_script_gaps(&mut inventory, context_id.as_ref(), &relative_path);
    inventory.instructions.sort_by(|left, right| {
        left.function_name
            .cmp(&right.function_name)
            .then_with(|| left.directive.cmp(&right.directive))
            .then_with(|| left.value.cmp(&right.value))
            .then_with(|| left.raw_instruction.cmp(&right.raw_instruction))
    });
    inventory.gaps.sort_by_key(|gap| gap.missing_truth);
    Ok(inventory)
}

pub fn capture_static_include_edges(
    root: impl AsRef<Path>,
    source_path: impl AsRef<Path>,
    context_id: impl AsRef<str>,
) -> Result<StaticIncludeInventory, RustStaticError> {
    let root = root.as_ref();
    let source_path = source_path.as_ref();
    let source = fs::read_to_string(source_path).map_err(|source| RustStaticError::Read {
        path: source_path.to_path_buf(),
        source,
    })?;
    let syntax = syn::parse_file(&source).map_err(|source| RustStaticError::Parse {
        path: source_path.to_path_buf(),
        source,
    })?;
    let relative_path = relative_path(root, source_path)?;
    let mut inventory = StaticIncludeInventory {
        edges: Vec::new(),
        gaps: Vec::new(),
    };
    collect_static_include_edges(
        &syntax.items,
        context_id.as_ref(),
        &relative_path,
        "",
        &mut inventory,
    );
    inventory.edges.sort_by(|left, right| {
        left.module_path
            .cmp(&right.module_path)
            .then_with(|| left.edge_kind.cmp(&right.edge_kind))
            .then_with(|| left.target_path.cmp(&right.target_path))
    });
    inventory.gaps.sort_by(|left, right| {
        left.module_path
            .cmp(&right.module_path)
            .then_with(|| left.edge_kind.cmp(&right.edge_kind))
            .then_with(|| left.reason.cmp(&right.reason))
    });
    Ok(inventory)
}

pub fn capture_native_link_static(
    root: impl AsRef<Path>,
    build_script_path: impl AsRef<Path>,
    context_id: impl AsRef<str>,
) -> Result<NativeLinkInventory, RustStaticError> {
    let build_script = capture_build_script_static(root, build_script_path, context_id.as_ref())?;
    let mut inventory = NativeLinkInventory {
        libraries: Vec::new(),
        link_args: Vec::new(),
        link_search_paths: Vec::new(),
        gaps: Vec::new(),
    };
    for instruction in &build_script.instructions {
        match instruction.directive.as_str() {
            "rustc-link-lib" => push_native_library(&mut inventory, instruction),
            "rustc-link-arg" => push_link_arg(&mut inventory, instruction),
            "rustc-link-search" => push_link_search_path(&mut inventory, instruction),
            _ => {}
        }
    }
    for script in &build_script.scripts {
        push_native_link_gaps(&mut inventory, &script.context_id, &script.relative_path);
    }
    inventory.libraries.sort_by(|left, right| {
        left.library
            .cmp(&right.library)
            .then_with(|| left.library_kind.cmp(&right.library_kind))
    });
    inventory
        .link_args
        .sort_by(|left, right| left.arg.cmp(&right.arg));
    inventory.link_search_paths.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.search_kind.cmp(&right.search_kind))
    });
    inventory.gaps.sort_by_key(|gap| gap.missing_truth);
    Ok(inventory)
}

pub fn semantic_hash_report(rows: &[RustItemRow]) -> SemanticHashReport {
    let mut semantic_inputs = rows.iter().map(semantic_hash_input).collect::<Vec<_>>();
    semantic_inputs.sort();
    let mut public_api_inputs = rows
        .iter()
        .filter(|row| row.visibility == RustVisibility::Public)
        .map(semantic_hash_input)
        .collect::<Vec<_>>();
    public_api_inputs.sort();

    SemanticHashReport {
        semantic_hash: hash_lines(&semantic_inputs),
        public_api_hash: hash_lines(&public_api_inputs),
        semantic_inputs,
        public_api_inputs,
        limitation:
            "static syntax hash excludes function bodies, type layout, macro expansion, and rustc semantic checks"
                .to_string(),
    }
}

impl CompilerExecutionApprovalAuthority {
    /// Creates an in-memory authority from kernel randomness.
    ///
    /// Trusted frontdoors must retain this object outside request/CLI data and
    /// mint a capability only after completing their durable operator-approval
    /// policy.
    fn new() -> Result<Self, String> {
        let secret = kernel_random_32()?;
        Ok(Self {
            authority_id: sha256_bytes(&secret),
            secret,
        })
    }

    /// Mints a single-use capability bound to source bytes and every
    /// execution-relevant compiler, extern, context, and sandbox option.
    fn approve(
        &self,
        source_path: impl AsRef<Path>,
        options: &CompilerEvidenceOptions,
    ) -> Result<CompilerExecutionCapability, String> {
        if !options.enabled {
            return Err("compiler execution intent is disabled".to_string());
        }
        let request_digest = compiler_execution_request_digest(source_path.as_ref(), options)?;
        let nonce = kernel_random_32()?;
        let authenticator =
            compiler_capability_authenticator(&self.secret, &request_digest, &nonce);
        Ok(CompilerExecutionCapability {
            authority_id: self.authority_id.clone(),
            request_digest,
            nonce,
            authenticator,
            used: Arc::new(AtomicBool::new(false)),
        })
    }
}

/// Refuses untrusted compiler execution requests.
///
/// `enabled = true` is intent data, not authority. This library intentionally
/// exposes no public authority, capability, or raw execution function. A
/// trusted broker must live outside the public library API and consume the
/// resulting non-executing artifact/report surface.
///
/// External crates cannot mint compiler-execution authority:
///
/// ```compile_fail
/// use codedb_rust_static::CompilerExecutionApprovalAuthority;
///
/// let _self_issued = CompilerExecutionApprovalAuthority::new().unwrap();
/// ```
///
/// External crates also cannot call the raw capability executor:
///
/// ```compile_fail
/// use std::path::Path;
///
/// use codedb_rust_static::{
///     CompilerEvidenceOptions, CompilerExecutionApprovalAuthority,
///     capture_compiler_evidence_with_capability,
/// };
///
/// let source = Path::new("untrusted.rs");
/// let options = CompilerEvidenceOptions {
///     enabled: true,
///     ..CompilerEvidenceOptions::default()
/// };
/// let authority = CompilerExecutionApprovalAuthority::new().unwrap();
/// let capability = authority.approve(source, &options).unwrap();
/// let _ = capture_compiler_evidence_with_capability(
///     &authority,
///     capability,
///     source,
///     options,
/// );
/// ```
pub fn capture_compiler_evidence(
    source_path: impl AsRef<Path>,
    options: CompilerEvidenceOptions,
) -> CompilerEvidenceReport {
    let source_path = source_path.as_ref().to_path_buf();
    let operator_instructions = compiler_operator_instructions();
    if !options.enabled {
        return compiler_evidence_unavailable(
            source_path,
            None,
            None,
            None,
            "compiler evidence collection is disabled; static inventory is not compiler evidence",
            operator_instructions,
        );
    }

    compiler_evidence_unavailable(
        source_path,
        None,
        None,
        None,
        "compiler execution requires an opaque request-bound capability; boolean and request-shaped data cannot authorize execution",
        operator_instructions,
    )
}

/// Runs the production compiler-observed lane after validating complete
/// operator provenance and an evidence destination outside the source repo.
///
/// The authority, capability, and raw executor remain crate-private. Callers
/// receive only the approved outcome and cannot mint or replay a capability.
pub fn capture_approved_compiler_evidence(
    request: ApprovedCompilerEvidenceRequest,
) -> Result<ApprovedCompilerEvidenceOutcome, String> {
    if !request.options.enabled {
        return Err("approved compiler evidence requires enabled execution intent".to_string());
    }
    for (field, value) in [
        ("approver", request.approver.as_str()),
        ("task_id", request.task_id.as_str()),
        ("before_state", request.before_state.as_str()),
        ("cleanup_plan", request.cleanup_plan.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(format!(
                "approved compiler evidence requires non-empty {field}"
            ));
        }
    }

    let repo_path = request
        .repo_path
        .canonicalize()
        .map_err(|error| format!("cannot resolve compiler source repository: {error}"))?;
    let source_path = request
        .source_path
        .canonicalize()
        .map_err(|error| format!("cannot resolve compiler source file: {error}"))?;
    if !source_path.is_file() || !source_path.starts_with(&repo_path) {
        return Err("compiler source must be a regular file inside the selected repository".into());
    }
    if !request.evidence_dir.is_absolute()
        || request
            .evidence_dir
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("compiler evidence directory must be an absolute normalized path".into());
    }
    let evidence_parent = request
        .evidence_dir
        .parent()
        .ok_or_else(|| "compiler evidence directory must have a parent".to_string())?
        .canonicalize()
        .map_err(|error| format!("cannot resolve compiler evidence parent: {error}"))?;
    let evidence_dir = evidence_parent.join(
        request
            .evidence_dir
            .file_name()
            .ok_or_else(|| "compiler evidence directory must have a final name".to_string())?,
    );
    if evidence_dir.starts_with(&repo_path) {
        return Err("compiler evidence directory must be outside the source repository".into());
    }

    let approval_id = sha256_bytes(
        format!(
            "{}\0{}\0{}\0{}\0{}\0{}\0{}",
            repo_path.display(),
            source_path.display(),
            evidence_dir.display(),
            request.approver,
            request.task_id,
            request.before_state,
            request.cleanup_plan,
        )
        .as_bytes(),
    );
    let authority = CompilerExecutionApprovalAuthority::new()?;
    let capability = authority.approve(&source_path, &request.options)?;
    let report = capture_compiler_evidence_with_capability(
        &authority,
        capability,
        &source_path,
        request.options,
    );
    Ok(ApprovedCompilerEvidenceOutcome {
        approval_id,
        repo_path,
        evidence_dir,
        approver: request.approver,
        task_id: request.task_id,
        before_state: request.before_state,
        cleanup_plan: request.cleanup_plan,
        report,
    })
}

/// Runs compiler-observed capture only after validating a single-use,
/// exact-request capability and preparing the mandatory Linux bubblewrap
/// sandbox.
///
/// No direct `rustc`, `rustdoc`, or supplied proc-macro execution fallback
/// exists. A missing, untrusted, or unusable sandbox produces only
/// `EvidenceUnavailable`.
fn capture_compiler_evidence_with_capability(
    authority: &CompilerExecutionApprovalAuthority,
    capability: CompilerExecutionCapability,
    source_path: impl AsRef<Path>,
    options: CompilerEvidenceOptions,
) -> CompilerEvidenceReport {
    let source_path = source_path.as_ref().to_path_buf();
    let operator_instructions = compiler_operator_instructions();
    if !options.enabled {
        return compiler_evidence_unavailable(
            source_path,
            None,
            None,
            None,
            "compiler evidence collection is disabled",
            operator_instructions,
        );
    }
    let request_digest = match compiler_execution_request_digest(&source_path, &options) {
        Ok(request_digest) => request_digest,
        Err(reason) => {
            return compiler_evidence_unavailable(
                source_path,
                None,
                None,
                None,
                &format!("cannot bind exact compiler request: {reason}"),
                operator_instructions,
            );
        }
    };
    if let Err(reason) =
        validate_compiler_execution_capability(authority, &capability, &request_digest)
    {
        return compiler_evidence_unavailable(
            source_path,
            None,
            None,
            None,
            &reason,
            operator_instructions,
        );
    }
    let scratch = match CompilerScratch::new() {
        Ok(scratch) => scratch,
        Err(error) => {
            return compiler_evidence_unavailable(
                source_path,
                None,
                None,
                None,
                &format!("cannot create compiler-evidence scratch directory: {error}"),
                operator_instructions,
            );
        }
    };
    let sandbox =
        match CompilerSandboxPlan::new(&options, &source_path, &scratch.path, &request_digest) {
            Ok(sandbox) => sandbox,
            Err(reason) => {
                return compiler_evidence_unavailable(
                    source_path,
                    None,
                    None,
                    None,
                    &format!("mandatory Linux sandbox unavailable: {reason}"),
                    operator_instructions,
                );
            }
        };

    capture_authorized_compiler_evidence(
        source_path,
        options,
        scratch,
        sandbox,
        operator_instructions,
    )
}

/// Internal compiler-observed lane. Every process spawned below is routed
/// through `sandbox`; keeping this function private prevents bypassing the
/// authorization and sandbox gates above.
fn capture_authorized_compiler_evidence(
    source_path: PathBuf,
    options: CompilerEvidenceOptions,
    scratch: CompilerScratch,
    sandbox: CompilerSandboxPlan,
    operator_instructions: Vec<String>,
) -> CompilerEvidenceReport {
    let source = match fs::read(&source_path) {
        Ok(source) => source,
        Err(error) => {
            return compiler_evidence_unavailable(
                source_path.clone(),
                None,
                None,
                None,
                &format!("cannot read source for compiler evidence: {error}"),
                operator_instructions,
            );
        }
    };
    let source_sha256 = sha256_bytes(&source);
    let canonical_source_path = fs::canonicalize(&source_path).unwrap_or(source_path.clone());
    let source_arg = match canonical_source_path.to_str() {
        Some(path) => path.to_string(),
        None => {
            return compiler_evidence_unavailable(
                source_path,
                Some(source_sha256),
                None,
                None,
                "source path is not UTF-8; compiler evidence is unavailable",
                operator_instructions,
            );
        }
    };
    let rustc_path = resolve_program_path(&options.rustc);
    let rustdoc_path = resolve_program_path(&options.rustdoc);
    let rustc_version = match capture_tool_version(&sandbox, &rustc_path) {
        Ok(version) => version,
        Err(reason) => {
            return compiler_evidence_unavailable(
                source_path,
                Some(source_sha256),
                None,
                None,
                &format!("cannot capture full rustc identity: {reason}"),
                operator_instructions,
            );
        }
    };
    let rustdoc_version = match capture_tool_version(&sandbox, &rustdoc_path) {
        Ok(version) => version,
        Err(reason) => {
            return compiler_evidence_unavailable(
                source_path,
                Some(source_sha256),
                None,
                None,
                &format!("cannot capture full rustdoc identity: {reason}"),
                operator_instructions,
            );
        }
    };
    let host = match toolchain_host(&rustc_version) {
        Some(host) => host,
        None => {
            return compiler_evidence_unavailable(
                source_path,
                Some(source_sha256),
                None,
                None,
                "rustc verbose version did not report a host triple",
                operator_instructions,
            );
        }
    };
    let target = options.target.clone().unwrap_or_else(|| host.clone());
    let sysroot = match capture_tool_path(&sandbox, &rustc_path, &["--print", "sysroot"]) {
        Ok(path) => path,
        Err(reason) => {
            return compiler_evidence_unavailable(
                source_path,
                Some(source_sha256),
                None,
                None,
                &format!("cannot capture rustc sysroot: {reason}"),
                operator_instructions,
            );
        }
    };
    let target_libdir = match capture_tool_path(
        &sandbox,
        &rustc_path,
        &["--print", "target-libdir", "--target", &target],
    ) {
        Ok(path) => path,
        Err(reason) => {
            return compiler_evidence_unavailable(
                source_path,
                Some(source_sha256),
                None,
                None,
                &format!("cannot capture rustc target library directory: {reason}"),
                operator_instructions,
            );
        }
    };
    let toolchain_sha256 = hash_lines(&[
        format!("rustc_path\0{}", rustc_path.display()),
        format!("rustc_version\0{rustc_version}"),
        format!("rustdoc_path\0{}", rustdoc_path.display()),
        format!("rustdoc_version\0{rustdoc_version}"),
        format!("sysroot\0{}", sysroot.display()),
        format!("target_libdir\0{}", target_libdir.display()),
        format!("host\0{host}"),
    ]);
    let toolchain = CompilerToolchainProvenance {
        rustc_path,
        rustc_version,
        rustdoc_path,
        rustdoc_version,
        sysroot,
        target_libdir,
        host,
        toolchain_sha256,
    };
    let crate_name = options
        .crate_name
        .clone()
        .unwrap_or_else(|| compiler_crate_name(&source_path));
    if !valid_crate_name(&crate_name) {
        return compiler_evidence_unavailable(
            source_path,
            Some(source_sha256),
            Some(toolchain),
            None,
            "compiler evidence crate name must contain only ASCII alphanumeric characters or underscores",
            operator_instructions,
        );
    }
    let mut cfgs = options.cfgs.clone();
    cfgs.sort();
    cfgs.dedup();
    let mut features = options.features.clone();
    features.sort();
    features.dedup();
    let mut library_search_paths = options
        .library_search_paths
        .iter()
        .map(|path| fs::canonicalize(path).unwrap_or_else(|_| path.clone()))
        .collect::<Vec<_>>();
    library_search_paths.sort();
    library_search_paths.dedup();
    let externs = match capture_extern_provenance(&options.externs) {
        Ok(externs) => externs,
        Err(reason) => {
            return compiler_evidence_unavailable(
                source_path,
                Some(source_sha256),
                Some(toolchain),
                None,
                &reason,
                operator_instructions,
            );
        }
    };
    let compiler_cfg =
        match capture_compiler_cfg(&sandbox, &toolchain.rustc_path, &target, &cfgs, &features) {
            Ok(cfg) => cfg,
            Err(reason) => {
                return compiler_evidence_unavailable(
                    source_path,
                    Some(source_sha256),
                    Some(toolchain),
                    None,
                    &format!("cannot capture exact rustc cfg context: {reason}"),
                    operator_instructions,
                );
            }
        };
    let environment = controlled_compiler_environment();
    let context_sha256 = compiler_context_hash(
        &canonical_source_path,
        &source_sha256,
        &crate_name,
        &options.edition,
        &target,
        &cfgs,
        &features,
        &compiler_cfg,
        &externs,
        &library_search_paths,
        &environment,
    );
    let context = CompilerContextProvenance {
        source_path: canonical_source_path,
        source_sha256: source_sha256.clone(),
        crate_name,
        crate_type: "lib".to_string(),
        edition: options.edition,
        target,
        cfgs,
        features,
        compiler_cfg,
        externs,
        library_search_paths,
        environment,
        context_sha256,
    };

    let macro_expansion = observe_text_artifact(
        &sandbox,
        CompilerEvidenceArtifactKind::MacroExpansion,
        &toolchain.rustc_path,
        rustc_unpretty_args(&context, &source_arg, "expanded,identified"),
    );
    let macro_resolution = observe_binary_artifact(
        &sandbox,
        CompilerEvidenceArtifactKind::MacroResolution,
        &toolchain.rustc_path,
        rustc_metadata_args(&context, &source_arg, scratch.path.join("metadata.rmeta")),
        scratch.path.join("metadata.rmeta"),
    );
    let macro_hygiene = observe_text_artifact(
        &sandbox,
        CompilerEvidenceArtifactKind::MacroHygiene,
        &toolchain.rustc_path,
        rustc_unpretty_args(&context, &source_arg, "expanded,hygiene"),
    );
    let hir = observe_text_artifact(
        &sandbox,
        CompilerEvidenceArtifactKind::Hir,
        &toolchain.rustc_path,
        rustc_unpretty_args(&context, &source_arg, "hir"),
    );
    let mir = observe_text_artifact(
        &sandbox,
        CompilerEvidenceArtifactKind::Mir,
        &toolchain.rustc_path,
        rustc_unpretty_args(&context, &source_arg, "mir"),
    );
    let (rustdoc_public_api, rustdoc_public_api_inputs) = observe_rustdoc_public_api_artifact(
        &sandbox,
        &toolchain.rustdoc_path,
        &context,
        &source_arg,
        scratch.path.join("rustdoc"),
    );

    let mut artifacts = vec![
        macro_expansion,
        macro_resolution,
        macro_hygiene,
        hir,
        mir,
        rustdoc_public_api,
    ];
    finalize_artifact_pins(&mut artifacts, &context, &toolchain, &source_arg);
    let all_observed = artifacts
        .iter()
        .all(|artifact| artifact.status == CompilerArtifactStatus::CompilerObserved);
    let mut gaps = artifacts
        .iter()
        .filter(|artifact| artifact.status == CompilerArtifactStatus::EvidenceUnavailable)
        .map(|artifact| CompilerEvidenceGap {
            artifact: Some(artifact.kind),
            reason: artifact.diagnostic.clone(),
        })
        .collect::<Vec<_>>();

    if !all_observed {
        return CompilerEvidenceReport {
            collection_status: CompilerEvidenceCollectionStatus::EvidenceUnavailable,
            source_path,
            source_sha256: Some(source_sha256),
            toolchain: Some(toolchain),
            context: Some(context),
            artifacts,
            semantic_hash: None,
            public_api_hash: None,
            semantic_inputs: Vec::new(),
            public_api_inputs: Vec::new(),
            gaps,
            operator_instructions,
        };
    }

    let mut semantic_inputs = vec![
        format!("rustc\0{}", toolchain.rustc_version),
        format!("edition\0{}", context.edition),
        format!("target\0{}", context.target),
        format!("context_sha256\0{}", context.context_sha256),
        format!("toolchain_sha256\0{}", toolchain.toolchain_sha256),
    ];
    for kind in [
        CompilerEvidenceArtifactKind::Hir,
        CompilerEvidenceArtifactKind::Mir,
    ] {
        let artifact = artifacts
            .iter()
            .find(|artifact| artifact.kind == kind)
            .expect("all required compiler artifacts are present");
        let output = artifact
            .output
            .as_deref()
            .expect("observed HIR/MIR artifacts retain UTF-8 output");
        semantic_inputs.push(format!(
            "{}\0{}",
            kind.as_str(),
            normalize_compiler_output(output, &source_arg)
        ));
    }
    semantic_inputs.sort();
    let Some((_artifact_public_api_hash, mut public_api_inputs)) = rustdoc_public_api_inputs else {
        gaps.push(CompilerEvidenceGap {
            artifact: Some(CompilerEvidenceArtifactKind::RustdocPublicApi),
            reason: "rustdoc JSON did not yield a public API snapshot".to_string(),
        });
        return CompilerEvidenceReport {
            collection_status: CompilerEvidenceCollectionStatus::EvidenceUnavailable,
            source_path,
            source_sha256: Some(source_sha256),
            toolchain: Some(toolchain),
            context: Some(context),
            artifacts,
            semantic_hash: None,
            public_api_hash: None,
            semantic_inputs: Vec::new(),
            public_api_inputs: Vec::new(),
            gaps,
            operator_instructions,
        };
    };
    public_api_inputs.push(format!("rustdoc\0{}", toolchain.rustdoc_version));
    public_api_inputs.push(format!("edition\0{}", context.edition));
    public_api_inputs.push(format!("target\0{}", context.target));
    public_api_inputs.push(format!("toolchain_sha256\0{}", toolchain.toolchain_sha256));
    public_api_inputs.sort();
    let public_api_hash = hash_lines(&public_api_inputs);

    CompilerEvidenceReport {
        collection_status: CompilerEvidenceCollectionStatus::CompilerObserved,
        source_path,
        source_sha256: Some(source_sha256),
        toolchain: Some(toolchain),
        context: Some(context),
        artifacts,
        semantic_hash: Some(hash_lines(&semantic_inputs)),
        public_api_hash: Some(public_api_hash),
        semantic_inputs,
        public_api_inputs,
        gaps,
        operator_instructions,
    }
}

fn compiler_evidence_unavailable(
    source_path: PathBuf,
    source_sha256: Option<String>,
    toolchain: Option<CompilerToolchainProvenance>,
    context: Option<CompilerContextProvenance>,
    reason: &str,
    operator_instructions: Vec<String>,
) -> CompilerEvidenceReport {
    CompilerEvidenceReport {
        collection_status: CompilerEvidenceCollectionStatus::EvidenceUnavailable,
        source_path,
        source_sha256,
        toolchain,
        context,
        artifacts: Vec::new(),
        semantic_hash: None,
        public_api_hash: None,
        semantic_inputs: Vec::new(),
        public_api_inputs: Vec::new(),
        gaps: vec![CompilerEvidenceGap {
            artifact: None,
            reason: reason.to_string(),
        }],
        operator_instructions,
    }
}

fn compiler_execution_request_digest(
    source_path: &Path,
    options: &CompilerEvidenceOptions,
) -> Result<String, String> {
    let source_path = fs::canonicalize(source_path)
        .map_err(|error| format!("cannot resolve source path: {error}"))?;
    let source = fs::read(&source_path)
        .map_err(|error| format!("cannot read source for approval binding: {error}"))?;
    let rustc = resolve_program_path(&options.rustc);
    let rustdoc = resolve_program_path(&options.rustdoc);
    let sandbox = fs::canonicalize(&options.sandbox.executable)
        .unwrap_or_else(|_| options.sandbox.executable.clone());
    let mut inputs = vec![
        format!("enabled\0{}", options.enabled),
        format!("source_path\0{}", source_path.display()),
        format!("source_sha256\0{}", sha256_bytes(&source)),
        format!("rustc\0{}", rustc.display()),
        format!("rustdoc\0{}", rustdoc.display()),
        format!("edition\0{}", options.edition),
        format!(
            "crate_name\0{}",
            options.crate_name.as_deref().unwrap_or("<derived>")
        ),
        format!(
            "target\0{}",
            options.target.as_deref().unwrap_or("<compiler-host>")
        ),
        format!("sandbox\0{}", sandbox.display()),
    ];
    let mut cfgs = options.cfgs.clone();
    cfgs.sort();
    cfgs.dedup();
    inputs.extend(cfgs.into_iter().map(|cfg| format!("cfg\0{cfg}")));
    let mut features = options.features.clone();
    features.sort();
    features.dedup();
    inputs.extend(
        features
            .into_iter()
            .map(|feature| format!("feature\0{feature}")),
    );
    let mut externs = capture_extern_provenance(&options.externs)?;
    externs.sort_by(|left, right| left.name.cmp(&right.name));
    inputs.extend(externs.into_iter().map(|external| {
        format!(
            "extern\0{}\0{}\0{}\0{}\0{}",
            external.name,
            external.kind.as_str(),
            external.path.display(),
            external.artifact_sha256,
            external.artifact_bytes
        )
    }));
    let mut library_paths = options
        .library_search_paths
        .iter()
        .map(|path| fs::canonicalize(path).unwrap_or_else(|_| path.clone()))
        .collect::<Vec<_>>();
    library_paths.sort();
    library_paths.dedup();
    inputs.extend(
        library_paths
            .into_iter()
            .map(|path| format!("library_search_path\0{}", path.display())),
    );
    inputs.sort();
    Ok(hash_lines(&inputs))
}

fn validate_compiler_execution_capability(
    authority: &CompilerExecutionApprovalAuthority,
    capability: &CompilerExecutionCapability,
    request_digest: &str,
) -> Result<(), String> {
    if capability.used.swap(true, Ordering::AcqRel) {
        return Err("compiler execution capability has already been used".to_string());
    }
    if capability.authority_id != authority.authority_id {
        return Err("compiler execution capability came from the wrong authority".to_string());
    }
    if capability.request_digest != request_digest {
        return Err(
            "compiler execution capability does not match the exact compiler request".to_string(),
        );
    }
    let expected =
        compiler_capability_authenticator(&authority.secret, request_digest, &capability.nonce);
    if capability.authenticator != expected {
        return Err("compiler execution capability authenticator is invalid".to_string());
    }
    Ok(())
}

fn compiler_capability_authenticator(
    secret: &[u8; 32],
    request_digest: &str,
    nonce: &[u8; 32],
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"codedb-rust-static-compiler-capability-v1");
    hasher.update(secret);
    hasher.update(request_digest.as_bytes());
    hasher.update(nonce);
    hasher.finalize().into()
}

fn kernel_random_32() -> Result<[u8; 32], String> {
    let mut random = [0u8; 32];
    fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(|error| format!("kernel randomness unavailable: {error}"))?;
    Ok(random)
}

fn default_bwrap_path() -> PathBuf {
    let mut candidates = vec![
        "/home/flexnetos/.nix-profile/bin/bwrap",
        "/run/current-system/sw/bin/bwrap",
        "/usr/bin/bwrap",
        "/bin/bwrap",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect::<Vec<_>>();
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|directory| directory.join("bwrap")));
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("/usr/bin/bwrap"))
}

struct CompilerSandboxPlan {
    executable: PathBuf,
    arguments: Vec<String>,
    request_digest: String,
}

impl CompilerSandboxPlan {
    #[cfg(target_os = "linux")]
    fn new(
        options: &CompilerEvidenceOptions,
        source_path: &Path,
        scratch_path: &Path,
        request_digest: &str,
    ) -> Result<Self, String> {
        let executable = trusted_bwrap_executable(&options.sandbox.executable)?;
        let source_path = fs::canonicalize(source_path)
            .map_err(|error| format!("cannot resolve sandbox source: {error}"))?;
        let rustc = resolve_program_path(&options.rustc);
        let rustdoc = resolve_program_path(&options.rustdoc);
        let mut read_only_mounts = vec![source_path, rustc, rustdoc];
        for external in &options.externs {
            read_only_mounts.push(fs::canonicalize(&external.path).map_err(|error| {
                format!(
                    "cannot resolve sandbox extern {}: {error}",
                    external.path.display()
                )
            })?);
        }
        for path in &options.library_search_paths {
            read_only_mounts.push(fs::canonicalize(path).map_err(|error| {
                format!(
                    "cannot resolve sandbox library path {}: {error}",
                    path.display()
                )
            })?);
        }
        read_only_mounts.sort();
        read_only_mounts.dedup();

        let mut parent_directories = BTreeSet::new();
        for path in read_only_mounts
            .iter()
            .chain(std::iter::once(&scratch_path.to_path_buf()))
        {
            let mut parent = path.parent();
            while let Some(directory) = parent {
                if directory != Path::new("/") {
                    parent_directories.insert(directory.to_path_buf());
                }
                parent = directory.parent();
            }
        }

        let mut arguments = vec![
            "--die-with-parent".to_string(),
            "--new-session".to_string(),
            "--unshare-all".to_string(),
            "--cap-drop".to_string(),
            "ALL".to_string(),
            "--clearenv".to_string(),
            "--proc".to_string(),
            "/proc".to_string(),
            "--dev".to_string(),
            "/dev".to_string(),
            "--tmpfs".to_string(),
            "/tmp".to_string(),
            "--dir".to_string(),
            "/homeless".to_string(),
        ];
        for directory in parent_directories {
            arguments.push("--dir".to_string());
            arguments.push(directory.to_string_lossy().into_owned());
        }
        for path in ["/nix/store", "/usr", "/bin", "/lib", "/lib64"] {
            if Path::new(path).exists() {
                arguments.extend(["--ro-bind".to_string(), path.to_string(), path.to_string()]);
            }
        }
        for path in read_only_mounts {
            let path = path.to_string_lossy().into_owned();
            arguments.extend(["--ro-bind".to_string(), path.clone(), path]);
        }
        let scratch = scratch_path.to_string_lossy().into_owned();
        arguments.extend(["--bind".to_string(), scratch.clone(), scratch.clone()]);
        arguments.extend([
            "--setenv".to_string(),
            "HOME".to_string(),
            "/homeless".to_string(),
            "--setenv".to_string(),
            "TMPDIR".to_string(),
            "/tmp".to_string(),
            "--setenv".to_string(),
            "LANG".to_string(),
            "C".to_string(),
            "--setenv".to_string(),
            "LC_ALL".to_string(),
            "C".to_string(),
            "--setenv".to_string(),
            "SOURCE_DATE_EPOCH".to_string(),
            "1".to_string(),
            "--chdir".to_string(),
            "/tmp".to_string(),
        ]);
        Ok(Self {
            executable,
            arguments,
            request_digest: request_digest.to_string(),
        })
    }

    #[cfg(not(target_os = "linux"))]
    fn new(
        _options: &CompilerEvidenceOptions,
        _source_path: &Path,
        _scratch_path: &Path,
        _request_digest: &str,
    ) -> Result<Self, String> {
        Err("compiler execution is supported only on Linux".to_string())
    }
}

#[cfg(target_os = "linux")]
fn trusted_bwrap_executable(configured: &Path) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(configured)
        .map_err(|error| format!("cannot resolve {}: {error}", configured.display()))?;
    let trusted = canonical == Path::new("/usr/bin/bwrap")
        || canonical == Path::new("/bin/bwrap")
        || (canonical.starts_with("/nix/store")
            && canonical.file_name().and_then(|name| name.to_str()) == Some("bwrap"));
    if !trusted || !canonical.is_file() {
        return Err(format!(
            "{} is not a trusted bubblewrap executable",
            canonical.display()
        ));
    }
    Ok(canonical)
}

struct CompilerScratch {
    path: PathBuf,
}

impl CompilerScratch {
    fn new() -> Result<Self, io::Error> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "codedb_compiler_evidence_{}_{}",
            std::process::id(),
            nonce
        ));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }
}

impl Drop for CompilerScratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn resolve_program_path(program: &Path) -> PathBuf {
    if program.components().count() > 1 {
        return fs::canonicalize(program).unwrap_or_else(|_| program.to_path_buf());
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(program);
            if candidate.is_file() {
                return fs::canonicalize(&candidate).unwrap_or(candidate);
            }
        }
    }
    program.to_path_buf()
}

fn capture_tool_version(sandbox: &CompilerSandboxPlan, program: &Path) -> Result<String, String> {
    let args = vec!["--version".to_string(), "--verbose".to_string()];
    let output = run_tool(sandbox, program, &args)?;
    let version = String::from_utf8(output.stdout)
        .map_err(|_| "tool version output is not UTF-8".to_string())?;
    if version.trim().is_empty() {
        return Err("tool version output is empty".to_string());
    }
    Ok(version)
}

fn compiler_operator_instructions() -> Vec<String> {
    vec![
        "Compiler execution requires a trusted frontdoor to mint one opaque request-bound capability; enabled=true is never authorization."
            .to_string(),
        "Every rustc/rustdoc/proc-macro invocation is mandatory Linux bubblewrap execution with unshared network, hidden HOME, read-only source/extern mounts, and one isolated writable scratch root."
            .to_string(),
        "Use one matching nightly rustc/rustdoc pair that supports rustc -Zunpretty and rustdoc JSON."
            .to_string(),
        "RUSTC=/absolute/path/to/nightly-rustc RUSTDOC=/absolute/path/to/matching-nightly-rustdoc cargo test -p codedb-rust-static --lib compiler_observed_tests::observes_real_proc_macro_and_pins_every_compiler_artifact -- --nocapture"
            .to_string(),
        "The positive fixture compiles a real proc-macro artifact, hashes it, and supplies it through --extern; do not substitute static syntax inventory."
            .to_string(),
        "Compiler and rustdoc evidence runs with only LANG=C, LC_ALL=C, and SOURCE_DATE_EPOCH=1; undeclared ambient environment is intentionally unavailable."
            .to_string(),
    ]
}

fn toolchain_host(verbose_version: &str) -> Option<String> {
    verbose_version
        .lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_string))
}

fn capture_tool_path(
    sandbox: &CompilerSandboxPlan,
    program: &Path,
    args: &[&str],
) -> Result<PathBuf, String> {
    let args = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    let output = run_tool(sandbox, program, &args)?;
    let value = String::from_utf8(output.stdout)
        .map_err(|_| "tool path output is not UTF-8".to_string())?;
    let value = value.trim();
    if value.is_empty() {
        return Err("tool path output is empty".to_string());
    }
    let path = PathBuf::from(value);
    Ok(fs::canonicalize(&path).unwrap_or(path))
}

fn capture_tool_text(
    sandbox: &CompilerSandboxPlan,
    program: &Path,
    args: &[String],
) -> Result<String, String> {
    let output = run_tool(sandbox, program, args)?;
    let text = String::from_utf8(output.stdout)
        .map_err(|_| "tool output is not valid UTF-8".to_string())?;
    if text.trim().is_empty() {
        return Err("tool output is empty".to_string());
    }
    Ok(text)
}

fn valid_crate_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn capture_extern_provenance(
    configured_externs: &[CompilerExtern],
) -> Result<Vec<CompilerExternProvenance>, String> {
    let mut externs = Vec::with_capacity(configured_externs.len());
    for configured in configured_externs {
        if !valid_crate_name(&configured.name) {
            return Err(format!(
                "compiler extern name {:?} is not a valid crate name",
                configured.name
            ));
        }
        let path = fs::canonicalize(&configured.path).map_err(|error| {
            format!(
                "cannot resolve compiler extern {} at {}: {error}",
                configured.name,
                configured.path.display()
            )
        })?;
        if path.to_str().is_none() {
            return Err(format!(
                "compiler extern path is not UTF-8: {}",
                path.display()
            ));
        }
        let artifact = fs::read(&path).map_err(|error| {
            format!(
                "cannot read compiler extern {} at {}: {error}",
                configured.name,
                path.display()
            )
        })?;
        if artifact.is_empty() {
            return Err(format!(
                "compiler extern {} at {} is empty",
                configured.name,
                path.display()
            ));
        }
        externs.push(CompilerExternProvenance {
            name: configured.name.clone(),
            path,
            kind: configured.kind,
            artifact_sha256: sha256_bytes(&artifact),
            artifact_bytes: artifact.len(),
        });
    }
    externs.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.path.cmp(&right.path))
    });
    if externs.windows(2).any(|pair| pair[0].name == pair[1].name) {
        return Err("compiler extern names must be unique".to_string());
    }
    Ok(externs)
}

fn capture_compiler_cfg(
    sandbox: &CompilerSandboxPlan,
    rustc: &Path,
    target: &str,
    cfgs: &[String],
    features: &[String],
) -> Result<Vec<String>, String> {
    let mut args = vec![
        "--print".to_string(),
        "cfg".to_string(),
        "--target".to_string(),
        target.to_string(),
    ];
    push_cfg_args(&mut args, cfgs, features);
    let output = capture_tool_text(sandbox, rustc, &args)?;
    let mut rows = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    rows.sort();
    rows.dedup();
    Ok(rows)
}

#[allow(clippy::too_many_arguments)]
fn compiler_context_hash(
    source_path: &Path,
    source_sha256: &str,
    crate_name: &str,
    edition: &str,
    target: &str,
    cfgs: &[String],
    features: &[String],
    compiler_cfg: &[String],
    externs: &[CompilerExternProvenance],
    library_search_paths: &[PathBuf],
    environment: &BTreeMap<String, String>,
) -> String {
    let mut inputs = vec![
        format!("source_path\0{}", source_path.display()),
        format!("source_sha256\0{source_sha256}"),
        format!("crate_name\0{crate_name}"),
        "crate_type\0lib".to_string(),
        format!("edition\0{edition}"),
        format!("target\0{target}"),
    ];
    inputs.extend(cfgs.iter().map(|cfg| format!("cfg\0{cfg}")));
    inputs.extend(features.iter().map(|feature| format!("feature\0{feature}")));
    inputs.extend(
        compiler_cfg
            .iter()
            .map(|cfg| format!("compiler_cfg\0{cfg}")),
    );
    inputs.extend(externs.iter().map(|external| {
        format!(
            "extern\0{}\0{}\0{}\0{}\0{}",
            external.name,
            external.kind.as_str(),
            external.path.display(),
            external.artifact_sha256,
            external.artifact_bytes
        )
    }));
    inputs.extend(
        library_search_paths
            .iter()
            .map(|path| format!("library_search_path\0{}", path.display())),
    );
    inputs.extend(
        environment
            .iter()
            .map(|(name, value)| format!("environment\0{name}\0{value}")),
    );
    hash_lines(&inputs)
}

fn controlled_compiler_environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("HOME".to_string(), "/homeless".to_string()),
        ("LANG".to_string(), "C".to_string()),
        ("LC_ALL".to_string(), "C".to_string()),
        ("SOURCE_DATE_EPOCH".to_string(), "1".to_string()),
        ("TMPDIR".to_string(), "/tmp".to_string()),
    ])
}

fn compiler_crate_name(source_path: &Path) -> String {
    let stem = source_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("source");
    let safe_stem = stem
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("codedb_compiler_{safe_stem}")
}

fn rustc_base_args(context: &CompilerContextProvenance) -> Vec<String> {
    let mut args = vec![
        "--crate-name".to_string(),
        context.crate_name.clone(),
        "--crate-type".to_string(),
        context.crate_type.clone(),
        "--edition".to_string(),
        context.edition.clone(),
        "--target".to_string(),
        context.target.clone(),
        "-C".to_string(),
        format!("metadata={}", context.context_sha256),
    ];
    push_cfg_args(&mut args, &context.cfgs, &context.features);
    for path in &context.library_search_paths {
        args.push("-L".to_string());
        args.push(format!("dependency={}", path.display()));
    }
    for external in &context.externs {
        args.push("--extern".to_string());
        args.push(format!("{}={}", external.name, external.path.display()));
    }
    args
}

fn push_cfg_args(args: &mut Vec<String>, cfgs: &[String], features: &[String]) {
    for cfg in cfgs {
        args.push("--cfg".to_string());
        args.push(cfg.clone());
    }
    for feature in features {
        args.push("--cfg".to_string());
        args.push(format!("feature=\"{feature}\""));
    }
}

fn rustc_unpretty_args(
    context: &CompilerContextProvenance,
    source_path: &str,
    mode: &str,
) -> Vec<String> {
    let mut args = rustc_base_args(context);
    args.push(format!("-Zunpretty={mode}"));
    args.push(source_path.to_string());
    args
}

fn rustc_metadata_args(
    context: &CompilerContextProvenance,
    source_path: &str,
    metadata_path: PathBuf,
) -> Vec<String> {
    let mut args = rustc_base_args(context);
    args.push("--emit=metadata".to_string());
    args.push("-o".to_string());
    args.push(metadata_path.to_string_lossy().into_owned());
    args.push(source_path.to_string());
    args
}

fn observe_text_artifact(
    sandbox: &CompilerSandboxPlan,
    kind: CompilerEvidenceArtifactKind,
    program: &Path,
    args: Vec<String>,
) -> CompilerArtifactEvidence {
    let command = command_vector(program, &args);
    let output = match run_tool(sandbox, program, &args) {
        Ok(output) => output,
        Err(reason) => return unavailable_artifact(kind, command, &reason),
    };
    let diagnostic = String::from_utf8_lossy(&output.stderr).into_owned();
    let output = match String::from_utf8(output.stdout) {
        Ok(output) if !output.is_empty() => output,
        Ok(_) => return unavailable_artifact(kind, command, "compiler emitted no evidence output"),
        Err(_) => {
            return unavailable_artifact(kind, command, "compiler evidence output is not UTF-8");
        }
    };
    CompilerArtifactEvidence {
        kind,
        status: CompilerArtifactStatus::CompilerObserved,
        command,
        evidence_sha256: Some(sha256_bytes(output.as_bytes())),
        evidence_bytes: Some(output.len()),
        output: Some(output),
        context_sha256: None,
        toolchain_sha256: None,
        pin_sha256: None,
        diagnostic,
    }
}

fn observe_binary_artifact(
    sandbox: &CompilerSandboxPlan,
    kind: CompilerEvidenceArtifactKind,
    program: &Path,
    args: Vec<String>,
    artifact_path: PathBuf,
) -> CompilerArtifactEvidence {
    let command = command_vector(program, &args);
    let output = match run_tool(sandbox, program, &args) {
        Ok(output) => output,
        Err(reason) => return unavailable_artifact(kind, command, &reason),
    };
    let diagnostic = String::from_utf8_lossy(&output.stderr).into_owned();
    let bytes = match fs::read(&artifact_path) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        Ok(_) => return unavailable_artifact(kind, command, "compiler metadata artifact is empty"),
        Err(error) => {
            return unavailable_artifact(
                kind,
                command,
                &format!("compiler metadata artifact is unavailable: {error}"),
            );
        }
    };
    CompilerArtifactEvidence {
        kind,
        status: CompilerArtifactStatus::CompilerObserved,
        command,
        output: None,
        evidence_sha256: Some(sha256_bytes(&bytes)),
        evidence_bytes: Some(bytes.len()),
        context_sha256: None,
        toolchain_sha256: None,
        pin_sha256: None,
        diagnostic,
    }
}

fn observe_rustdoc_public_api_artifact(
    sandbox: &CompilerSandboxPlan,
    program: &Path,
    context: &CompilerContextProvenance,
    source_path: &str,
    output_directory: PathBuf,
) -> (CompilerArtifactEvidence, Option<(String, Vec<String>)>) {
    let kind = CompilerEvidenceArtifactKind::RustdocPublicApi;
    if let Err(error) = fs::create_dir_all(&output_directory) {
        return (
            unavailable_artifact(
                kind,
                command_vector(program, &[]),
                &format!("cannot create rustdoc output directory: {error}"),
            ),
            None,
        );
    }
    let mut args = rustc_base_args(context);
    args.extend([
        "-Z".to_string(),
        "unstable-options".to_string(),
        "--output-format".to_string(),
        "json".to_string(),
        "-o".to_string(),
        output_directory.to_string_lossy().into_owned(),
        source_path.to_string(),
    ]);
    let command = command_vector(program, &args);
    let output = match run_tool(sandbox, program, &args) {
        Ok(output) => output,
        Err(reason) => return (unavailable_artifact(kind, command, &reason), None),
    };
    let diagnostic = String::from_utf8_lossy(&output.stderr).into_owned();
    let json_path = output_directory.join(format!("{}.json", context.crate_name));
    let bytes = match fs::read(&json_path) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        Ok(_) => {
            return (
                unavailable_artifact(kind, command, "rustdoc JSON artifact is empty"),
                None,
            );
        }
        Err(error) => {
            return (
                unavailable_artifact(
                    kind,
                    command,
                    &format!("rustdoc JSON artifact is unavailable: {error}"),
                ),
                None,
            );
        }
    };
    let json = match String::from_utf8(bytes) {
        Ok(json) => normalize_compiler_output(&json, source_path),
        Err(_) => {
            return (
                unavailable_artifact(kind, command, "rustdoc JSON artifact is not UTF-8"),
                None,
            );
        }
    };
    let public_api = match rustdoc_public_api_snapshot(&json) {
        Ok(public_api) => public_api,
        Err(reason) => {
            let artifact = CompilerArtifactEvidence {
                kind,
                status: CompilerArtifactStatus::EvidenceUnavailable,
                command,
                evidence_sha256: Some(sha256_bytes(json.as_bytes())),
                evidence_bytes: Some(json.len()),
                output: Some(json),
                context_sha256: None,
                toolchain_sha256: None,
                pin_sha256: None,
                diagnostic: reason,
            };
            return (artifact, None);
        }
    };
    let artifact = CompilerArtifactEvidence {
        kind,
        status: CompilerArtifactStatus::CompilerObserved,
        command,
        evidence_sha256: Some(sha256_bytes(json.as_bytes())),
        evidence_bytes: Some(json.len()),
        output: Some(json),
        context_sha256: None,
        toolchain_sha256: None,
        pin_sha256: None,
        diagnostic,
    };
    (artifact, Some(public_api))
}

fn run_tool(
    sandbox: &CompilerSandboxPlan,
    program: &Path,
    args: &[String],
) -> Result<Output, String> {
    let mut command = Command::new(&sandbox.executable);
    command.env_clear();
    command.args(&sandbox.arguments);
    command.arg("--");
    command.arg(program);
    command.args(args);
    let output = command.output().map_err(|error| {
        format!(
            "failed to execute {} in mandatory sandbox for request {}: {error}",
            program.display(),
            sandbox.request_digest
        )
    })?;
    if output.status.success() {
        Ok(output)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "{} exited with {}: {}",
            program.display(),
            output.status,
            stderr.trim()
        ))
    }
}

fn command_vector(program: &Path, args: &[String]) -> Vec<String> {
    std::iter::once(program.to_string_lossy().into_owned())
        .chain(args.iter().cloned())
        .collect()
}

fn unavailable_artifact(
    kind: CompilerEvidenceArtifactKind,
    command: Vec<String>,
    reason: &str,
) -> CompilerArtifactEvidence {
    CompilerArtifactEvidence {
        kind,
        status: CompilerArtifactStatus::EvidenceUnavailable,
        command,
        output: None,
        evidence_sha256: None,
        evidence_bytes: None,
        context_sha256: None,
        toolchain_sha256: None,
        pin_sha256: None,
        diagnostic: reason.to_string(),
    }
}

fn finalize_artifact_pins(
    artifacts: &mut [CompilerArtifactEvidence],
    context: &CompilerContextProvenance,
    toolchain: &CompilerToolchainProvenance,
    source_path: &str,
) {
    for artifact in artifacts {
        if artifact.status != CompilerArtifactStatus::CompilerObserved {
            continue;
        }
        if let Some(output) = &mut artifact.output {
            *output = normalize_compiler_output(output, source_path);
            artifact.evidence_sha256 = Some(sha256_bytes(output.as_bytes()));
            artifact.evidence_bytes = Some(output.len());
        }
        let (Some(evidence_sha256), Some(evidence_bytes)) =
            (artifact.evidence_sha256.as_deref(), artifact.evidence_bytes)
        else {
            artifact.status = CompilerArtifactStatus::EvidenceUnavailable;
            artifact.diagnostic =
                "observed artifact lacked digest or byte count; refusing pin".to_string();
            continue;
        };
        artifact.context_sha256 = Some(context.context_sha256.clone());
        artifact.toolchain_sha256 = Some(toolchain.toolchain_sha256.clone());
        artifact.pin_sha256 = Some(hash_lines(&[
            format!("kind\0{}", artifact.kind.as_str()),
            format!("evidence_sha256\0{evidence_sha256}"),
            format!("evidence_bytes\0{evidence_bytes}"),
            format!("context_sha256\0{}", context.context_sha256),
            format!("toolchain_sha256\0{}", toolchain.toolchain_sha256),
        ]));
    }
}

fn rustdoc_public_api_snapshot(json: &str) -> Result<(String, Vec<String>), String> {
    let compact = json
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    if !compact.starts_with('{')
        || !compact.contains("\"index\":")
        || !compact.contains("\"includes_private\":false")
    {
        return Err("rustdoc output is not a public-only JSON artifact with an index".to_string());
    }

    // rustdoc itself selected the public-only item graph (`includes_private:false`).
    // Hash the exact emitted artifact rather than projecting it back into a static
    // syntax claim or depending on a second JSON-semantic implementation.
    let public_api_hash = sha256_bytes(json.as_bytes());
    Ok((
        public_api_hash.clone(),
        vec![format!("rustdoc_public_json_sha256\0{public_api_hash}")],
    ))
}

fn normalize_compiler_output(output: &str, source_path: &str) -> String {
    output.replace(source_path, "<source>")
}

fn collect_items(
    items: &[Item],
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    rows: &mut Vec<RustItemRow>,
) {
    let mut anonymous_impl_index = 0usize;
    for item in items {
        match item {
            Item::Mod(item_mod) => {
                let name = item_mod.ident.to_string();
                push_row(
                    rows,
                    context_id,
                    relative_path,
                    module_path,
                    RustItemKind::Module,
                    &name,
                    classify_visibility(&item_mod.vis),
                );
                if let Some((_, nested_items)) = &item_mod.content {
                    let nested_module_path = join_module_path(module_path, &name);
                    collect_items(
                        nested_items,
                        context_id,
                        relative_path,
                        &nested_module_path,
                        rows,
                    );
                }
            }
            Item::Fn(item_fn) => push_row(
                rows,
                context_id,
                relative_path,
                module_path,
                RustItemKind::Function,
                &item_fn.sig.ident.to_string(),
                classify_visibility(&item_fn.vis),
            ),
            Item::Struct(item_struct) => push_row(
                rows,
                context_id,
                relative_path,
                module_path,
                RustItemKind::Struct,
                &item_struct.ident.to_string(),
                classify_visibility(&item_struct.vis),
            ),
            Item::Enum(item_enum) => push_row(
                rows,
                context_id,
                relative_path,
                module_path,
                RustItemKind::Enum,
                &item_enum.ident.to_string(),
                classify_visibility(&item_enum.vis),
            ),
            Item::Trait(item_trait) => push_row(
                rows,
                context_id,
                relative_path,
                module_path,
                RustItemKind::Trait,
                &item_trait.ident.to_string(),
                classify_visibility(&item_trait.vis),
            ),
            Item::Type(item_type) => push_row(
                rows,
                context_id,
                relative_path,
                module_path,
                RustItemKind::TypeAlias,
                &item_type.ident.to_string(),
                classify_visibility(&item_type.vis),
            ),
            Item::Const(item_const) => push_row(
                rows,
                context_id,
                relative_path,
                module_path,
                RustItemKind::Const,
                &item_const.ident.to_string(),
                classify_visibility(&item_const.vis),
            ),
            Item::Static(item_static) => push_row(
                rows,
                context_id,
                relative_path,
                module_path,
                RustItemKind::Static,
                &item_static.ident.to_string(),
                classify_visibility(&item_static.vis),
            ),
            Item::Impl(_) => {
                anonymous_impl_index += 1;
                let name = format!("impl#{anonymous_impl_index}");
                push_row_with_identity(
                    rows,
                    RustItemInput {
                        context_id,
                        relative_path,
                        module_path,
                        item_kind: RustItemKind::Impl,
                        name: &name,
                        visibility: RustVisibility::Private,
                        identity_kind: RustIdentityKind::UnstableAnonymous,
                        identity_note: "anonymous impl identity is scan-order stable but source-drift sensitive",
                    },
                );
            }
            Item::Use(item_use) => push_row(
                rows,
                context_id,
                relative_path,
                module_path,
                RustItemKind::Use,
                "use",
                classify_visibility(&item_use.vis),
            ),
            _ => {}
        }
    }
}

fn collect_macros(
    items: &[Item],
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    inventory: &mut MacroInventory,
) {
    for item in items {
        match item {
            Item::Macro(item_macro) if item_macro.mac.path.is_ident("macro_rules") => {
                let name = item_macro
                    .ident
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "macro_rules".to_string());
                push_macro_definition(
                    inventory,
                    context_id,
                    relative_path,
                    module_path,
                    &name,
                    &item_macro.mac.tokens.to_string(),
                );
            }
            Item::Macro(item_macro) => push_macro_invocation(
                inventory,
                context_id,
                relative_path,
                module_path,
                &path_to_string(&item_macro.mac.path),
                MacroInvocationKind::Item,
                &item_macro.mac.tokens.to_string(),
            ),
            Item::Mod(item_mod) => {
                if let Some((_, nested_items)) = &item_mod.content {
                    let nested_module_path =
                        join_module_path(module_path, &item_mod.ident.to_string());
                    collect_macros(
                        nested_items,
                        context_id,
                        relative_path,
                        &nested_module_path,
                        inventory,
                    );
                }
            }
            Item::Fn(item_fn) => collect_block_macros(
                &item_fn.block,
                context_id,
                relative_path,
                module_path,
                inventory,
            ),
            _ => {}
        }
    }
}

fn collect_proc_macros(
    items: &[Item],
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    inventory: &mut ProcMacroInventory,
) {
    for item in items {
        collect_attribute_proc_invocations(
            item_attrs(item),
            context_id,
            relative_path,
            module_path,
            inventory,
        );
        match item {
            Item::Fn(item_fn) => {
                for export_kind in proc_macro_export_kinds(&item_fn.attrs) {
                    push_proc_macro_crate(
                        inventory,
                        context_id,
                        relative_path,
                        module_path,
                        &item_fn.sig.ident.to_string(),
                        export_kind,
                    );
                }
                collect_proc_block_macros(
                    &item_fn.block,
                    context_id,
                    relative_path,
                    module_path,
                    inventory,
                );
            }
            Item::Macro(item_macro) if !item_macro.mac.path.is_ident("macro_rules") => {
                push_proc_macro_invocation(
                    inventory,
                    context_id,
                    relative_path,
                    module_path,
                    &path_to_string(&item_macro.mac.path),
                    ProcMacroInvocationKind::FunctionLikeCandidate,
                    &item_macro.mac.tokens.to_string(),
                );
            }
            Item::Mod(item_mod) => {
                if let Some((_, nested_items)) = &item_mod.content {
                    let nested_module_path =
                        join_module_path(module_path, &item_mod.ident.to_string());
                    collect_proc_macros(
                        nested_items,
                        context_id,
                        relative_path,
                        &nested_module_path,
                        inventory,
                    );
                }
            }
            _ => {}
        }
    }
}

fn collect_build_script_instructions(
    items: &[Item],
    context_id: &str,
    relative_path: &str,
    inventory: &mut BuildScriptInventory,
) {
    for item in items {
        match item {
            Item::Fn(item_fn) => collect_build_script_block(
                &item_fn.block,
                context_id,
                relative_path,
                &item_fn.sig.ident.to_string(),
                inventory,
            ),
            Item::Mod(item_mod) => {
                if let Some((_, nested_items)) = &item_mod.content {
                    collect_build_script_instructions(
                        nested_items,
                        context_id,
                        relative_path,
                        inventory,
                    );
                }
            }
            _ => {}
        }
    }
}

fn collect_static_include_edges(
    items: &[Item],
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    inventory: &mut StaticIncludeInventory,
) {
    for item in items {
        collect_path_attribute_edges(
            item_attrs(item),
            context_id,
            relative_path,
            module_path,
            inventory,
        );
        match item {
            Item::Macro(item_macro) => maybe_push_include_macro_edge(
                inventory,
                context_id,
                relative_path,
                module_path,
                &path_to_string(&item_macro.mac.path),
                &item_macro.mac.tokens.to_string(),
            ),
            Item::Mod(item_mod) => {
                if let Some((_, nested_items)) = &item_mod.content {
                    let nested_module_path =
                        join_module_path(module_path, &item_mod.ident.to_string());
                    collect_static_include_edges(
                        nested_items,
                        context_id,
                        relative_path,
                        &nested_module_path,
                        inventory,
                    );
                }
            }
            Item::Fn(item_fn) => collect_static_include_block(
                &item_fn.block,
                context_id,
                relative_path,
                module_path,
                inventory,
            ),
            _ => {}
        }
    }
}

fn collect_static_include_block(
    block: &Block,
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    inventory: &mut StaticIncludeInventory,
) {
    for statement in &block.stmts {
        match statement {
            Stmt::Local(local) => {
                if let Some(init) = &local.init {
                    collect_static_include_expr(
                        &init.expr,
                        context_id,
                        relative_path,
                        module_path,
                        inventory,
                    );
                }
            }
            Stmt::Macro(statement_macro) => maybe_push_include_macro_edge(
                inventory,
                context_id,
                relative_path,
                module_path,
                &path_to_string(&statement_macro.mac.path),
                &statement_macro.mac.tokens.to_string(),
            ),
            Stmt::Expr(Expr::Macro(expr_macro), _) => maybe_push_include_macro_edge(
                inventory,
                context_id,
                relative_path,
                module_path,
                &path_to_string(&expr_macro.mac.path),
                &expr_macro.mac.tokens.to_string(),
            ),
            _ => {}
        }
    }
}

fn collect_static_include_expr(
    expr: &Expr,
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    inventory: &mut StaticIncludeInventory,
) {
    if let Expr::Macro(expr_macro) = expr {
        maybe_push_include_macro_edge(
            inventory,
            context_id,
            relative_path,
            module_path,
            &path_to_string(&expr_macro.mac.path),
            &expr_macro.mac.tokens.to_string(),
        );
    }
}

fn collect_path_attribute_edges(
    attrs: &[Attribute],
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    inventory: &mut StaticIncludeInventory,
) {
    for attr in attrs {
        if !attr.path().is_ident("path") {
            continue;
        }
        match &attr.meta {
            Meta::NameValue(name_value) => {
                if let Expr::Lit(expr_lit) = &name_value.value
                    && let Lit::Str(lit_str) = &expr_lit.lit
                {
                    push_include_edge(
                        inventory,
                        context_id,
                        relative_path,
                        module_path,
                        StaticIncludeEdgeKind::PathAttribute,
                        &lit_str.value(),
                    );
                    continue;
                }
                push_include_gap(
                    inventory,
                    context_id,
                    relative_path,
                    module_path,
                    StaticIncludeEdgeKind::PathAttribute,
                    "path attribute is not a string literal",
                );
            }
            _ => push_include_gap(
                inventory,
                context_id,
                relative_path,
                module_path,
                StaticIncludeEdgeKind::PathAttribute,
                "path attribute is not name-value syntax",
            ),
        }
    }
}

fn maybe_push_include_macro_edge(
    inventory: &mut StaticIncludeInventory,
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    macro_path: &str,
    tokens: &str,
) {
    let edge_kind = match macro_path {
        "include" => StaticIncludeEdgeKind::Include,
        "include_str" => StaticIncludeEdgeKind::IncludeStr,
        "include_bytes" => StaticIncludeEdgeKind::IncludeBytes,
        _ => return,
    };
    if let Some(target_path) = only_string_literal(tokens) {
        push_include_edge(
            inventory,
            context_id,
            relative_path,
            module_path,
            edge_kind,
            &target_path,
        );
    } else {
        push_include_gap(
            inventory,
            context_id,
            relative_path,
            module_path,
            edge_kind,
            "include macro target is not a string literal",
        );
    }
}

fn only_string_literal(tokens: &str) -> Option<String> {
    let trimmed = tokens.trim();
    if !trimmed.starts_with('"') {
        return None;
    }
    let value = first_string_literal(trimmed)?;
    let closing_index = value.len() + 2;
    if trimmed[closing_index..].trim().is_empty() {
        Some(value)
    } else {
        None
    }
}

fn push_include_edge(
    inventory: &mut StaticIncludeInventory,
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    edge_kind: StaticIncludeEdgeKind,
    target_path: &str,
) {
    inventory.edges.push(StaticIncludeEdgeRow {
        stable_id: stable_macro_id(
            context_id,
            relative_path,
            module_path,
            edge_kind.as_str(),
            target_path,
            "",
        ),
        context_id: context_id.to_string(),
        relative_path: relative_path.to_string(),
        module_path: module_path.to_string(),
        edge_kind,
        target_path: target_path.to_string(),
        confidence: StaticCaptureConfidence::SyntaxOnly,
    });
}

fn push_include_gap(
    inventory: &mut StaticIncludeInventory,
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    edge_kind: StaticIncludeEdgeKind,
    reason: &str,
) {
    inventory.gaps.push(StaticIncludeGap {
        context_id: context_id.to_string(),
        relative_path: relative_path.to_string(),
        module_path: module_path.to_string(),
        edge_kind,
        reason: reason.to_string(),
    });
}

fn collect_build_script_block(
    block: &Block,
    context_id: &str,
    relative_path: &str,
    function_name: &str,
    inventory: &mut BuildScriptInventory,
) {
    for statement in &block.stmts {
        match statement {
            Stmt::Macro(statement_macro) => maybe_push_build_instruction(
                inventory,
                context_id,
                relative_path,
                function_name,
                &path_to_string(&statement_macro.mac.path),
                &statement_macro.mac.tokens.to_string(),
            ),
            Stmt::Expr(Expr::Macro(expr_macro), _) => maybe_push_build_instruction(
                inventory,
                context_id,
                relative_path,
                function_name,
                &path_to_string(&expr_macro.mac.path),
                &expr_macro.mac.tokens.to_string(),
            ),
            _ => {}
        }
    }
}

fn maybe_push_build_instruction(
    inventory: &mut BuildScriptInventory,
    context_id: &str,
    relative_path: &str,
    function_name: &str,
    macro_path: &str,
    tokens: &str,
) {
    if !matches!(macro_path, "println" | "eprintln") {
        return;
    }
    let Some(raw_instruction) = first_string_literal(tokens) else {
        return;
    };
    let Some((directive, value)) = parse_cargo_instruction(&raw_instruction) else {
        return;
    };
    inventory.instructions.push(BuildScriptInstructionRow {
        stable_id: stable_macro_id(
            context_id,
            relative_path,
            "",
            "build_script_instruction",
            function_name,
            &raw_instruction,
        ),
        context_id: context_id.to_string(),
        relative_path: relative_path.to_string(),
        function_name: function_name.to_string(),
        macro_path: macro_path.to_string(),
        directive,
        value,
        raw_instruction,
        confidence: StaticCaptureConfidence::SyntaxOnly,
    });
}

fn push_build_script_gaps(
    inventory: &mut BuildScriptInventory,
    context_id: &str,
    relative_path: &str,
) {
    for missing_truth in [
        BuildScriptMissingTruth::Execution,
        BuildScriptMissingTruth::Environment,
        BuildScriptMissingTruth::Stdout,
        BuildScriptMissingTruth::Stderr,
        BuildScriptMissingTruth::OutDirArtifacts,
    ] {
        inventory.gaps.push(BuildScriptCaptureGap {
            context_id: context_id.to_string(),
            relative_path: relative_path.to_string(),
            missing_truth,
            reason: "static build.rs detection does not execute build scripts".to_string(),
        });
    }
}

fn first_string_literal(tokens: &str) -> Option<String> {
    let start = tokens.find('"')?;
    let mut escaped = false;
    for (offset, character) in tokens[start + 1..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == '"' {
            return Some(tokens[start + 1..start + 1 + offset].to_string());
        }
    }
    None
}

fn parse_cargo_instruction(raw_instruction: &str) -> Option<(String, String)> {
    let body = raw_instruction
        .strip_prefix("cargo::")
        .or_else(|| raw_instruction.strip_prefix("cargo:"))?;
    let (directive, value) = body.split_once('=').unwrap_or((body, ""));
    Some((directive.to_string(), value.to_string()))
}

fn push_native_library(
    inventory: &mut NativeLinkInventory,
    instruction: &BuildScriptInstructionRow,
) {
    let (library_kind, library) = instruction
        .value
        .split_once('=')
        .map(|(kind, name)| (Some(kind.to_string()), name.to_string()))
        .unwrap_or((None, instruction.value.clone()));
    inventory.libraries.push(NativeLibraryRow {
        stable_id: stable_macro_id(
            &instruction.context_id,
            &instruction.relative_path,
            "",
            "native_library",
            &library,
            library_kind.as_deref().unwrap_or(""),
        ),
        context_id: instruction.context_id.clone(),
        relative_path: instruction.relative_path.clone(),
        library,
        library_kind,
        confidence: StaticCaptureConfidence::SyntaxOnly,
    });
}

fn push_link_arg(inventory: &mut NativeLinkInventory, instruction: &BuildScriptInstructionRow) {
    inventory.link_args.push(LinkArgRow {
        stable_id: stable_macro_id(
            &instruction.context_id,
            &instruction.relative_path,
            "",
            "link_arg",
            &instruction.value,
            "",
        ),
        context_id: instruction.context_id.clone(),
        relative_path: instruction.relative_path.clone(),
        arg: instruction.value.clone(),
        confidence: StaticCaptureConfidence::SyntaxOnly,
    });
}

fn push_link_search_path(
    inventory: &mut NativeLinkInventory,
    instruction: &BuildScriptInstructionRow,
) {
    let (search_kind, path) = instruction
        .value
        .split_once('=')
        .map(|(kind, path)| (Some(kind.to_string()), path.to_string()))
        .unwrap_or((None, instruction.value.clone()));
    inventory.link_search_paths.push(LinkSearchPathRow {
        stable_id: stable_macro_id(
            &instruction.context_id,
            &instruction.relative_path,
            "",
            "link_search_path",
            &path,
            search_kind.as_deref().unwrap_or(""),
        ),
        context_id: instruction.context_id.clone(),
        relative_path: instruction.relative_path.clone(),
        path,
        search_kind,
        confidence: StaticCaptureConfidence::SyntaxOnly,
    });
}

fn push_native_link_gaps(
    inventory: &mut NativeLinkInventory,
    context_id: &str,
    relative_path: &str,
) {
    for missing_truth in [
        NativeLinkMissingTruth::LinkerTool,
        NativeLinkMissingTruth::LibraryAvailability,
        NativeLinkMissingTruth::LinkResult,
    ] {
        inventory.gaps.push(NativeLinkGap {
            context_id: context_id.to_string(),
            relative_path: relative_path.to_string(),
            missing_truth,
            reason: "static native/link capture does not execute build scripts or linkers"
                .to_string(),
        });
    }
}

fn collect_proc_block_macros(
    block: &Block,
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    inventory: &mut ProcMacroInventory,
) {
    for statement in &block.stmts {
        match statement {
            Stmt::Macro(statement_macro) => push_proc_macro_invocation(
                inventory,
                context_id,
                relative_path,
                module_path,
                &path_to_string(&statement_macro.mac.path),
                ProcMacroInvocationKind::FunctionLikeCandidate,
                &statement_macro.mac.tokens.to_string(),
            ),
            Stmt::Expr(Expr::Macro(expr_macro), _) => push_proc_macro_invocation(
                inventory,
                context_id,
                relative_path,
                module_path,
                &path_to_string(&expr_macro.mac.path),
                ProcMacroInvocationKind::FunctionLikeCandidate,
                &expr_macro.mac.tokens.to_string(),
            ),
            _ => {}
        }
    }
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

fn proc_macro_export_kinds(attrs: &[Attribute]) -> Vec<ProcMacroExportKind> {
    attrs
        .iter()
        .filter_map(|attr| {
            if attr.path().is_ident("proc_macro") {
                Some(ProcMacroExportKind::FunctionLike)
            } else if attr.path().is_ident("proc_macro_attribute") {
                Some(ProcMacroExportKind::Attribute)
            } else if attr.path().is_ident("proc_macro_derive") {
                Some(ProcMacroExportKind::Derive)
            } else {
                None
            }
        })
        .collect()
}

fn collect_attribute_proc_invocations(
    attrs: &[Attribute],
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    inventory: &mut ProcMacroInventory,
) {
    for attr in attrs {
        let path = path_to_string(attr.path());
        if attr.path().is_ident("derive") {
            for derive_name in derive_invocation_names(attr) {
                push_proc_macro_invocation(
                    inventory,
                    context_id,
                    relative_path,
                    module_path,
                    &derive_name,
                    ProcMacroInvocationKind::Derive,
                    &attr.meta.to_token_stream_string(),
                );
            }
        } else if is_builtin_non_proc_attribute(&path) {
            continue;
        } else {
            push_proc_macro_invocation(
                inventory,
                context_id,
                relative_path,
                module_path,
                &path,
                ProcMacroInvocationKind::Attribute,
                &attr.meta.to_token_stream_string(),
            );
        }
    }
}

fn is_builtin_non_proc_attribute(path: &str) -> bool {
    matches!(
        path,
        "allow"
            | "cfg"
            | "cfg_attr"
            | "derive"
            | "deny"
            | "doc"
            | "forbid"
            | "inline"
            | "must_use"
            | "proc_macro"
            | "proc_macro_attribute"
            | "proc_macro_derive"
            | "repr"
            | "test"
            | "warn"
    )
}

fn derive_invocation_names(attr: &Attribute) -> Vec<String> {
    match &attr.meta {
        Meta::List(list) => list
            .tokens
            .to_string()
            .split(',')
            .map(|part| part.trim().to_string())
            .filter(|part| !part.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn collect_block_macros(
    block: &Block,
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    inventory: &mut MacroInventory,
) {
    for statement in &block.stmts {
        match statement {
            Stmt::Macro(statement_macro) => push_macro_invocation(
                inventory,
                context_id,
                relative_path,
                module_path,
                &path_to_string(&statement_macro.mac.path),
                MacroInvocationKind::Statement,
                &statement_macro.mac.tokens.to_string(),
            ),
            Stmt::Expr(Expr::Macro(expr_macro), _) => push_macro_invocation(
                inventory,
                context_id,
                relative_path,
                module_path,
                &path_to_string(&expr_macro.mac.path),
                MacroInvocationKind::Expression,
                &expr_macro.mac.tokens.to_string(),
            ),
            _ => {}
        }
    }
}

fn push_macro_definition(
    inventory: &mut MacroInventory,
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    name: &str,
    tokens: &str,
) {
    inventory.definitions.push(MacroDefinitionRow {
        stable_id: stable_macro_id(
            context_id,
            relative_path,
            module_path,
            "definition",
            name,
            tokens,
        ),
        context_id: context_id.to_string(),
        relative_path: relative_path.to_string(),
        module_path: module_path.to_string(),
        name: name.to_string(),
        matcher_summary: summarize_macro_matcher(tokens),
        transcriber_summary: summarize_macro_transcriber(tokens),
        confidence: StaticCaptureConfidence::SyntaxOnly,
    });
    push_macro_gap(
        inventory,
        context_id,
        relative_path,
        module_path,
        name,
        MacroMissingTruth::Expansion,
    );
    push_macro_expansion_gate(inventory, context_id, relative_path, module_path, name);
    push_macro_gap(
        inventory,
        context_id,
        relative_path,
        module_path,
        name,
        MacroMissingTruth::Hygiene,
    );
}

fn push_macro_invocation(
    inventory: &mut MacroInventory,
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    macro_path: &str,
    invocation_kind: MacroInvocationKind,
    tokens: &str,
) {
    inventory.invocations.push(MacroInvocationRow {
        stable_id: stable_macro_id(
            context_id,
            relative_path,
            module_path,
            invocation_kind.as_str(),
            macro_path,
            tokens,
        ),
        context_id: context_id.to_string(),
        relative_path: relative_path.to_string(),
        module_path: module_path.to_string(),
        macro_path: macro_path.to_string(),
        invocation_kind,
        token_summary: summarize_tokens(tokens),
        confidence: StaticCaptureConfidence::SyntaxOnly,
    });
    push_macro_expansion_gate(
        inventory,
        context_id,
        relative_path,
        module_path,
        macro_path,
    );
}

fn push_macro_gap(
    inventory: &mut MacroInventory,
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    macro_name: &str,
    missing_truth: MacroMissingTruth,
) {
    inventory.gaps.push(MacroCaptureGap {
        context_id: context_id.to_string(),
        relative_path: relative_path.to_string(),
        module_path: module_path.to_string(),
        macro_name: macro_name.to_string(),
        missing_truth,
        reason: "static macro capture does not prove compiler expansion or hygiene".to_string(),
    });
}

fn push_macro_expansion_gate(
    inventory: &mut MacroInventory,
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    macro_name: &str,
) {
    let already_recorded = inventory.expansion_gates.iter().any(|gate| {
        gate.context_id == context_id
            && gate.relative_path == relative_path
            && gate.module_path == module_path
            && gate.macro_name == macro_name
            && gate.gate_status == MacroExpansionGateStatus::Gap
    });
    if already_recorded {
        return;
    }
    inventory.expansion_gates.push(MacroExpansionGateRow {
        context_id: context_id.to_string(),
        relative_path: relative_path.to_string(),
        module_path: module_path.to_string(),
        macro_name: macro_name.to_string(),
        gate_status: MacroExpansionGateStatus::Gap,
        evidence_kind: "compiler_observed_expansion".to_string(),
        reason: "compiler-observed macro expansion was not executed; static capture records a GAP"
            .to_string(),
    });
}

fn push_proc_macro_crate(
    inventory: &mut ProcMacroInventory,
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    name: &str,
    export_kind: ProcMacroExportKind,
) {
    inventory.crate_exports.push(ProcMacroCrateRow {
        stable_id: stable_macro_id(
            context_id,
            relative_path,
            module_path,
            export_kind.as_str(),
            name,
            "",
        ),
        context_id: context_id.to_string(),
        relative_path: relative_path.to_string(),
        module_path: module_path.to_string(),
        name: name.to_string(),
        export_kind,
        confidence: StaticCaptureConfidence::SyntaxOnly,
    });
    push_proc_macro_gaps(inventory, context_id, relative_path, module_path, name);
}

fn push_proc_macro_invocation(
    inventory: &mut ProcMacroInventory,
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    macro_path: &str,
    invocation_kind: ProcMacroInvocationKind,
    tokens: &str,
) {
    inventory.invocations.push(ProcMacroInvocationRow {
        stable_id: stable_macro_id(
            context_id,
            relative_path,
            module_path,
            invocation_kind.as_str(),
            macro_path,
            tokens,
        ),
        context_id: context_id.to_string(),
        relative_path: relative_path.to_string(),
        module_path: module_path.to_string(),
        macro_path: macro_path.to_string(),
        invocation_kind,
        token_summary: summarize_tokens(tokens),
        confidence: StaticCaptureConfidence::SyntaxOnly,
    });
    push_proc_macro_gaps(
        inventory,
        context_id,
        relative_path,
        module_path,
        macro_path,
    );
}

fn push_proc_macro_gaps(
    inventory: &mut ProcMacroInventory,
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    macro_name: &str,
) {
    for missing_truth in [
        ProcMacroMissingTruth::OutputTokenStream,
        ProcMacroMissingTruth::Panic,
        ProcMacroMissingTruth::Environment,
        ProcMacroMissingTruth::FileAccess,
    ] {
        inventory.gaps.push(ProcMacroCaptureGap {
            context_id: context_id.to_string(),
            relative_path: relative_path.to_string(),
            module_path: module_path.to_string(),
            macro_name: macro_name.to_string(),
            missing_truth,
            reason: "static proc-macro detection does not execute proc macros".to_string(),
        });
    }
}

fn push_row(
    rows: &mut Vec<RustItemRow>,
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    item_kind: RustItemKind,
    name: &str,
    visibility: RustVisibility,
) {
    push_row_with_identity(
        rows,
        RustItemInput {
            context_id,
            relative_path,
            module_path,
            item_kind,
            name,
            visibility,
            identity_kind: RustIdentityKind::StableNamed,
            identity_note: "named syntax identity is stable across repeated scans",
        },
    );
}

struct RustItemInput<'a> {
    context_id: &'a str,
    relative_path: &'a str,
    module_path: &'a str,
    item_kind: RustItemKind,
    name: &'a str,
    visibility: RustVisibility,
    identity_kind: RustIdentityKind,
    identity_note: &'a str,
}

fn push_row_with_identity(rows: &mut Vec<RustItemRow>, input: RustItemInput<'_>) {
    rows.push(RustItemRow {
        stable_id: stable_item_id(
            input.context_id,
            input.relative_path,
            input.module_path,
            input.item_kind,
            input.name,
        ),
        context_id: input.context_id.to_string(),
        relative_path: input.relative_path.to_string(),
        module_path: input.module_path.to_string(),
        item_kind: input.item_kind,
        name: input.name.to_string(),
        identity_kind: input.identity_kind,
        identity_note: input.identity_note.to_string(),
        visibility: input.visibility,
        confidence: StaticCaptureConfidence::SyntaxOnly,
    });
}

fn stable_item_id(
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    item_kind: RustItemKind,
    name: &str,
) -> String {
    let mut hasher = Sha256::new();
    for value in [
        context_id,
        relative_path,
        module_path,
        item_kind.as_str(),
        name,
    ] {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn stable_macro_id(
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    row_kind: &str,
    name: &str,
    tokens: &str,
) -> String {
    let mut hasher = Sha256::new();
    for value in [
        context_id,
        relative_path,
        module_path,
        row_kind,
        name,
        tokens,
    ] {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn semantic_hash_input(row: &RustItemRow) -> String {
    format!(
        "{}\0{}\0{}\0{}\0{}\0{}\0{}",
        row.relative_path,
        row.module_path,
        row.item_kind.as_str(),
        row.name,
        row.visibility.as_str(),
        row.identity_kind.as_str(),
        row.identity_note
    )
}

fn hash_lines(lines: &[String]) -> String {
    let mut hasher = Sha256::new();
    for line in lines {
        hasher.update(line.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn path_to_string(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

trait MetaTokenString {
    fn to_token_stream_string(&self) -> String;
}

impl MetaTokenString for Meta {
    fn to_token_stream_string(&self) -> String {
        match self {
            Meta::Path(path) => path_to_string(path),
            Meta::List(list) => format!("{} {}", path_to_string(&list.path), list.tokens),
            Meta::NameValue(name_value) => path_to_string(&name_value.path),
        }
    }
}

fn summarize_macro_matcher(tokens: &str) -> String {
    summarize_macro_side(tokens, true)
}

fn summarize_macro_transcriber(tokens: &str) -> String {
    summarize_macro_side(tokens, false)
}

fn summarize_macro_side(tokens: &str, matcher: bool) -> String {
    let marker = "=>";
    let value = if let Some(index) = tokens.find(marker) {
        if matcher {
            &tokens[..index]
        } else {
            &tokens[index + marker.len()..]
        }
    } else {
        tokens
    };
    summarize_tokens(value)
}

fn summarize_tokens(tokens: &str) -> String {
    tokens.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn classify_visibility(visibility: &Visibility) -> RustVisibility {
    match visibility {
        Visibility::Public(_) => RustVisibility::Public,
        Visibility::Restricted(restricted) if restricted.path.is_ident("crate") => {
            RustVisibility::Crate
        }
        Visibility::Restricted(_) => RustVisibility::Restricted,
        Visibility::Inherited => RustVisibility::Private,
    }
}

fn join_module_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}::{child}")
    }
}

fn relative_path(root: &Path, path: &Path) -> Result<String, RustStaticError> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .to_str()
        .map(|value| value.replace('\\', "/"))
        .ok_or_else(|| RustStaticError::NonUtf8Path {
            path: path.to_path_buf(),
        })
}

#[cfg(test)]
mod compiler_broker;
#[cfg(test)]
mod compiler_execution_gate_tests;
#[cfg(test)]
mod compiler_observed_tests;
#[cfg(test)]
mod tests {
    // Test lane: default

    use super::*;

    // Defends: CDB022 captures simple static Rust items deterministically without semantic overclaim.
    #[test]
    fn simple_item_fixture_passes() {
        let fixture = FixtureWorkspace::new();
        fixture.write(
            "src/lib.rs",
            r#"
pub mod inner {
    pub struct Thing;
    enum Hidden {
        One,
    }
    pub(crate) trait DoIt {}
    pub fn make() {}
}

pub type Alias = inner::Thing;
const LIMIT: usize = 8;
static NAME: &str = "codedb";
use inner::Thing;
impl Thing {}
"#,
        );

        let first =
            capture_rust_items(&fixture.root, fixture.root.join("src/lib.rs"), "ctx-1").unwrap();
        let second =
            capture_rust_items(&fixture.root, fixture.root.join("src/lib.rs"), "ctx-1").unwrap();

        assert_eq!(first, second);
        assert!(first.iter().all(|row| row.context_id == "ctx-1"));
        assert!(first.iter().all(|row| !row.stable_id.is_empty()));
        assert!(
            first
                .iter()
                .all(|row| row.confidence == StaticCaptureConfidence::SyntaxOnly)
        );
        assert!(first.iter().any(|row| {
            row.item_kind == RustItemKind::Module
                && row.name == "inner"
                && row.module_path.is_empty()
        }));
        assert!(first.iter().any(|row| {
            row.item_kind == RustItemKind::Struct
                && row.name == "Thing"
                && row.module_path == "inner"
                && row.visibility == RustVisibility::Public
        }));
        assert!(first.iter().any(|row| {
            row.item_kind == RustItemKind::Trait
                && row.name == "DoIt"
                && row.visibility == RustVisibility::Crate
        }));
        assert!(
            first
                .iter()
                .any(|row| row.item_kind == RustItemKind::Function && row.name == "make")
        );
        assert!(
            first
                .iter()
                .any(|row| row.item_kind == RustItemKind::TypeAlias && row.name == "Alias")
        );
        assert!(first.iter().any(|row| row.item_kind == RustItemKind::Impl
            && row.name == "impl#1"
            && row.identity_kind == RustIdentityKind::UnstableAnonymous));
    }

    // Defends: CDB084 anonymous syntax nodes receive deterministic IDs but remain marked unstable.
    #[test]
    fn anonymous_impl_identity_is_distinct_and_marked_unstable() {
        let fixture = FixtureWorkspace::new();
        fixture.write(
            "src/lib.rs",
            r#"
struct One;
struct Two;

impl One {}
impl Two {}
"#,
        );

        let first =
            capture_rust_items(&fixture.root, fixture.root.join("src/lib.rs"), "ctx-1").unwrap();
        let second =
            capture_rust_items(&fixture.root, fixture.root.join("src/lib.rs"), "ctx-1").unwrap();

        assert_eq!(first, second);
        let impl_rows = first
            .iter()
            .filter(|row| row.item_kind == RustItemKind::Impl)
            .collect::<Vec<_>>();
        assert_eq!(impl_rows.len(), 2);
        assert_ne!(impl_rows[0].stable_id, impl_rows[1].stable_id);
        assert_eq!(impl_rows[0].name, "impl#1");
        assert_eq!(impl_rows[1].name, "impl#2");
        assert!(impl_rows.iter().all(|row| {
            row.identity_kind == RustIdentityKind::UnstableAnonymous
                && row.identity_note.contains("source-drift sensitive")
        }));

        let named_rows = first
            .iter()
            .filter(|row| row.item_kind == RustItemKind::Struct)
            .collect::<Vec<_>>();
        assert!(
            named_rows
                .iter()
                .all(|row| row.identity_kind == RustIdentityKind::StableNamed)
        );
    }

    // Defends: CDB085 public API hashing ignores private/body drift but moves on public symbol drift.
    #[test]
    fn semantic_and_public_api_hashes_are_stable_for_expected_inputs() {
        let fixture = FixtureWorkspace::new();
        fixture.write(
            "src/lib.rs",
            r#"
pub fn public_api() -> usize {
    1
}

fn helper() -> usize {
    1
}
"#,
        );
        let base_rows =
            capture_rust_items(&fixture.root, fixture.root.join("src/lib.rs"), "ctx-1").unwrap();
        let base = semantic_hash_report(&base_rows);

        fixture.write(
            "src/lib.rs",
            r#"
// comment drift should not affect static item hashes
pub fn public_api() -> usize {
    2
}

fn helper_private_renamed() -> usize {
    2
}
"#,
        );
        let private_drift_rows =
            capture_rust_items(&fixture.root, fixture.root.join("src/lib.rs"), "ctx-1").unwrap();
        let private_drift = semantic_hash_report(&private_drift_rows);

        assert_ne!(base.semantic_hash, private_drift.semantic_hash);
        assert_eq!(base.public_api_hash, private_drift.public_api_hash);
        assert!(
            private_drift
                .limitation
                .contains("excludes function bodies")
        );

        fixture.write(
            "src/lib.rs",
            r#"
pub fn public_api_renamed() -> usize {
    2
}

fn helper_private_renamed() -> usize {
    2
}
"#,
        );
        let public_drift_rows =
            capture_rust_items(&fixture.root, fixture.root.join("src/lib.rs"), "ctx-1").unwrap();
        let public_drift = semantic_hash_report(&public_drift_rows);

        assert_ne!(base.public_api_hash, public_drift.public_api_hash);
        assert!(
            base.public_api_inputs
                .iter()
                .any(|input| input.contains("public_api"))
        );
        assert!(
            public_drift
                .public_api_inputs
                .iter()
                .any(|input| input.contains("public_api_renamed"))
        );
    }

    // Defends: CDB077 records compiler-observed macro expansion, resolution, and hygiene
    // only when the configured compiler supplies all required evidence.
    #[test]
    fn compiler_observed_macro_evidence_is_provenanced_or_fails_closed() {
        let fixture_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/macro_rules/src/lib.rs");
        let options = CompilerEvidenceOptions {
            enabled: true,
            edition: "2021".to_string(),
            ..CompilerEvidenceOptions::default()
        };
        let authority = CompilerExecutionApprovalAuthority::new().expect("approval authority");
        let capability = authority
            .approve(&fixture_path, &options)
            .expect("request-bound compiler approval");
        let report = capture_compiler_evidence_with_capability(
            &authority,
            capability,
            &fixture_path,
            options,
        );
        assert_eq!(
            report.collection_status,
            CompilerEvidenceCollectionStatus::CompilerObserved,
            "sandboxed compiler evidence failed: {:#?}",
            report.gaps
        );

        match report.collection_status {
            CompilerEvidenceCollectionStatus::CompilerObserved => {
                let provenance = report.toolchain.as_ref().expect("toolchain provenance");
                assert!(provenance.rustc_version.contains("commit-hash:"));
                assert!(provenance.rustdoc_version.contains("commit-hash:"));
                let expansion = report
                    .artifact(CompilerEvidenceArtifactKind::MacroExpansion)
                    .expect("macro expansion evidence");
                assert!(
                    expansion
                        .command
                        .iter()
                        .any(|arg| arg == "-Zunpretty=expanded,identified")
                );
                assert!(expansion.output.as_deref().is_some_and(|output| {
                    output.contains("generated_answer") && output.contains("/*")
                }));
                assert_eq!(
                    report
                        .artifact(CompilerEvidenceArtifactKind::MacroResolution)
                        .expect("macro resolution evidence")
                        .status,
                    CompilerArtifactStatus::CompilerObserved
                );
                let hygiene = report
                    .artifact(CompilerEvidenceArtifactKind::MacroHygiene)
                    .expect("macro hygiene evidence");
                assert!(
                    hygiene
                        .command
                        .iter()
                        .any(|arg| arg == "-Zunpretty=expanded,hygiene")
                );
                assert!(hygiene.output.as_deref().is_some_and(|output| {
                    output.contains("Expansions:")
                        && output.contains("SyntaxContexts:")
                        && output.contains("add_one_with_hygienic_local")
                }));
                assert_eq!(
                    report
                        .artifact(CompilerEvidenceArtifactKind::Hir)
                        .expect("HIR evidence")
                        .status,
                    CompilerArtifactStatus::CompilerObserved
                );
                assert_eq!(
                    report
                        .artifact(CompilerEvidenceArtifactKind::Mir)
                        .expect("MIR evidence")
                        .status,
                    CompilerArtifactStatus::CompilerObserved
                );
            }
            CompilerEvidenceCollectionStatus::EvidenceUnavailable => {
                assert!(report.semantic_hash.is_none());
                assert!(report.public_api_hash.is_none());
                assert!(!report.gaps.is_empty());
            }
        }
    }

    // Defends: the compiler-capture layer is opt-in; its default cannot turn
    // static inventory into a compiler-observed semantic or public-API claim.
    #[test]
    fn compiler_evidence_defaults_to_fail_closed() {
        let fixture = FixtureWorkspace::new();
        fixture.write(
            "src/lib.rs",
            r#"
pub fn visible() -> u32 {
    42
}
"#,
        );
        let report = capture_compiler_evidence(
            fixture.root.join("src/lib.rs"),
            CompilerEvidenceOptions::default(),
        );

        assert_eq!(
            report.collection_status,
            CompilerEvidenceCollectionStatus::EvidenceUnavailable
        );
        assert!(report.artifacts.is_empty());
        assert!(report.semantic_hash.is_none());
        assert!(report.public_api_hash.is_none());
        assert!(
            report
                .gaps
                .iter()
                .any(|gap| gap.reason.contains("disabled"))
        );
    }

    // Defends: CDB084 retains named identities across a source shift but refuses to
    // match scan-order anonymous identities after that shift.
    #[test]
    fn identity_scan_reports_anonymous_source_shift_as_conflict() {
        let fixture = FixtureWorkspace::new();
        fixture.write(
            "src/lib.rs",
            r#"
struct One;
struct Two;

impl One {}
impl Two {}
"#,
        );
        let source_path = fixture.root.join("src/lib.rs");
        let first = capture_rust_item_scan(&fixture.root, &source_path, "ctx-1").unwrap();
        let repeated = capture_rust_item_scan(&fixture.root, &source_path, "ctx-1").unwrap();
        let repeat_comparison = compare_rust_item_scans(&first, &repeated);
        assert_eq!(
            repeat_comparison.status,
            RustIdentityComparisonStatus::RepeatScanVerified
        );
        assert!(repeat_comparison.conflicts.is_empty());

        fixture.write(
            "src/lib.rs",
            r#"
struct Zero;
struct One;
struct Two;

impl Zero {}
impl One {}
impl Two {}
"#,
        );
        let shifted = capture_rust_item_scan(&fixture.root, &source_path, "ctx-1").unwrap();
        let shifted_comparison = compare_rust_item_scans(&first, &shifted);

        assert_eq!(
            shifted_comparison.status,
            RustIdentityComparisonStatus::SourceShiftConflict
        );
        assert!(
            shifted_comparison
                .stable_matches
                .iter()
                .any(|row| row.name == "One")
        );
        assert!(
            shifted_comparison
                .stable_matches
                .iter()
                .any(|row| row.name == "Two")
        );
        assert_eq!(shifted_comparison.conflicts.len(), 3);
        assert!(shifted_comparison.conflicts.iter().all(|conflict| {
            conflict.kind == RustIdentityConflictKind::UnstableAnonymousSourceShift
                && conflict.reason.contains("not matched")
        }));
    }

    // Defends: CDB085 binds semantic and public-API hashes to compiler HIR/MIR and
    // rustdoc JSON evidence, but produces no such claim if evidence collection fails.
    #[test]
    fn compiler_and_rustdoc_semantic_evidence_tracks_public_api_source_drift() {
        let fixture = FixtureWorkspace::new();
        let source_path = fixture.root.join("src/lib.rs");
        fixture.write(
            "src/lib.rs",
            r#"
pub fn public_api() -> u32 {
    1
}

fn private_helper() -> u32 {
    1
}
"#,
        );
        let options = CompilerEvidenceOptions {
            enabled: true,
            edition: "2024".to_string(),
            ..CompilerEvidenceOptions::default()
        };
        let authority = CompilerExecutionApprovalAuthority::new().expect("approval authority");
        let base_capability = authority
            .approve(&source_path, &options)
            .expect("base compiler approval");
        let base = capture_compiler_evidence_with_capability(
            &authority,
            base_capability,
            &source_path,
            options.clone(),
        );

        fixture.write(
            "src/lib.rs",
            r#"
pub fn public_api() -> u32 {
    2
}

fn renamed_private_helper() -> u32 {
    2
}
"#,
        );
        let private_capability = authority
            .approve(&source_path, &options)
            .expect("private-shift compiler approval");
        let private_shift = capture_compiler_evidence_with_capability(
            &authority,
            private_capability,
            &source_path,
            options.clone(),
        );

        fixture.write(
            "src/lib.rs",
            r#"
pub fn public_api() -> u64 {
    2
}

fn renamed_private_helper() -> u32 {
    2
}
"#,
        );
        let public_capability = authority
            .approve(&source_path, &options)
            .expect("public-shift compiler approval");
        let public_shift = capture_compiler_evidence_with_capability(
            &authority,
            public_capability,
            &source_path,
            options,
        );
        for report in [&base, &private_shift, &public_shift] {
            assert_eq!(
                report.collection_status,
                CompilerEvidenceCollectionStatus::CompilerObserved,
                "CDB085 requires positive compiler-observed evidence: {:#?}",
                report.gaps
            );
        }

        match (
            base.collection_status,
            private_shift.collection_status,
            public_shift.collection_status,
        ) {
            (
                CompilerEvidenceCollectionStatus::CompilerObserved,
                CompilerEvidenceCollectionStatus::CompilerObserved,
                CompilerEvidenceCollectionStatus::CompilerObserved,
            ) => {
                assert_ne!(base.semantic_hash, private_shift.semantic_hash);
                assert_eq!(base.public_api_hash, private_shift.public_api_hash);
                assert_ne!(base.public_api_hash, public_shift.public_api_hash);
                assert!(
                    base.semantic_inputs
                        .iter()
                        .any(|input| input.starts_with("hir\0"))
                );
                assert!(
                    base.semantic_inputs
                        .iter()
                        .any(|input| input.starts_with("mir\0"))
                );
                assert!(
                    base.semantic_inputs
                        .iter()
                        .any(|input| input == "edition\x002024")
                );
                assert!(
                    base.public_api_inputs
                        .iter()
                        .any(|input| input == "edition\x002024")
                );
                assert!(
                    base.public_api_inputs
                        .iter()
                        .any(|input| input.starts_with("rustdoc\0rustdoc "))
                );
                assert_eq!(
                    base.public_api_hash.as_deref(),
                    Some(hash_lines(&base.public_api_inputs).as_str())
                );
                assert!(
                    base.artifact(CompilerEvidenceArtifactKind::Hir)
                        .expect("HIR evidence")
                        .command
                        .iter()
                        .any(|arg| arg == "-Zunpretty=hir")
                );
                assert!(
                    base.artifact(CompilerEvidenceArtifactKind::Mir)
                        .expect("MIR evidence")
                        .command
                        .iter()
                        .any(|arg| arg == "-Zunpretty=mir")
                );
                assert!(
                    base.artifact(CompilerEvidenceArtifactKind::RustdocPublicApi)
                        .expect("rustdoc JSON evidence")
                        .output
                        .as_deref()
                        .is_some_and(|output| output.contains("public_api"))
                );
            }
            _ => {
                for report in [&base, &private_shift, &public_shift] {
                    assert!(report.semantic_hash.is_none());
                    assert!(report.public_api_hash.is_none());
                    assert!(!report.gaps.is_empty());
                }
            }
        }
    }

    // Defends: CDB023 captures macro_rules definitions/invocations and records gaps for expansion/hygiene.
    #[test]
    fn macro_fixture_passes_with_gaps() {
        let fixture = FixtureWorkspace::new();
        fixture.write(
            "src/lib.rs",
            r#"
macro_rules! hello {
    ($name:expr) => {
        format!("hello {}", $name)
    };
}

pub mod nested {
    macro_rules! local {
        () => { 1 };
    }

    local!();
}

hello!("codedb");

pub fn run() {
    hello!("agent");
}
"#,
        );

        let first =
            capture_rust_macros(&fixture.root, fixture.root.join("src/lib.rs"), "ctx-1").unwrap();
        let second =
            capture_rust_macros(&fixture.root, fixture.root.join("src/lib.rs"), "ctx-1").unwrap();

        assert_eq!(first, second);
        assert!(
            first
                .definitions
                .iter()
                .all(|row| row.confidence == StaticCaptureConfidence::SyntaxOnly)
        );
        assert!(
            first
                .invocations
                .iter()
                .all(|row| row.confidence == StaticCaptureConfidence::SyntaxOnly)
        );
        assert!(first.definitions.iter().any(|row| {
            row.name == "hello"
                && row.module_path.is_empty()
                && row.matcher_summary.contains("$ name : expr")
                && row.transcriber_summary.contains("format")
        }));
        assert!(
            first
                .definitions
                .iter()
                .any(|row| row.name == "local" && row.module_path == "nested")
        );
        assert!(first.invocations.iter().any(|row| {
            row.macro_path == "hello" && row.invocation_kind == MacroInvocationKind::Item
        }));
        assert!(first.invocations.iter().any(|row| {
            row.macro_path == "hello" && row.invocation_kind == MacroInvocationKind::Statement
        }));
        assert!(
            first
                .invocations
                .iter()
                .any(|row| row.macro_path == "local" && row.module_path == "nested")
        );
        assert!(first.gaps.iter().any(|gap| {
            gap.macro_name == "hello" && gap.missing_truth == MacroMissingTruth::Expansion
        }));
        assert!(first.gaps.iter().any(|gap| {
            gap.macro_name == "hello" && gap.missing_truth == MacroMissingTruth::Hygiene
        }));
    }

    // Defends: CDB077 gates dynamic/compiler-observed macro expansion as GAP, not FACT.
    #[test]
    fn macro_expansion_gate_records_question_not_fact() {
        let fixture = FixtureWorkspace::new();
        fixture.write(
            "src/lib.rs",
            r#"
macro_rules! make_item {
    () => { pub fn generated() {} };
}

make_item!();
"#,
        );

        let inventory =
            capture_rust_macros(&fixture.root, fixture.root.join("src/lib.rs"), "ctx-1").unwrap();

        assert!(inventory.expansion_gates.iter().any(|gate| {
            gate.macro_name == "make_item"
                && gate.gate_status == MacroExpansionGateStatus::Gap
                && gate.evidence_kind == "compiler_observed_expansion"
                && gate.reason.contains("not executed")
        }));
        assert!(inventory.gaps.iter().any(|gap| {
            gap.macro_name == "make_item" && gap.missing_truth == MacroMissingTruth::Expansion
        }));
    }

    // Defends: CDB024 statically detects proc-macro exports/invocation shapes without executing them.
    #[test]
    fn proc_macro_fixture_emits_static_rows_and_gaps() {
        let fixture = FixtureWorkspace::new();
        fixture.write(
            "src/lib.rs",
            r#"
#[proc_macro]
pub fn make_item(input: TokenStream) -> TokenStream {
    input
}

#[proc_macro_attribute]
pub fn traced(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

#[proc_macro_derive(Builder)]
pub fn derive_builder(input: TokenStream) -> TokenStream {
    input
}

#[derive(Builder, Debug)]
#[traced]
pub struct Thing;

make_item!(struct Generated;);
"#,
        );

        let first =
            capture_proc_macro_static(&fixture.root, fixture.root.join("src/lib.rs"), "ctx-1")
                .unwrap();
        let second =
            capture_proc_macro_static(&fixture.root, fixture.root.join("src/lib.rs"), "ctx-1")
                .unwrap();

        assert_eq!(first, second);
        assert!(
            first
                .crate_exports
                .iter()
                .all(|row| row.confidence == StaticCaptureConfidence::SyntaxOnly)
        );
        assert!(first.crate_exports.iter().any(|row| {
            row.name == "make_item" && row.export_kind == ProcMacroExportKind::FunctionLike
        }));
        assert!(first.crate_exports.iter().any(|row| {
            row.name == "traced" && row.export_kind == ProcMacroExportKind::Attribute
        }));
        assert!(first.crate_exports.iter().any(|row| {
            row.name == "derive_builder" && row.export_kind == ProcMacroExportKind::Derive
        }));
        assert!(first.invocations.iter().any(|row| {
            row.macro_path == "Builder" && row.invocation_kind == ProcMacroInvocationKind::Derive
        }));
        assert!(first.invocations.iter().any(|row| {
            row.macro_path == "traced" && row.invocation_kind == ProcMacroInvocationKind::Attribute
        }));
        assert!(first.invocations.iter().any(|row| {
            row.macro_path == "make_item"
                && row.invocation_kind == ProcMacroInvocationKind::FunctionLikeCandidate
        }));
        assert!(first.gaps.iter().any(|gap| {
            gap.macro_name == "make_item"
                && gap.missing_truth == ProcMacroMissingTruth::OutputTokenStream
        }));
        assert!(first.gaps.iter().any(|gap| {
            gap.macro_name == "traced" && gap.missing_truth == ProcMacroMissingTruth::FileAccess
        }));
    }

    // Defends: CDB025 detects build.rs and static Cargo instruction sites without executing build scripts.
    #[test]
    fn build_script_fixture_emits_static_rows_and_gaps() {
        let fixture = FixtureWorkspace::new();
        fixture.write(
            "build.rs",
            r#"
fn main() {
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo::rustc-link-lib=static=foo");
    helper();
}

fn helper() {
    eprintln!("cargo:warning=generated bindings are disabled in static capture");
}
"#,
        );

        let first =
            capture_build_script_static(&fixture.root, fixture.root.join("build.rs"), "ctx-1")
                .unwrap();
        let second =
            capture_build_script_static(&fixture.root, fixture.root.join("build.rs"), "ctx-1")
                .unwrap();

        assert_eq!(first, second);
        assert_eq!(first.scripts.len(), 1);
        assert!(first.scripts[0].is_canonical_build_rs);
        assert!(
            first
                .scripts
                .iter()
                .all(|row| row.confidence == StaticCaptureConfidence::SyntaxOnly)
        );
        assert!(first.instructions.iter().any(|row| {
            row.function_name == "main"
                && row.directive == "rerun-if-changed"
                && row.value == "wrapper.h"
                && row.macro_path == "println"
        }));
        assert!(first.instructions.iter().any(|row| {
            row.function_name == "main"
                && row.directive == "rustc-link-lib"
                && row.value == "static=foo"
        }));
        assert!(first.instructions.iter().any(|row| {
            row.function_name == "helper"
                && row.directive == "warning"
                && row.macro_path == "eprintln"
        }));
        assert!(first.gaps.iter().any(|gap| {
            gap.missing_truth == BuildScriptMissingTruth::Execution
                && gap.relative_path == "build.rs"
        }));
        assert!(first.gaps.iter().any(|gap| {
            gap.missing_truth == BuildScriptMissingTruth::OutDirArtifacts
                && gap.relative_path == "build.rs"
        }));
    }

    // Defends: CDB026 captures literal static include/path edges without claiming dynamic file tracing.
    #[test]
    fn include_fixture_passes() {
        let fixture = FixtureWorkspace::new();
        fixture.write(
            "src/lib.rs",
            r#"
#[path = "alt.rs"]
mod alt;

include!("generated.rs");

pub fn read_assets() {
    let _text = include_str!("assets/schema.nu");
    let _bytes = include_bytes!("assets/blob.bin");
    let _computed = include_str!(concat!("assets/", "dynamic.txt"));
}
"#,
        );

        let first =
            capture_static_include_edges(&fixture.root, fixture.root.join("src/lib.rs"), "ctx-1")
                .unwrap();
        let second =
            capture_static_include_edges(&fixture.root, fixture.root.join("src/lib.rs"), "ctx-1")
                .unwrap();

        assert_eq!(first, second);
        assert!(
            first
                .edges
                .iter()
                .all(|row| row.confidence == StaticCaptureConfidence::SyntaxOnly)
        );
        assert!(first.edges.iter().any(|row| {
            row.edge_kind == StaticIncludeEdgeKind::PathAttribute && row.target_path == "alt.rs"
        }));
        assert!(first.edges.iter().any(|row| {
            row.edge_kind == StaticIncludeEdgeKind::Include && row.target_path == "generated.rs"
        }));
        assert!(first.edges.iter().any(|row| {
            row.edge_kind == StaticIncludeEdgeKind::IncludeStr
                && row.target_path == "assets/schema.nu"
        }));
        assert!(first.edges.iter().any(|row| {
            row.edge_kind == StaticIncludeEdgeKind::IncludeBytes
                && row.target_path == "assets/blob.bin"
        }));
        assert!(first.gaps.iter().any(|gap| {
            gap.edge_kind == StaticIncludeEdgeKind::IncludeStr
                && gap.reason == "include macro target is not a string literal"
        }));
    }

    // Defends: CDB027 projects native/link rows from static build-script instructions without linker execution.
    #[test]
    fn native_link_fixture_emits_static_rows_and_gaps() {
        let fixture = FixtureWorkspace::new();
        fixture.write(
            "build.rs",
            r#"
fn main() {
    println!("cargo:rustc-link-lib=static=foo");
    println!("cargo:rustc-link-search=native=/opt/foo/lib");
    println!("cargo:rustc-link-arg=-Wl,--as-needed");
}
"#,
        );

        let first =
            capture_native_link_static(&fixture.root, fixture.root.join("build.rs"), "ctx-1")
                .unwrap();
        let second =
            capture_native_link_static(&fixture.root, fixture.root.join("build.rs"), "ctx-1")
                .unwrap();

        assert_eq!(first, second);
        assert!(
            first.libraries.iter().any(|row| {
                row.library == "foo" && row.library_kind.as_deref() == Some("static")
            })
        );
        assert!(first.link_search_paths.iter().any(|row| {
            row.path == "/opt/foo/lib" && row.search_kind.as_deref() == Some("native")
        }));
        assert!(
            first
                .link_args
                .iter()
                .any(|row| row.arg == "-Wl,--as-needed")
        );
        assert!(first.gaps.iter().any(|gap| {
            gap.missing_truth == NativeLinkMissingTruth::LinkerTool
                && gap.relative_path == "build.rs"
        }));
        assert!(first.gaps.iter().any(|gap| {
            gap.missing_truth == NativeLinkMissingTruth::LibraryAvailability
                && gap.relative_path == "build.rs"
        }));
        assert!(
            first
                .gaps
                .iter()
                .any(|gap| gap.missing_truth == NativeLinkMissingTruth::LinkResult)
        );
    }

    #[test]
    fn fixture_workspace_roots_are_reserved_and_collision_free() {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    (0..16)
                        .map(|_| {
                            let workspace = FixtureWorkspace::new();
                            workspace.write("src/lib.rs", "pub fn probe() {}\n");
                            workspace
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let mut workspaces: Vec<FixtureWorkspace> = handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("fixture thread"))
            .collect();
        let mut roots: Vec<_> = workspaces
            .iter()
            .map(|workspace| workspace.root.clone())
            .collect();
        roots.sort();
        roots.dedup();
        assert_eq!(
            roots.len(),
            workspaces.len(),
            "fixture roots must be unique so one Drop can never delete a live sibling"
        );
        let survivor = workspaces.pop().expect("at least one workspace");
        drop(workspaces);
        assert!(
            survivor.root.join("src/lib.rs").is_file(),
            "dropping sibling workspaces must not remove another workspace's files"
        );
    }

    struct FixtureWorkspace {
        root: PathBuf,
    }

    impl FixtureWorkspace {
        fn new() -> Self {
            static NEXT_FIXTURE_ID: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            let sequence = NEXT_FIXTURE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "codedb_rust_static_fixture_{}_{}",
                std::process::id(),
                sequence
            ));
            fs::create_dir(&root).expect("reserve unique fixture root");
            Self { root }
        }

        fn write(&self, relative_path: &str, content: &str) {
            let path = self.root.join(relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create fixture parent");
            }
            fs::write(path, content).expect("write fixture file");
        }
    }

    impl Drop for FixtureWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
