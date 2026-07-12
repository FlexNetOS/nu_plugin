use std::fs::{self, File};
use std::io::Write;

use codedb_core::capture_policy::{
    CapturePolicyError, ExternalPolicyLoadStage, RawPersistenceAuthorization,
    RawPersistenceDisposition, RawPersistenceReason, SourceClass, authorize_raw_persistence,
    load_external_policy, load_external_policy_with_hook,
};
use sha2::Digest;

const REPOSITORY_BINDING: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn external_policy_document(repository_binding: &str, allow: &str) -> String {
    format!(
        "version=codedb.raw-persistence-policy.v1\n\
         policy_id=operator-reviewed-source\n\
         authority=operator:local-user\n\
         repository_binding={repository_binding}\n\
         allow={allow}\n"
    )
}

#[test]
fn default_is_deny_even_when_classifier_finds_no_secret() {
    let decision = authorize_raw_persistence(
        "src/lib.rs",
        b"pub fn safe_source() {}\n",
        REPOSITORY_BINDING,
        None,
    );

    assert_eq!(decision.source_class, SourceClass::SourceCode);
    assert_eq!(
        decision.disposition,
        RawPersistenceDisposition::MetadataOnly
    );
    assert_eq!(decision.reason, RawPersistenceReason::MissingAuthorization);
    assert!(!decision.raw_persistence_allowed());
    assert_eq!(decision.policy.repository_binding, REPOSITORY_BINDING);
    assert_eq!(decision.policy.policy_digest.len(), "sha256:".len() + 64);
    assert_eq!(decision.policy.binding_digest.len(), "sha256:".len() + 64);
}

#[test]
fn built_in_policy_authorizes_only_core_owned_safe_source_classes() {
    let authorization = RawPersistenceAuthorization::BuiltInSafeSourceClasses;

    for path in ["src/lib.rs", "src/worker.py", "docs/security.md"] {
        let decision = authorize_raw_persistence(
            path,
            b"documented, reviewed source bytes\n",
            REPOSITORY_BINDING,
            Some(&authorization),
        );
        assert_eq!(
            decision.disposition,
            RawPersistenceDisposition::PersistRaw,
            "{path}: {decision:?}"
        );
        assert_eq!(
            decision.reason,
            RawPersistenceReason::AuthorizedSafeSourceClass
        );
        assert!(decision.raw_persistence_allowed());
    }

    for path in [
        "Cargo.toml",
        "config/settings.json",
        ".env.production",
        "assets/opaque.txt",
    ] {
        let decision = authorize_raw_persistence(
            path,
            b"FEATURE_FLAG=true\n",
            REPOSITORY_BINDING,
            Some(&authorization),
        );
        assert_eq!(
            decision.disposition,
            RawPersistenceDisposition::MetadataOnly,
            "{path}: {decision:?}"
        );
        assert!(!decision.raw_persistence_allowed());
    }
}

#[test]
fn classifier_is_a_guard_and_never_an_authorization() {
    let authorization = RawPersistenceAuthorization::BuiltInSafeSourceClasses;
    let decision = authorize_raw_persistence(
        "src/lib.rs",
        b"const CLIENT_SECRET: &str = \"not-for-persistence\";\n",
        REPOSITORY_BINDING,
        Some(&authorization),
    );

    assert_eq!(decision.source_class, SourceClass::SourceCode);
    assert_eq!(
        decision.disposition,
        RawPersistenceDisposition::MetadataOnly
    );
    assert_eq!(
        decision.reason,
        RawPersistenceReason::ClassifierSecretDetected
    );
    assert!(!decision.raw_persistence_allowed());
}

#[test]
fn unsafe_or_escaping_paths_are_unknown_and_metadata_only() {
    let authorization = RawPersistenceAuthorization::BuiltInSafeSourceClasses;

    for path in [
        "",
        "../src/lib.rs",
        "/absolute/src/lib.rs",
        "src/../../lib.rs",
    ] {
        let decision = authorize_raw_persistence(
            path,
            b"pub fn path_must_stay_contained() {}\n",
            REPOSITORY_BINDING,
            Some(&authorization),
        );
        assert_eq!(decision.source_class, SourceClass::Unknown, "{path}");
        assert_eq!(
            decision.disposition,
            RawPersistenceDisposition::MetadataOnly,
            "{path}: {decision:?}"
        );
        assert_eq!(decision.reason, RawPersistenceReason::HardDeniedSourceClass);
    }
}

