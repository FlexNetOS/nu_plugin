#![forbid(unsafe_code)]

use std::error::Error as StdError;
use std::fmt::{Display, Formatter};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const STATUS: &str = "cargo_metadata_capture_available";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoMetadataCapture {
    pub workspace: CargoWorkspaceRow,
    pub packages: Vec<CargoPackageRow>,
    pub targets: Vec<CargoTargetRow>,
    pub dependencies: Vec<CargoDependencyRow>,
    pub resolve_nodes: Vec<CargoResolveNodeRow>,
    pub features: Vec<CargoFeatureRow>,
    pub sources: Vec<CargoSourceRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoContextCapture {
    pub context: CodeDbContextRow,
    pub toolchain: ToolchainRow,
    pub cargo_version: CargoVersionRow,
    pub rustc_version: RustcVersionRow,
    pub target: TargetTripleRow,
    pub host: HostTripleRow,
    pub cfgs: Vec<TargetCfgRow>,
    pub feature_set: FeatureSetRow,
    pub profile: CargoProfileRow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeDbContextRow {
    pub context_id: String,
    pub toolchain_id: String,
    pub target_triple: String,
    pub feature_set_hash: String,
    pub cfg_hash: String,
    pub cargo_lock_hash: String,
    pub profile: String,
    pub edition: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolchainRow {
    pub toolchain_id: String,
    pub cargo_version: String,
    pub rustc_version: String,
    pub host_triple: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoVersionRow {
    pub toolchain_id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustcVersionRow {
    pub toolchain_id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetTripleRow {
    pub context_id: String,
    pub triple: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostTripleRow {
    pub toolchain_id: String,
    pub triple: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetCfgRow {
    pub context_id: String,
    pub cfg: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureSetRow {
    pub feature_set_hash: String,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoProfileRow {
    pub context_id: String,
    pub profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoContextInput {
    pub cargo_version: String,
    pub rustc_version: String,
    pub host_triple: String,
    pub target_triple: String,
    pub cfgs: Vec<String>,
    pub features: Vec<String>,
    pub profile: String,
    pub edition: String,
    pub cargo_lock_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoWorkspaceRow {
    pub workspace_root: String,
    pub target_directory: String,
    pub workspace_members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoPackageRow {
    pub package_id: String,
    pub name: String,
    pub version: String,
    pub edition: String,
    pub manifest_path: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoTargetRow {
    pub package_id: String,
    pub name: String,
    pub kind: Vec<String>,
    pub crate_types: Vec<String>,
    pub src_path: String,
    pub edition: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoDependencyRow {
    pub package_id: String,
    pub name: String,
    pub req: String,
    pub kind: Option<String>,
    pub target: Option<String>,
    pub optional: bool,
    pub uses_default_features: bool,
    pub features: Vec<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoResolveNodeRow {
    pub package_id: String,
    pub dependencies: Vec<String>,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoFeatureRow {
    pub package_id: String,
    pub feature: String,
    pub enables: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CargoSourceKind {
    Path,
    Registry,
    Git,
    Unknown,
}

impl CargoSourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Registry => "registry",
            Self::Git => "git",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoSourceRow {
    pub owner_package_id: String,
    pub source_name: String,
    pub kind: CargoSourceKind,
    pub source: Option<String>,
    pub observed_from: CargoSourceObservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CargoSourceObservation {
    Package,
    Dependency,
}

#[derive(Debug)]
pub enum CargoMetadataError {
    NonUtf8Path { path: PathBuf },
    Spawn { source: io::Error },
    Failed { status: i32, stderr: String },
    Parse { source: serde_json::Error },
}

impl Display for CargoMetadataError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonUtf8Path { path } => {
                write!(f, "path is not valid UTF-8: {}", path.display())
            }
            Self::Spawn { source } => write!(f, "failed to run cargo metadata: {source}"),
            Self::Failed { status, stderr } => {
                write!(f, "cargo metadata exited with status {status}: {stderr}")
            }
            Self::Parse { source } => write!(f, "failed to parse cargo metadata JSON: {source}"),
        }
    }
}

impl StdError for CargoMetadataError {}

pub fn capture_cargo_metadata(
    manifest_path: impl AsRef<Path>,
) -> Result<CargoMetadataCapture, CargoMetadataError> {
    let manifest_path = manifest_path.as_ref();
    let manifest_path_arg =
        manifest_path
            .to_str()
            .ok_or_else(|| CargoMetadataError::NonUtf8Path {
                path: manifest_path.to_path_buf(),
            })?;

    let output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--manifest-path",
            manifest_path_arg,
        ])
        .output()
        .map_err(|source| CargoMetadataError::Spawn { source })?;

    if !output.status.success() {
        return Err(CargoMetadataError::Failed {
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    let metadata: Metadata = serde_json::from_slice(&output.stdout)
        .map_err(|source| CargoMetadataError::Parse { source })?;

    Ok(metadata.into_capture())
}

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<MetadataPackage>,
    workspace_members: Vec<String>,
    workspace_root: String,
    target_directory: String,
    resolve: Option<MetadataResolve>,
}

#[derive(Debug, Deserialize)]
struct MetadataPackage {
    id: String,
    name: String,
    version: String,
    source: Option<String>,
    manifest_path: String,
    edition: String,
    targets: Vec<MetadataTarget>,
    dependencies: Vec<MetadataDependency>,
    features: std::collections::BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct MetadataTarget {
    name: String,
    kind: Vec<String>,
    crate_types: Vec<String>,
    src_path: String,
    edition: String,
}

#[derive(Debug, Deserialize)]
struct MetadataDependency {
    name: String,
    source: Option<String>,
    req: String,
    kind: Option<String>,
    target: Option<String>,
    optional: bool,
    uses_default_features: bool,
    features: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct MetadataResolve {
    nodes: Vec<MetadataResolveNode>,
}

#[derive(Debug, Deserialize)]
struct MetadataResolveNode {
    id: String,
    dependencies: Vec<String>,
    features: Vec<String>,
}

impl Metadata {
    fn into_capture(self) -> CargoMetadataCapture {
        let workspace = CargoWorkspaceRow {
            workspace_root: self.workspace_root,
            target_directory: self.target_directory,
            workspace_members: sorted(self.workspace_members),
        };

        let mut packages = Vec::new();
        let mut targets = Vec::new();
        let mut dependencies = Vec::new();
        let mut features = Vec::new();
        let mut sources = Vec::new();

        for package in self.packages {
            let package_id = package.id;
            sources.push(CargoSourceRow {
                owner_package_id: package_id.clone(),
                source_name: package.name.clone(),
                kind: classify_cargo_source(package.source.as_deref()),
                source: package.source.clone(),
                observed_from: CargoSourceObservation::Package,
            });

            packages.push(CargoPackageRow {
                package_id: package_id.clone(),
                name: package.name,
                version: package.version,
                edition: package.edition,
                manifest_path: package.manifest_path,
                source: package.source,
            });

            for target in package.targets {
                targets.push(CargoTargetRow {
                    package_id: package_id.clone(),
                    name: target.name,
                    kind: sorted(target.kind),
                    crate_types: sorted(target.crate_types),
                    src_path: target.src_path,
                    edition: target.edition,
                });
            }

            for dependency in package.dependencies {
                sources.push(CargoSourceRow {
                    owner_package_id: package_id.clone(),
                    source_name: dependency.name.clone(),
                    kind: classify_cargo_source(dependency.source.as_deref()),
                    source: dependency.source.clone(),
                    observed_from: CargoSourceObservation::Dependency,
                });

                dependencies.push(CargoDependencyRow {
                    package_id: package_id.clone(),
                    name: dependency.name,
                    req: dependency.req,
                    kind: dependency.kind,
                    target: dependency.target,
                    optional: dependency.optional,
                    uses_default_features: dependency.uses_default_features,
                    features: sorted(dependency.features),
                    source: dependency.source,
                });
            }

            for (feature, enables) in package.features {
                features.push(CargoFeatureRow {
                    package_id: package_id.clone(),
                    feature,
                    enables: sorted(enables),
                });
            }
        }

        let mut resolve_nodes = self
            .resolve
            .map(|resolve| {
                resolve
                    .nodes
                    .into_iter()
                    .map(|node| CargoResolveNodeRow {
                        package_id: node.id,
                        dependencies: sorted(node.dependencies),
                        features: sorted(node.features),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        packages.sort_by(|left, right| left.package_id.cmp(&right.package_id));
        targets.sort_by(|left, right| {
            left.package_id
                .cmp(&right.package_id)
                .then_with(|| left.name.cmp(&right.name))
        });
        dependencies.sort_by(|left, right| {
            left.package_id
                .cmp(&right.package_id)
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.kind.cmp(&right.kind))
                .then_with(|| left.target.cmp(&right.target))
        });
        resolve_nodes.sort_by(|left, right| left.package_id.cmp(&right.package_id));
        features.sort_by(|left, right| {
            left.package_id
                .cmp(&right.package_id)
                .then_with(|| left.feature.cmp(&right.feature))
        });
        sources.sort_by(|left, right| {
            left.owner_package_id
                .cmp(&right.owner_package_id)
                .then_with(|| left.observed_from.cmp(&right.observed_from))
                .then_with(|| left.source_name.cmp(&right.source_name))
                .then_with(|| left.source.cmp(&right.source))
        });

        CargoMetadataCapture {
            workspace,
            packages,
            targets,
            dependencies,
            resolve_nodes,
            features,
            sources,
        }
    }
}

pub fn classify_cargo_source(source: Option<&str>) -> CargoSourceKind {
    match source {
        None => CargoSourceKind::Path,
        Some(value) if value.starts_with("registry+") || value.starts_with("sparse+") => {
            CargoSourceKind::Registry
        }
        Some(value) if value.starts_with("git+") => CargoSourceKind::Git,
        Some(_) => CargoSourceKind::Unknown,
    }
}

pub fn build_context_rows(input: CargoContextInput) -> CargoContextCapture {
    let cfgs = sorted(input.cfgs);
    let features = sorted(input.features);
    let cfg_hash = stable_hash(&cfgs);
    let feature_set_hash = stable_hash(&features);
    let cargo_lock_hash = input.cargo_lock_hash.unwrap_or_else(|| "none".to_string());
    let toolchain_material = [
        input.cargo_version.as_str(),
        input.rustc_version.as_str(),
        input.host_triple.as_str(),
    ];
    let toolchain_id = stable_hash(toolchain_material);
    let context_material = [
        toolchain_id.as_str(),
        input.target_triple.as_str(),
        feature_set_hash.as_str(),
        cfg_hash.as_str(),
        cargo_lock_hash.as_str(),
        input.profile.as_str(),
        input.edition.as_str(),
    ];
    let context_id = stable_hash(context_material);

    CargoContextCapture {
        context: CodeDbContextRow {
            context_id: context_id.clone(),
            toolchain_id: toolchain_id.clone(),
            target_triple: input.target_triple.clone(),
            feature_set_hash: feature_set_hash.clone(),
            cfg_hash: cfg_hash.clone(),
            cargo_lock_hash,
            profile: input.profile.clone(),
            edition: input.edition,
        },
        toolchain: ToolchainRow {
            toolchain_id: toolchain_id.clone(),
            cargo_version: input.cargo_version.clone(),
            rustc_version: input.rustc_version.clone(),
            host_triple: input.host_triple.clone(),
        },
        cargo_version: CargoVersionRow {
            toolchain_id: toolchain_id.clone(),
            version: input.cargo_version,
        },
        rustc_version: RustcVersionRow {
            toolchain_id: toolchain_id.clone(),
            version: input.rustc_version,
        },
        target: TargetTripleRow {
            context_id: context_id.clone(),
            triple: input.target_triple,
        },
        host: HostTripleRow {
            toolchain_id,
            triple: input.host_triple,
        },
        cfgs: cfgs
            .into_iter()
            .map(|cfg| TargetCfgRow {
                context_id: context_id.clone(),
                cfg,
            })
            .collect(),
        feature_set: FeatureSetRow {
            feature_set_hash,
            features,
        },
        profile: CargoProfileRow {
            context_id,
            profile: input.profile,
        },
    }
}

fn stable_hash(values: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    let mut hasher = Sha256::new();
    for value in values {
        hasher.update(value.as_ref().as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn sorted(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

#[cfg(test)]
mod tests {
    // Test lane: default

    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Defends: CDB019 must use Cargo's structured metadata output and produce stable package/target rows.
    #[test]
    fn cargo_metadata_fixture_capture_is_stable() {
        let fixture = FixtureWorkspace::new();
        fixture.write(
            "Cargo.toml",
            r#"[package]
name = "codedb_fixture"
version = "0.1.0"
edition = "2024"

[features]
default = ["serde"]
serde = []

[dependencies]
helper = { path = "helper", optional = true }
"#,
        );
        fixture.write("src/lib.rs", "pub fn answer() -> u8 { 42 }\n");
        fixture.write(
            "helper/Cargo.toml",
            r#"[package]
name = "helper"
version = "0.1.0"
edition = "2024"
"#,
        );
        fixture.write("helper/src/lib.rs", "pub fn helper() {}\n");

        let manifest_path = fixture.root.join("Cargo.toml");
        let first = capture_cargo_metadata(&manifest_path).expect("first metadata capture");
        let second = capture_cargo_metadata(&manifest_path).expect("second metadata capture");

        assert_eq!(first, second);
        assert_eq!(first.packages.len(), 1);
        assert_eq!(first.workspace.workspace_members.len(), 1);
        assert!(
            first
                .packages
                .iter()
                .any(|package| package.name == "codedb_fixture")
        );
        assert!(
            first
                .targets
                .iter()
                .any(|target| target.name == "codedb_fixture" && target.kind == ["lib"])
        );
        assert!(first.dependencies.iter().any(|dependency| {
            dependency.package_id.contains("codedb_fixture")
                && dependency.name == "helper"
                && dependency.optional
        }));
        assert!(
            first
                .features
                .iter()
                .any(|feature| feature.feature == "default" && feature.enables == ["serde"])
        );
        assert!(first.sources.iter().any(|source| {
            source.source_name == "helper"
                && source.kind == CargoSourceKind::Path
                && source.observed_from == CargoSourceObservation::Dependency
        }));
        assert!(!first.resolve_nodes.is_empty());
    }

    // Defends: CDB020 must classify registry, git, and path provenance without network mutation.
    #[test]
    fn cargo_source_classifier_covers_registry_git_and_path() {
        assert_eq!(classify_cargo_source(None), CargoSourceKind::Path);
        assert_eq!(
            classify_cargo_source(Some(
                "registry+https://github.com/rust-lang/crates.io-index"
            )),
            CargoSourceKind::Registry
        );
        assert_eq!(
            classify_cargo_source(Some("sparse+https://index.crates.io/")),
            CargoSourceKind::Registry
        );
        assert_eq!(
            classify_cargo_source(Some(
                "git+https://github.com/example/repo?rev=abc#0123456789abcdef"
            )),
            CargoSourceKind::Git
        );
    }

    // Defends: CDB021 context rows must be keyed and deterministic across input ordering.
    #[test]
    fn context_rows_are_keyed_and_deterministic() {
        let first = build_context_rows(CargoContextInput {
            cargo_version: "cargo 1.92.0".to_string(),
            rustc_version: "rustc 1.92.0".to_string(),
            host_triple: "x86_64-unknown-linux-gnu".to_string(),
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            cfgs: vec![
                "target_os=\"linux\"".to_string(),
                "target_arch=\"x86_64\"".to_string(),
                "target_os=\"linux\"".to_string(),
            ],
            features: vec!["serde".to_string(), "default".to_string()],
            profile: "debug".to_string(),
            edition: "2024".to_string(),
            cargo_lock_hash: Some("lockhash".to_string()),
        });
        let second = build_context_rows(CargoContextInput {
            cargo_version: "cargo 1.92.0".to_string(),
            rustc_version: "rustc 1.92.0".to_string(),
            host_triple: "x86_64-unknown-linux-gnu".to_string(),
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            cfgs: vec![
                "target_arch=\"x86_64\"".to_string(),
                "target_os=\"linux\"".to_string(),
            ],
            features: vec!["default".to_string(), "serde".to_string()],
            profile: "debug".to_string(),
            edition: "2024".to_string(),
            cargo_lock_hash: Some("lockhash".to_string()),
        });

        assert_eq!(first, second);
        assert!(!first.context.context_id.is_empty());
        assert!(!first.context.toolchain_id.is_empty());
        assert_eq!(first.context.profile, "debug");
        assert_eq!(first.context.edition, "2024");
        assert_eq!(first.feature_set.features, ["default", "serde"]);
        assert_eq!(
            first
                .cfgs
                .iter()
                .map(|row| row.cfg.as_str())
                .collect::<Vec<_>>(),
            ["target_arch=\"x86_64\"", "target_os=\"linux\""]
        );
        assert_eq!(first.context.context_id, first.target.context_id);
        assert_eq!(first.context.context_id, first.profile.context_id);
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
                "codedb_cargo_metadata_fixture_{}_{}",
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
