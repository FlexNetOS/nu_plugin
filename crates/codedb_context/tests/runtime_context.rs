use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use codedb_context::{
    CargoContextRequest, CommandOutput, CommandRunner, ContextError, capture_context_with_runner,
};

#[derive(Default)]
struct FakeRunner {
    invocations: Mutex<Vec<(String, Vec<String>, PathBuf)>>,
}

impl CommandRunner for FakeRunner {
    fn output(
        &self,
        program: &str,
        args: &[String],
        current_dir: &Path,
    ) -> Result<CommandOutput, ContextError> {
        self.invocations.lock().unwrap().push((
            program.to_string(),
            args.to_vec(),
            current_dir.to_path_buf(),
        ));
        match (program, args) {
            ("cargo", [arg]) if arg == "--version" => {
                Ok(CommandOutput::success("cargo 1.93.1 (fixture)\n", ""))
            }
            ("rustc", [arg]) if arg == "-vV" => Ok(CommandOutput::success(
                "rustc 1.93.1 (fixture)\nbinary: rustc\ncommit-hash: fixture\nhost: x86_64-unknown-linux-gnu\nrelease: 1.93.1\nLLVM version: 20.1.0\n",
                "",
            )),
            ("rustc", [print, cfg, target_flag, target])
                if print == "--print"
                    && cfg == "cfg"
                    && target_flag == "--target"
                    && target == "aarch64-unknown-linux-gnu" =>
            {
                Ok(CommandOutput::success(
                    "target_arch=\"aarch64\"\ntarget_os=\"linux\"\nunix\n",
                    "",
                ))
            }
            ("cargo", args) if args.first().is_some_and(|arg| arg == "metadata") => {
                Ok(CommandOutput::success(
                    r#"{
  "packages": [{"id":"path+file:///fixture#app@0.1.0","name":"app","version":"0.1.0"}],
  "workspace_members": ["path+file:///fixture#app@0.1.0"],
  "workspace_root": "/fixture",
  "target_directory": "/fixture/target",
  "resolve": {
    "nodes": [{
      "id":"path+file:///fixture#app@0.1.0",
      "dependencies":[],
      "features":["default","serde"]
    }]
  },
  "version": 1
}"#,
                    "",
                ))
            }
            _ => panic!("unexpected command: {program} {args:?}"),
        }
    }
}

#[test]
fn captures_locked_target_feature_and_toolchain_context() {
    let fixture = Fixture::new();
    let runner = FakeRunner::default();
    let capture = capture_context_with_runner(
        &CargoContextRequest {
            manifest_path: fixture.root.join("Cargo.toml"),
            target_triple: "aarch64-unknown-linux-gnu".to_string(),
            features: vec!["serde".to_string()],
            all_features: false,
            no_default_features: false,
            profile: "release".to_string(),
        },
        &runner,
    )
    .expect("context capture");

    assert_eq!(capture.cargo_version, "cargo 1.93.1 (fixture)");
    assert_eq!(capture.rustc_version, "rustc 1.93.1 (fixture)");
    assert_eq!(capture.host_triple, "x86_64-unknown-linux-gnu");
    assert_eq!(capture.target_triple, "aarch64-unknown-linux-gnu");
    assert_eq!(
        capture.target_cfgs,
        [
            "target_arch=\"aarch64\"".to_string(),
            "target_os=\"linux\"".to_string(),
            "unix".to_string(),
        ]
    );
    assert_eq!(capture.requested_features, ["serde"]);
    assert_eq!(
        capture.resolved_features,
        BTreeMap::from([(
            "path+file:///fixture#app@0.1.0".to_string(),
            vec!["default".to_string(), "serde".to_string()],
        )])
    );
    assert_eq!(capture.profile, "release");
    assert_eq!(capture.cargo_lock_sha256.len(), 64);
    assert_eq!(capture.context_id.len(), 64);

    let invocations = runner.invocations.lock().unwrap();
    let metadata = invocations
        .iter()
        .find(|(program, args, _)| {
            program == "cargo" && args.first().is_some_and(|arg| arg == "metadata")
        })
        .expect("cargo metadata invocation");
    assert!(metadata.1.windows(1).any(|arg| arg == ["--locked"]));
    assert!(
        metadata
            .1
            .windows(2)
            .any(|args| { args == ["--filter-platform", "aarch64-unknown-linux-gnu"] })
    );
    assert!(
        metadata
            .1
            .windows(2)
            .any(|args| args == ["--features", "serde"])
    );
}