#[test]
fn external_policy_is_digest_bound_and_outside_the_repository() {
    let repository = tempfile::tempdir().expect("repository");
    let policy_home = tempfile::tempdir().expect("external policy home");
    let policy_path = policy_home.path().join("capture.policy");
    fs::write(
        &policy_path,
        external_policy_document(REPOSITORY_BINDING, "source-code,documentation"),
    )
    .expect("write external policy");

    let binding =
        load_external_policy(repository.path(), &policy_path, REPOSITORY_BINDING).expect("binding");
    assert_eq!(binding.policy_id(), "operator-reviewed-source");
    assert_eq!(binding.authority(), "operator:local-user");
    assert_eq!(binding.repository_binding(), REPOSITORY_BINDING);
    assert_eq!(binding.policy_digest().len(), "sha256:".len() + 64);
    assert!(binding.policy_path().is_absolute());

    let authorization = RawPersistenceAuthorization::External(binding.clone());
    let decision = authorize_raw_persistence(
        "src/lib.rs",
        b"pub fn externally_reviewed() {}\n",
        REPOSITORY_BINDING,
        Some(&authorization),
    );
    assert_eq!(decision.disposition, RawPersistenceDisposition::PersistRaw);
    assert_eq!(
        decision.reason,
        RawPersistenceReason::AuthorizedExternalPolicy
    );
    assert_eq!(decision.policy.policy_digest, binding.policy_digest());
    assert_eq!(
        decision.policy.external_policy_path.as_deref(),
        Some(binding.policy_path())
    );

    fs::write(
        &policy_path,
        external_policy_document(REPOSITORY_BINDING, "documentation"),
    )
    .expect("change external policy");
    let changed =
        load_external_policy(repository.path(), &policy_path, REPOSITORY_BINDING).expect("changed");
    assert_ne!(binding.policy_digest(), changed.policy_digest());
}

#[test]
fn repository_controlled_policy_cannot_self_authorize_raw_persistence() {
    let repository = tempfile::tempdir().expect("repository");
    let policy_path = repository.path().join(".codedb/capture.policy");
    fs::create_dir_all(policy_path.parent().expect("policy parent")).expect("policy parent");
    fs::write(
        &policy_path,
        external_policy_document(REPOSITORY_BINDING, "source-code"),
    )
    .expect("write repository policy");

    let error = load_external_policy(repository.path(), &policy_path, REPOSITORY_BINDING)
        .expect_err("repository-owned policy must be rejected");
    assert!(matches!(
        error,
        CapturePolicyError::RepositoryControlledPolicy { .. }
    ));
}

#[test]
fn external_policy_cannot_widen_sensitive_config_or_unknown_classes() {
    let repository = tempfile::tempdir().expect("repository");
    let policy_home = tempfile::tempdir().expect("external policy home");
    let invalid_path = policy_home.path().join("invalid.policy");
    fs::write(
        &invalid_path,
        external_policy_document(
            REPOSITORY_BINDING,
            "source-code,configuration,sensitive,unknown",
        ),
    )
    .expect("write invalid policy");

    let error = load_external_policy(repository.path(), &invalid_path, REPOSITORY_BINDING)
        .expect_err("hard-denied classes cannot be authorized");
    assert!(matches!(
        error,
        CapturePolicyError::HardDeniedClassInPolicy { .. }
    ));

    let valid_path = policy_home.path().join("valid.policy");
    fs::write(
        &valid_path,
        external_policy_document(REPOSITORY_BINDING, "source-code,documentation"),
    )
    .expect("write valid policy");
    let binding =
        load_external_policy(repository.path(), &valid_path, REPOSITORY_BINDING).expect("binding");
    let authorization = RawPersistenceAuthorization::External(binding);

    for path in ["config.toml", ".ssh/config", "assets/unclassified.data"] {
        let decision = authorize_raw_persistence(
            path,
            b"benign fixture content\n",
            REPOSITORY_BINDING,
            Some(&authorization),
        );
        assert_eq!(
            decision.disposition,
            RawPersistenceDisposition::MetadataOnly,
            "{path}: {decision:?}"
        );
        assert!(matches!(
            decision.reason,
            RawPersistenceReason::HardDeniedSourceClass
                | RawPersistenceReason::ClassifierSecretDetected
        ));
    }
}

#[test]
fn policy_provenance_never_echoes_an_opaque_caller_binding() {
    let opaque = "repository-binding-with-secret=do-not-echo";
    let decision = authorize_raw_persistence("src/lib.rs", b"pub fn safe() {}\n", opaque, None);

    assert!(
        decision
            .policy
            .repository_binding
            .starts_with("opaque-sha256:")
    );
    assert!(!decision.policy.repository_binding.contains("do-not-echo"));
    assert!(!format!("{decision:?}").contains("do-not-echo"));
}

