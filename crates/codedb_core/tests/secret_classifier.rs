use std::fs;

use codedb_core::{
    SecretClassificationStatus, SecretEvidenceKind, SourceBlobMode, TextEncodingStatus,
    capture_source_metadata, classify_source_secret, source_policy_row,
};

struct SecretCase {
    name: &'static str,
    path: &'static str,
    content: &'static [u8],
    evidence: SecretEvidenceKind,
}

#[test]
fn detects_conservative_path_and_content_secret_matrix() {
    let cases = [
        SecretCase {
            name: "dotenv path",
            path: ".env.production",
            content: b"FEATURE_FLAG=true\n",
            evidence: SecretEvidenceKind::SensitivePath,
        },
        SecretCase {
            name: "aws credentials path",
            path: ".aws/credentials",
            content: b"[default]\nregion=us-east-1\n",
            evidence: SecretEvidenceKind::SensitivePath,
        },
        SecretCase {
            name: "git control path",
            path: ".git/config",
            content: b"[core]\nrepositoryformatversion = 0\n",
            evidence: SecretEvidenceKind::SensitivePath,
        },
        SecretCase {
            name: "ssh control path",
            path: ".ssh/config",
            content: b"Host example\n  HostName example.test\n",
            evidence: SecretEvidenceKind::SensitivePath,
        },
        SecretCase {
            name: "private key path",
            path: "keys/id_ed25519",
            content: b"opaque fixture\n",
            evidence: SecretEvidenceKind::SensitivePath,
        },
        SecretCase {
            name: "bearer token",
            path: "request.txt",
            content: b"Authorization: Bearer abcdefghijklmnopqrstuvwxyz012345\n",
            evidence: SecretEvidenceKind::BearerToken,
        },
        SecretCase {
            name: "jwt",
            path: "session.txt",
            content: b"token eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c\n",
            evidence: SecretEvidenceKind::JsonWebToken,
        },
        SecretCase {
            name: "aws access key",
            path: "aws.txt",
            content: b"AKIAIOSFODNN7EXAMPLE\n",
            evidence: SecretEvidenceKind::AwsAccessKeyId,
        },
        SecretCase {
            name: "aws secret access key",
            path: "aws.txt",
            content: b"aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY\n",
            evidence: SecretEvidenceKind::AwsSecretAccessKey,
        },
        SecretCase {
            name: "npm auth token",
            path: "npm-config.txt",
            content: b"//registry.npmjs.org/:_authToken=npm_example_token_value\n",
            evidence: SecretEvidenceKind::NpmAuthToken,
        },
        SecretCase {
            name: "postgres password",
            path: "config.toml",
            content: b"DATABASE_URL=postgresql://codedb:swordfish@db.example/codedb\n",
            evidence: SecretEvidenceKind::DatabaseUriCredentials,
        },
        SecretCase {
            name: "percent encoded postgres password",
            path: "config.toml",
            content:
                b"DATABASE_URL=postgres://codedb:p%40ss%3Aword@db.example/codedb?sslmode=require\n",
            evidence: SecretEvidenceKind::DatabaseUriCredentials,
        },
        SecretCase {
            name: "postgres query password",
            path: "config.toml",
            content:
                b"DATABASE_URL=postgresql://db.example/codedb?user=codedb&password=p%40ssword\n",
            evidence: SecretEvidenceKind::DatabaseUriCredentials,
        },
        SecretCase {
            name: "generic pem private key",
            path: "fixture.txt",
            content: b"-----BEGIN PRIVATE KEY-----\nredacted-fixture\n",
            evidence: SecretEvidenceKind::PrivateKeyHeader,
        },
        SecretCase {
            name: "pem private key",
            path: "fixture.txt",
            content: b"-----BEGIN RSA PRIVATE KEY-----\nredacted-fixture\n",
            evidence: SecretEvidenceKind::PrivateKeyHeader,
        },
        SecretCase {
            name: "openssh private key",
            path: "fixture.txt",
            content: b"-----BEGIN OPENSSH PRIVATE KEY-----\nredacted-fixture\n",
            evidence: SecretEvidenceKind::PrivateKeyHeader,
        },
        SecretCase {
            name: "generic shell credential",
            path: "config.sh",
            content: b"export CLIENT_SECRET='not-for-persistence'\n",
            evidence: SecretEvidenceKind::CredentialAssignment,
        },
        SecretCase {
            name: "generic json credential",
            path: "config.json",
            content: br#"{"password":"not-for-persistence"}"#,
            evidence: SecretEvidenceKind::CredentialAssignment,
        },
    ];

    for case in cases {
        let classification = classify_source_secret(case.path, case.content);
        assert_eq!(
            classification.status,
            SecretClassificationStatus::SecretDetected,
            "{} should be secret-detected: {classification:?}",
            case.name
        );
        assert!(
            classification.evidence.contains(&case.evidence),
            "{} should include {:?}: {classification:?}",
            case.name,
            case.evidence
        );
        assert!(classification.has_secret());
        assert!(!classification.raw_persistence_safe());
    }
}

