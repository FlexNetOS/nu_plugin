//! Backend-neutral parsing and safe display of CodeDB store specifications.
//!
//! Parsing is intentionally side-effect free: callers choose when and how to
//! open a returned store. This lets command surfaces reject unsupported URI
//! schemes before any filesystem or database operation occurs.

use std::error::Error as StdError;
use std::fmt::{Debug, Display, Formatter};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreBackend {
    Redb,
    PostgreSql,
}

#[derive(Clone, PartialEq, Eq)]
enum StoreLocation {
    RedbPath(PathBuf),
    ConnectionString(String),
}

/// A parsed CodeDB store target.
///
/// The original PostgreSQL connection string remains available only to the
/// caller that must connect. [`Display`] and [`Debug`] always use the redacted
/// representation.
#[derive(Clone, PartialEq, Eq)]
pub struct StoreSpec {
    backend: StoreBackend,
    location: StoreLocation,
}

impl StoreSpec {
    /// Parse a store selector without opening it or creating filesystem paths.
    ///
    /// Plain paths and `redb://` URLs select the redb backend. `postgres://`
    /// and `postgresql://` URLs select PostgreSQL. The bare `pg` selector is
    /// allowed only when the caller supplies an explicit PostgreSQL DSN through
    /// `external_postgres_dsn`.
    pub fn parse(
        input: impl AsRef<str>,
        external_postgres_dsn: Option<&str>,
    ) -> Result<Self, StoreSpecError> {
        let input = input.as_ref();
        if input.is_empty() {
            return Err(StoreSpecError::Empty);
        }

        if input == "pg" {
            let dsn = external_postgres_dsn
                .filter(|dsn| !dsn.trim().is_empty())
                .ok_or(StoreSpecError::MissingExternalPostgresDsn)?;
            return Self::parse_postgres_dsn(dsn);
        }

        let Some((scheme, remainder)) = split_uri_scheme(input) else {
            return Ok(Self {
                backend: StoreBackend::Redb,
                location: StoreLocation::RedbPath(PathBuf::from(input)),
            });
        };

        match scheme.to_ascii_lowercase().as_str() {
            "redb" => {
                let path = remainder
                    .strip_prefix("//")
                    .filter(|path| !path.is_empty())
                    .ok_or(StoreSpecError::MalformedRedbUrl)?;
                Ok(Self {
                    backend: StoreBackend::Redb,
                    location: StoreLocation::RedbPath(PathBuf::from(path)),
                })
            }
            "postgres" | "postgresql" => Self::parse_postgres_dsn(input),
            _ => Err(StoreSpecError::UnsupportedUriScheme),
        }
    }

    pub const fn backend(&self) -> StoreBackend {
        self.backend
    }

    pub fn redb_path(&self) -> Option<&Path> {
        match &self.location {
            StoreLocation::RedbPath(path) => Some(path),
            StoreLocation::ConnectionString(_) => None,
        }
    }

    /// Return the unredacted PostgreSQL DSN for the connection owner.
    pub fn connection_string(&self) -> Option<&str> {
        match &self.location {
            StoreLocation::RedbPath(_) => None,
            StoreLocation::ConnectionString(dsn) => Some(dsn),
        }
    }

    /// Return a representation suitable for user-facing output and logs.
    pub fn redacted(&self) -> String {
        match &self.location {
            StoreLocation::RedbPath(path) => path.display().to_string(),
            StoreLocation::ConnectionString(dsn) => redact_postgres_dsn(dsn),
        }
    }

    fn parse_postgres_dsn(dsn: &str) -> Result<Self, StoreSpecError> {
        let Some((scheme, remainder)) = split_uri_scheme(dsn) else {
            return Err(StoreSpecError::InvalidPostgresDsn);
        };
        if !matches!(
            scheme.to_ascii_lowercase().as_str(),
            "postgres" | "postgresql"
        ) || !remainder.starts_with("//")
            || remainder == "//"
        {
            return Err(StoreSpecError::InvalidPostgresDsn);
        }

        Ok(Self {
            backend: StoreBackend::PostgreSql,
            location: StoreLocation::ConnectionString(dsn.to_owned()),
        })
    }
}

impl Display for StoreSpec {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.redacted())
    }
}

impl Debug for StoreSpec {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreSpec")
            .field("backend", &self.backend)
            .field("display", &self.redacted())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreSpecError {
    Empty,
    MissingExternalPostgresDsn,
    InvalidPostgresDsn,
    MalformedRedbUrl,
    UnsupportedUriScheme,
}

impl Display for StoreSpecError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("store specification is empty"),
            Self::MissingExternalPostgresDsn => {
                f.write_str("bare pg requires an explicit PostgreSQL DSN from the caller")
            }
            Self::InvalidPostgresDsn => {
                f.write_str("PostgreSQL store requires a postgres:// or postgresql:// URL")
            }
            Self::MalformedRedbUrl => f.write_str("redb URL requires a filesystem path"),
            Self::UnsupportedUriScheme => f.write_str("unsupported store URI scheme"),
        }
    }
}

impl StdError for StoreSpecError {}

fn split_uri_scheme(input: &str) -> Option<(&str, &str)> {
    let separator = input.find(':')?;
    let scheme = &input[..separator];
    let remainder = &input[separator + 1..];

    if scheme.is_empty()
        || !scheme
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
        || !scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
        || (scheme.len() == 1
            && scheme.as_bytes()[0].is_ascii_alphabetic()
            && matches!(remainder.as_bytes().first(), Some(b'/' | b'\\')))
    {
        return None;
    }

    Some((scheme, remainder))
}

fn redact_postgres_dsn(dsn: &str) -> String {
    let Some((scheme, rest)) = dsn.split_once("://") else {
        return "<redacted PostgreSQL DSN>".to_string();
    };
    let (authority, suffix) = rest
        .find(['/', '?', '#'])
        .map(|index| rest.split_at(index))
        .unwrap_or((rest, ""));
    let authority = authority
        .rsplit_once('@')
        .map(|(user_info, host)| {
            user_info
                .split_once(':')
                .map(|(user, _)| format!("{user}:***@{host}"))
                .unwrap_or_else(|| format!("{user_info}@{host}"))
        })
        .unwrap_or_else(|| authority.to_string());
    format!(
        "{scheme}://{authority}{}",
        redact_sensitive_query_values(suffix)
    )
}

fn redact_sensitive_query_values(suffix: &str) -> String {
    let Some(query_start) = suffix.find('?') else {
        return suffix.to_string();
    };
    let (prefix, query) = suffix.split_at(query_start);
    let query = &query[1..];
    let (query, fragment) = query
        .find('#')
        .map(|index| query.split_at(index))
        .unwrap_or((query, ""));
    let redacted = query
        .split('&')
        .map(|entry| {
            let Some((key, _)) = entry.split_once('=') else {
                return entry.to_string();
            };
            if is_sensitive_query_key(key) {
                format!("{key}=***")
            } else {
                entry.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{prefix}?{redacted}{fragment}")
}

fn is_sensitive_query_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    matches!(
        key.as_str(),
        "password" | "pass" | "pwd" | "user" | "username"
    ) || key.ends_with("password")
}