#[test]
fn external_policy_rejects_non_public_provenance_identifiers() {
    let repository = tempfile::tempdir().expect("repository");
    let policy_home = tempfile::tempdir().expect("external policy home");
    let policy_path = policy_home.path().join("capture.policy");
    let document = external_policy_document(REPOSITORY_BINDING, "source-code")
        .replace("operator:local-user", "operator:token=do-not-echo");
    fs::write(&policy_path, document).expect("write external policy");

    let error = load_external_policy(repository.path(), &policy_path, REPOSITORY_BINDING)
        .expect_err("non-public identifier must fail closed");
    assert!(matches!(
        error,
        CapturePolicyError::InvalidPolicyDocument { .. }
    ));
    assert!(!error.to_string().contains("do-not-echo"));
}

#[cfg(target_os = "linux")]
#[test]
fn external_policy_final_replacement_cannot_redirect_the_held_read() {
    use std::os::unix::fs::symlink;

    let repository = tempfile::tempdir().expect("repository");
    let policy_home = tempfile::tempdir().expect("external policy home");
    let policy_path = policy_home.path().join("capture.policy");
    let held_path = policy_home.path().join("held.policy");
    let repository_policy = repository.path().join("attacker.policy");
    let original = external_policy_document(REPOSITORY_BINDING, "source-code,documentation");
    fs::write(&policy_path, &original).expect("write original policy");
    fs::write(
        &repository_policy,
        external_policy_document(REPOSITORY_BINDING, "source-code"),
    )
    .expect("write repository-controlled replacement");

    let binding = load_external_policy_with_hook(
        repository.path(),
        &policy_path,
        REPOSITORY_BINDING,
        |stage| {
            if stage == ExternalPolicyLoadStage::PolicyOpened {
                fs::rename(&policy_path, &held_path).expect("rename opened policy");
                symlink(&repository_policy, &policy_path)
                    .expect("replace final component with repository symlink");
            }
        },
    )
    .expect("held descriptor must retain the original policy");

    assert_eq!(
        binding.policy_digest(),
        format!("sha256:{:x}", sha2::Sha256::digest(original.as_bytes()))
    );
    assert!(binding.allows(SourceClass::SourceCode));
    assert!(binding.allows(SourceClass::Documentation));
}

#[cfg(target_os = "linux")]
#[test]
fn external_policy_ancestor_replacement_cannot_redirect_the_open() {
    use std::os::unix::fs::symlink;

    let repository = tempfile::tempdir().expect("repository");
    let policy_home = tempfile::tempdir().expect("external policy home");
    let active = policy_home.path().join("active");
    let held = policy_home.path().join("held");
    fs::create_dir(&active).expect("create policy ancestor");
    let policy_path = active.join("capture.policy");
    fs::write(
        &policy_path,
        external_policy_document(REPOSITORY_BINDING, "source-code,documentation"),
    )
    .expect("write external policy");
    fs::write(
        repository.path().join("capture.policy"),
        external_policy_document(REPOSITORY_BINDING, "source-code"),
    )
    .expect("write repository policy");

    let binding = load_external_policy_with_hook(
        repository.path(),
        &policy_path,
        REPOSITORY_BINDING,
        |stage| {
            if stage == ExternalPolicyLoadStage::RootsOpened {
                fs::rename(&active, &held).expect("rename held policy ancestor");
                symlink(repository.path(), &active).expect("replace ancestor with repository link");
            }
        },
    )
    .expect("open must stay relative to the held policy directory");

    assert!(binding.allows(SourceClass::Documentation));
    assert!(binding.policy_path().starts_with(&held));
}

#[cfg(target_os = "linux")]
#[test]
fn external_policy_repository_root_replacement_does_not_change_held_containment() {
    use std::os::unix::fs::symlink;

    let sandbox = tempfile::tempdir().expect("sandbox");
    let repository = sandbox.path().join("repository");
    let held_repository = sandbox.path().join("repository-held");
    let policy_home = sandbox.path().join("policy-home");
    fs::create_dir(&repository).expect("repository");
    fs::create_dir(&policy_home).expect("policy home");
    let policy_path = policy_home.join("capture.policy");
    fs::write(
        &policy_path,
        external_policy_document(REPOSITORY_BINDING, "source-code"),
    )
    .expect("write policy");

    let binding =
        load_external_policy_with_hook(&repository, &policy_path, REPOSITORY_BINDING, |stage| {
            if stage == ExternalPolicyLoadStage::RootsOpened {
                fs::rename(&repository, &held_repository).expect("rename held repository");
                symlink(&policy_home, &repository).expect("replace repository root with symlink");
            }
        })
        .expect("containment proof must use the held repository descriptor");

    assert!(binding.allows(SourceClass::SourceCode));
}

