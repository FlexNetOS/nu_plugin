#![forbid(unsafe_code)]

use serde::Deserialize;
use std::error::Error as StdError;
use std::fmt::{Display, Formatter};

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
    Parse { source: serde_json::Error },
}

impl Display for CargoMetadataError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse { source } => write!(f, "failed to parse cargo metadata JSON: {source}"),
        }
    }
}

impl StdError for CargoMetadataError {}

pub fn capture_cargo_metadata_json(
    metadata_json: &str,
) -> Result<CargoMetadataCapture, CargoMetadataError> {
    let metadata: Metadata = serde_json::from_str(metadata_json)
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
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Defends: CDB019 must use Cargo's structured metadata output and produce stable package/target rows.
    #[test]
    fn captured_metadata_json_projection_is_stable() {
        let json = r#"{
          "packages": [{
            "id": "path+file:///fixture#codedb_fixture@0.1.0",
            "name": "codedb_fixture",
            "version": "0.1.0",
            "source": null,
            "manifest_path": "/fixture/Cargo.toml",
            "edition": "2024",
            "targets": [{
              "name": "codedb_fixture",
              "kind": ["lib"],
              "crate_types": ["lib"],
              "src_path": "/fixture/src/lib.rs",
              "edition": "2024"
            }],
            "dependencies": [],
            "features": {"default": ["serde"], "serde": []}
          }],
          "workspace_members": ["path+file:///fixture#codedb_fixture@0.1.0"],
          "workspace_root": "/fixture",
          "target_directory": "/fixture/target",
          "resolve": {"nodes": [{
            "id": "path+file:///fixture#codedb_fixture@0.1.0",
            "dependencies": [],
            "features": ["default", "serde"]
          }]}
        }"#;
        let first = capture_cargo_metadata_json(json).expect("first projection");
        let second = capture_cargo_metadata_json(json).expect("second projection");
        assert_eq!(first, second);
        assert_eq!(first.packages.len(), 1);
        assert_eq!(first.packages[0].name, "codedb_fixture");
        assert_eq!(first.targets[0].kind, ["lib"]);
        assert_eq!(first.features.len(), 2);
        assert_eq!(first.resolve_nodes.len(), 1);
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