#[test]
fn keeps_known_benign_text_clear_without_weakening_fail_closed_policy() {
    let cases = [
        ("src/lib.rs", "pub fn database_uri() {}\n"),
        (
            "docs/auth.md",
            "Bearer authentication requires a token supplied at runtime.\n",
        ),
        (
            "config.toml",
            "DATABASE_URL=postgresql://db.example/codedb?sslmode=require\n",
        ),
        ("public.pem", "-----BEGIN PUBLIC KEY-----\nfixture\n"),
        (
            "policy.toml",
            "password_policy = \"rotate-every-30-days\"\n",
        ),
    ];

    for (path, content) in cases {
        let classification = classify_source_secret(path, content.as_bytes());
        assert_eq!(
            classification.status,
            SecretClassificationStatus::NoSecretDetected,
            "{path} should remain clear: {classification:?}"
        );
        assert!(classification.evidence.is_empty());
        assert!(!classification.has_secret());
        assert!(classification.raw_persistence_safe());
    }
}

#[test]
fn invalid_utf8_and_binary_are_uncertain_and_never_raw_persistence_safe() {
    for (name, bytes) in [
        ("invalid utf8", &[0xff, 0xfe, b'a'][..]),
        ("binary", &[b'a', 0, b'b'][..]),
    ] {
        let classification = classify_source_secret("assets/data.bin", bytes);
        assert_eq!(
            classification.status,
            SecretClassificationStatus::Uncertain,
            "{name}: {classification:?}"
        );
        assert_eq!(
            classification.evidence,
            vec![SecretEvidenceKind::NonTextContent]
        );
        assert!(!classification.has_secret());
        assert!(!classification.raw_persistence_safe());
    }
}

#[test]
fn source_policy_preserves_metadata_only_fail_closed_handling_for_uncertainty() {
    let root = tempfile::tempdir().expect("create fixture");
    let path = root.path().join("opaque.dat");
    fs::write(&path, [0xff, 0xfe, b'a']).expect("write invalid utf8 fixture");

    let metadata = capture_source_metadata(root.path(), &path).expect("capture metadata");
    let policy = source_policy_row(&metadata);

    assert_eq!(metadata.encoding_status, TextEncodingStatus::InvalidUtf8);
    assert!(!metadata.has_secret_like_material);
    assert_eq!(metadata.default_mode, SourceBlobMode::MetadataOnly);
    assert!(!metadata.export_raw_by_default);
    assert_eq!(policy.mode, SourceBlobMode::MetadataOnly);
    assert!(!policy.raw_export_allowed);
    assert!(policy.reason.contains("uncertain"));
    assert!(!policy.reason.contains(char::REPLACEMENT_CHARACTER));
}