#[cfg(target_os = "linux")]
#[test]
fn external_policy_moved_under_held_repository_before_open_is_rejected() {
    let sandbox = tempfile::tempdir().expect("sandbox");
    let repository = sandbox.path().join("repository");
    let policy_home = sandbox.path().join("policy-home");
    fs::create_dir(&repository).expect("repository");
    fs::create_dir(&policy_home).expect("policy home");
    let policy_path = policy_home.join("capture.policy");
    fs::write(
        &policy_path,
        external_policy_document(REPOSITORY_BINDING, "source-code"),
    )
    .expect("write policy");

    let error =
        load_external_policy_with_hook(&repository, &policy_path, REPOSITORY_BINDING, |stage| {
            if stage == ExternalPolicyLoadStage::RootsOpened {
                fs::rename(&policy_home, repository.join("policy-home"))
                    .expect("move held policy root under repository");
            }
        })
        .expect_err("post-open held paths must reveal repository containment");

    assert!(matches!(
        error,
        CapturePolicyError::RepositoryControlledPolicy { .. }
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn external_policy_rejects_symlinks_special_files_and_oversize_files() {
    use std::os::unix::fs::symlink;
    use std::os::unix::net::UnixListener;

    let repository = tempfile::tempdir().expect("repository");
    let policy_home = tempfile::tempdir().expect("external policy home");
    let valid = policy_home.path().join("valid.policy");
    fs::write(
        &valid,
        external_policy_document(REPOSITORY_BINDING, "source-code"),
    )
    .expect("write valid policy");

    let final_symlink = policy_home.path().join("final-symlink.policy");
    symlink(&valid, &final_symlink).expect("create final symlink");
    assert!(matches!(
        load_external_policy(repository.path(), &final_symlink, REPOSITORY_BINDING),
        Err(CapturePolicyError::PolicyPathIsSymlink { .. })
    ));

    let symlinked_parent = policy_home.path().join("symlinked-parent");
    symlink(policy_home.path(), &symlinked_parent).expect("create ancestor symlink");
    assert!(matches!(
        load_external_policy(
            repository.path(),
            symlinked_parent.join("valid.policy"),
            REPOSITORY_BINDING
        ),
        Err(CapturePolicyError::PolicyPathIsSymlink { .. })
    ));

    let socket_path = policy_home.path().join("special.policy");
    let _socket = UnixListener::bind(&socket_path).expect("create socket");
    assert!(matches!(
        load_external_policy(repository.path(), &socket_path, REPOSITORY_BINDING),
        Err(CapturePolicyError::PolicyPathIsNotRegularFile { .. })
    ));

    let oversize_path = policy_home.path().join("oversize.policy");
    let mut oversize = File::create(&oversize_path).expect("create oversize policy");
    oversize
        .write_all(&vec![b'x'; 64 * 1024 + 1])
        .expect("write oversize policy");
    assert!(matches!(
        load_external_policy(repository.path(), &oversize_path, REPOSITORY_BINDING),
        Err(CapturePolicyError::PolicyDocumentTooLarge {
            bytes: 65_537,
            maximum: 65_536,
            ..
        })
    ));
}

#[test]
fn metadata_only_decision_preserves_hash_bound_exact_source_reproduction() {
    let original = b"DATABASE_URL=postgresql://db.example/codedb\n";
    let decision = authorize_raw_persistence(
        "config.toml",
        original,
        REPOSITORY_BINDING,
        Some(&RawPersistenceAuthorization::BuiltInSafeSourceClasses),
    );

    assert_eq!(
        decision.disposition,
        RawPersistenceDisposition::MetadataOnly
    );
    assert_eq!(decision.exact_source.byte_len, original.len() as u64);
    assert_eq!(decision.exact_source.sha256.len(), 64);

    let verified = decision
        .exact_source
        .verify(original)
        .expect("operator-supplied exact source verifies");
    assert_eq!(verified.bytes(), original);
    assert_eq!(verified.sha256(), decision.exact_source.sha256);

    let error = decision
        .exact_source
        .verify(b"DATABASE_URL=postgresql://different.example/codedb\n")
        .expect_err("changed source must not reproduce");
    assert!(!error.to_string().contains("different.example"));
}

#[test]
fn exact_source_verification_debug_output_never_contains_source_bytes() {
    let original = b"CLIENT_SECRET=operator-supplied-only\n";
    let decision = authorize_raw_persistence(".env", original, REPOSITORY_BINDING, None);
    let verified = decision
        .exact_source
        .verify(original)
        .expect("exact source verifies");

    let debug = format!("{verified:?}");
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("operator-supplied-only"));
}
