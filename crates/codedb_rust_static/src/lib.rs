#![forbid(unsafe_code)]

use std::error::Error as StdError;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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
    inventory
        .gaps
        .sort_by(|left, right| left.missing_truth.cmp(&right.missing_truth));
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
    inventory
        .gaps
        .sort_by(|left, right| left.missing_truth.cmp(&right.missing_truth));
    Ok(inventory)
}

fn collect_items(
    items: &[Item],
    context_id: &str,
    relative_path: &str,
    module_path: &str,
    rows: &mut Vec<RustItemRow>,
) {
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
            Item::Impl(_) => push_row(
                rows,
                context_id,
                relative_path,
                module_path,
                RustItemKind::Impl,
                "impl",
                RustVisibility::Private,
            ),
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
                if let Expr::Lit(expr_lit) = &name_value.value {
                    if let Lit::Str(lit_str) = &expr_lit.lit {
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
    rows.push(RustItemRow {
        stable_id: stable_item_id(context_id, relative_path, module_path, item_kind, name),
        context_id: context_id.to_string(),
        relative_path: relative_path.to_string(),
        module_path: module_path.to_string(),
        item_kind,
        name: name.to_string(),
        visibility,
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
mod tests {
    // Test lane: default

    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

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
        assert!(
            first
                .iter()
                .any(|row| row.item_kind == RustItemKind::Impl && row.name == "impl")
        );
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

    struct FixtureWorkspace {
        root: PathBuf,
    }

    impl FixtureWorkspace {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock before unix epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "codedb_rust_static_fixture_{}_{}",
                std::process::id(),
                nonce
            ));
            fs::create_dir_all(&root).expect("create fixture root");
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