#[test]
fn lockfile_and_target_are_part_of_context_identity() {
    let fixture = Fixture::new();
    let runner = FakeRunner::default();
    let request = CargoContextRequest {
        manifest_path: fixture.root.join("Cargo.toml"),
        target_triple: "aarch64-unknown-linux-gnu".to_string(),
        features: vec!["serde".to_string()],
        all_features: false,
        no_default_features: false,
        profile: "release".to_string(),
    };
    let first = capture_context_with_runner(&request, &runner).unwrap();
    fs::write(
        fixture.root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.1.1\"\n",
    )
    .unwrap();
    let second = capture_context_with_runner(&request, &runner).unwrap();
    assert_ne!(first.cargo_lock_sha256, second.cargo_lock_sha256);
    assert_ne!(first.context_id, second.context_id);
}

#[test]
fn missing_lockfile_blocks_capture_before_cargo_resolution() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.root.join("Cargo.lock")).unwrap();
    let error = capture_context_with_runner(
        &CargoContextRequest {
            manifest_path: fixture.root.join("Cargo.toml"),
            target_triple: "aarch64-unknown-linux-gnu".to_string(),
            features: vec![],
            all_features: false,
            no_default_features: false,
            profile: "debug".to_string(),
        },
        &FakeRunner::default(),
    )
    .unwrap_err();
    assert!(matches!(error, ContextError::MissingLockfile { .. }));
}

#[test]
fn cargo_feature_modes_are_forwarded_and_change_context_identity() {
    let fixture = Fixture::new();
    let runner = FakeRunner::default();
    let base = CargoContextRequest {
        manifest_path: fixture.root.join("Cargo.toml"),
        target_triple: "aarch64-unknown-linux-gnu".to_string(),
        features: vec![],
        all_features: false,
        no_default_features: false,
        profile: "debug".to_string(),
    };
    let default_capture = capture_context_with_runner(&base, &runner).unwrap();

    let mut all_request = base.clone();
    all_request.all_features = true;
    let all_capture = capture_context_with_runner(&all_request, &runner).unwrap();

    let mut no_default_request = base;
    no_default_request.no_default_features = true;
    let no_default_capture = capture_context_with_runner(&no_default_request, &runner).unwrap();

    assert_ne!(default_capture.context_id, all_capture.context_id);
    assert_ne!(default_capture.context_id, no_default_capture.context_id);
    assert_ne!(all_capture.context_id, no_default_capture.context_id);

    let invocations = runner.invocations.lock().unwrap();
    let metadata_args = invocations
        .iter()
        .filter(|(program, args, _)| {
            program == "cargo" && args.first().is_some_and(|arg| arg == "metadata")
        })
        .map(|(_, args, _)| args)
        .collect::<Vec<_>>();
    assert!(
        metadata_args
            .iter()
            .any(|args| args.iter().any(|arg| arg == "--all-features"))
    );
    assert!(
        metadata_args
            .iter()
            .any(|args| args.iter().any(|arg| arg == "--no-default-features"))
    );
}

#[test]
fn workspace_member_uses_nearest_ancestor_lockfile() {
    let fixture = Fixture::new();
    let member = fixture.root.join("crates/member");
    fs::create_dir_all(member.join("src")).unwrap();
    fs::write(
        member.join("Cargo.toml"),
        "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(member.join("src/lib.rs"), "pub fn member() {}\n").unwrap();

    let capture = capture_context_with_runner(
        &CargoContextRequest {
            manifest_path: member.join("Cargo.toml"),
            target_triple: "aarch64-unknown-linux-gnu".to_string(),
            features: vec![],
            all_features: false,
            no_default_features: false,
            profile: "debug".to_string(),
        },
        &FakeRunner::default(),
    )
    .expect("ancestor Cargo.lock should be accepted");
    assert_eq!(capture.cargo_lock_sha256.len(), 64);
}

#[test]
fn fixture_roots_are_reserved_and_collision_free() {
    let handles: Vec<_> = (0..8)
        .map(|_| std::thread::spawn(|| (0..16).map(|_| Fixture::new()).collect::<Vec<_>>()))
        .collect();
    let fixtures: Vec<Fixture> = handles
        .into_iter()
        .flat_map(|handle| handle.join().expect("fixture thread"))
        .collect();
    let roots = fixtures
        .iter()
        .map(|fixture| fixture.root.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        roots.len(),
        fixtures.len(),
        "fixture roots must be unique so one Drop can never delete a live sibling"
    );
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "codedb_context_fixture_{}_{}",
            std::process::id(),
            sequence
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(root.join("Cargo.lock"), "version = 4\n").unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn app() {}\n").unwrap();
        Self { root }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
