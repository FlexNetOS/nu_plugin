//! ARCHBP-039: the single-owner redb service and atomic mmap live
//! projection.
//!
//! One supervised service holds the only writable redb handle for its root.
//! Mutations and queries travel over a versioned, token-authenticated
//! Unix-domain socket; every committed mutation advances a monotonic
//! `local_seq`, publishes a checksummed read-only projection generation into
//! the inactive slot (write → fsync → atomic pointer flip), and appends an
//! ordered commit notification to an append-only spool. Readers mmap the
//! active slot, verify its checksum, and fall back to the previous
//! generation — visibly degraded, never silent — if the active bytes are
//! corrupt. No second opener, no HTTP surface, no PostgreSQL polling.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Versioned UDS request protocol.
pub const PROTOCOL_VERSION: &str = "flexnetos.redb-owner.v0";
/// Versioned on-disk projection format.
pub const PROJECTION_FORMAT_VERSION: &str = "flexnetos.redb-owner.projection.v0";

#[derive(Debug)]
pub enum OwnerError {
    AlreadyOwned(String),
    Rejected(String),
    Corrupt(String),
    Io(std::io::Error),
    Internal(String),
}

impl std::fmt::Display for OwnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyOwned(m) => write!(f, "root already owned: {m}"),
            Self::Rejected(m) => write!(f, "request rejected: {m}"),
            Self::Corrupt(m) => write!(f, "projection corrupt: {m}"),
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Internal(m) => write!(f, "internal error: {m}"),
        }
    }
}

impl std::error::Error for OwnerError {}

impl From<std::io::Error> for OwnerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// The running owner service. Dropping it shuts the service down.
pub struct OwnerService {
    _root: PathBuf,
}

impl std::fmt::Debug for OwnerService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnerService").finish_non_exhaustive()
    }
}

impl OwnerService {
    pub fn start(root: impl AsRef<Path>) -> Result<Self, OwnerError> {
        let _ = root.as_ref();
        Err(OwnerError::Internal("OwnerService::start is not implemented".into()))
    }

    /// Test failpoint: the next publication dies after the redb commit and
    /// before the projection flip.
    pub fn inject_publish_crash(&self) {}
}

/// A UDS client speaking the versioned authenticated protocol.
pub struct OwnerClient {
    _root: PathBuf,
}

impl OwnerClient {
    pub fn connect(root: impl AsRef<Path>) -> Result<Self, OwnerError> {
        let _ = root.as_ref();
        Err(OwnerError::Internal("OwnerClient::connect is not implemented".into()))
    }

    pub fn override_token(&mut self, _token: &str) {}

    pub fn override_protocol_version(&mut self, _version: &str) {}

    pub fn put(&mut self, key: &str, value: &str) -> Result<u64, OwnerError> {
        let _ = (key, value);
        Err(OwnerError::Internal("put is not implemented".into()))
    }

    pub fn get(&mut self, key: &str) -> Result<Option<String>, OwnerError> {
        let _ = key;
        Err(OwnerError::Internal("get is not implemented".into()))
    }
}

/// One decoded projection generation.
#[derive(Debug, Clone)]
pub struct Projection {
    pub local_seq: u64,
    pub slot: String,
    pub checksum: String,
    pub degraded: bool,
    pub entries: BTreeMap<String, String>,
}

/// Reads the active (or fallback) projection generation via mmap.
pub struct ProjectionReader;

impl ProjectionReader {
    pub fn read(root: impl AsRef<Path>) -> Result<Projection, OwnerError> {
        let _ = root.as_ref();
        Err(OwnerError::Internal("ProjectionReader::read is not implemented".into()))
    }
}

/// One ordered commit notification.
#[derive(Debug, Clone)]
pub struct CommitEvent {
    pub seq: u64,
    pub slot: String,
    pub checksum: String,
}

/// Read commit notifications with seq strictly greater than `after_seq`.
pub fn read_events(
    root: impl AsRef<Path>,
    after_seq: u64,
) -> Result<Vec<CommitEvent>, OwnerError> {
    let _ = (root.as_ref(), after_seq);
    Err(OwnerError::Internal("read_events is not implemented".into()))
}
