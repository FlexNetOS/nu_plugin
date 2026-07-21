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

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

/// Versioned UDS request protocol.
pub const PROTOCOL_VERSION: &str = "flexnetos.redb-owner.v0";
/// Versioned on-disk projection format.
pub const PROJECTION_FORMAT_VERSION: &str = "flexnetos.redb-owner.projection.v0";

const STATE: TableDefinition<&str, &str> = TableDefinition::new("state");
const META: TableDefinition<&str, u64> = TableDefinition::new("meta");
const LOCAL_SEQ_KEY: &str = "local_seq";

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

fn internal(message: impl std::fmt::Display) -> OwnerError {
    OwnerError::Internal(message.to_string())
}

struct OwnerPaths {
    db: PathBuf,
    socket: PathBuf,
    token: PathBuf,
    pointer: PathBuf,
    spool: PathBuf,
}

impl OwnerPaths {
    fn new(root: &Path) -> Self {
        Self {
            db: root.join("owner.redb"),
            socket: root.join("owner.sock"),
            token: root.join("owner.token"),
            pointer: root.join("projection.pointer"),
            spool: root.join("events.spool"),
        }
    }

    fn slot(root: &Path, slot: &str) -> PathBuf {
        root.join(format!("projection.{slot}.slot"))
    }
}

/// In-process ownership registry: redb's cross-process file lock does not
/// deny a second open from the SAME process, so the single-opener contract
/// is enforced here as well.
fn owned_roots() -> &'static Mutex<BTreeSet<PathBuf>> {
    static OWNED: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();
    OWNED.get_or_init(|| Mutex::new(BTreeSet::new()))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PointerGeneration {
    slot: String,
    local_seq: u64,
    checksum: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PointerFile {
    format_version: String,
    slot: String,
    local_seq: u64,
    checksum: String,
    previous: Option<PointerGeneration>,
}

#[derive(Debug, Deserialize)]
struct Request {
    protocol_version: String,
    token: String,
    op: String,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    value: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Response {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
}

struct Shared {
    root: PathBuf,
    db: Database,
    token: String,
    stop: AtomicBool,
    publish_crash: AtomicBool,
    write_lock: Mutex<()>,
    connections: Mutex<Vec<UnixStream>>,
}

/// The running owner service. Dropping it shuts the service down.
pub struct OwnerService {
    shared: Option<Arc<Shared>>,
    accept: Option<JoinHandle<()>>,
    workers: Arc<Mutex<Vec<JoinHandle<()>>>>,
    root: PathBuf,
}

impl std::fmt::Debug for OwnerService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnerService")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

fn random_token() -> Result<String, OwnerError> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn db_local_seq(db: &Database) -> Result<u64, OwnerError> {
    let read_txn = db.begin_read().map_err(internal)?;
    match read_txn.open_table(META) {
        Ok(meta) => Ok(meta
            .get(LOCAL_SEQ_KEY)
            .map_err(internal)?
            .map(|v| v.value())
            .unwrap_or(0)),
        Err(redb::TableError::TableDoesNotExist(_)) => Ok(0),
        Err(err) => Err(internal(err)),
    }
}

fn db_snapshot(db: &Database) -> Result<(u64, BTreeMap<String, String>), OwnerError> {
    let read_txn = db.begin_read().map_err(internal)?;
    let seq = match read_txn.open_table(META) {
        Ok(meta) => meta
            .get(LOCAL_SEQ_KEY)
            .map_err(internal)?
            .map(|v| v.value())
            .unwrap_or(0),
        Err(redb::TableError::TableDoesNotExist(_)) => 0,
        Err(err) => return Err(internal(err)),
    };
    let mut entries = BTreeMap::new();
    match read_txn.open_table(STATE) {
        Ok(state) => {
            for row in state.iter().map_err(internal)? {
                let (key, value) = row.map_err(internal)?;
                entries.insert(key.value().to_string(), value.value().to_string());
            }
        }
        Err(redb::TableError::TableDoesNotExist(_)) => {}
        Err(err) => return Err(internal(err)),
    }
    Ok((seq, entries))
}

fn render_projection(seq: u64, entries: &BTreeMap<String, String>) -> Vec<u8> {
    let mut body = serde_json::json!({
        "format_version": PROJECTION_FORMAT_VERSION,
        "local_seq": seq,
        "entry_count": entries.len(),
    })
    .to_string();
    body.push('\n');
    for (key, value) in entries {
        body.push_str(&serde_json::json!({"k": key, "v": value}).to_string());
        body.push('\n');
    }
    body.into_bytes()
}

fn read_pointer(paths: &OwnerPaths) -> Result<Option<PointerFile>, OwnerError> {
    match std::fs::read_to_string(&paths.pointer) {
        Ok(text) => Ok(Some(
            serde_json::from_str(&text)
                .map_err(|e| OwnerError::Corrupt(format!("pointer file: {e}")))?,
        )),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn fsync_file(path: &Path) -> Result<(), OwnerError> {
    std::fs::File::open(path)?.sync_all()?;
    Ok(())
}

/// Publish the current database snapshot into the inactive slot and flip the
/// pointer atomically. Appends the ordered commit notification only after
/// the generation is durable and active.
fn publish_projection(root: &Path, db: &Database) -> Result<(), OwnerError> {
    let paths = OwnerPaths::new(root);
    let (seq, entries) = db_snapshot(db)?;
    let body = render_projection(seq, &entries);
    let checksum = sha256_hex(&body);
    let current = read_pointer(&paths)?;
    if let Some(pointer) = &current
        && pointer.local_seq == seq
        && pointer.checksum == checksum
    {
        return Ok(());
    }
    let slot = match current.as_ref().map(|p| p.slot.as_str()) {
        Some("a") => "b",
        _ => "a",
    };
    let slot_path = OwnerPaths::slot(root, slot);
    let tmp_slot = root.join(format!("projection.{slot}.slot.tmp"));
    std::fs::write(&tmp_slot, &body)?;
    fsync_file(&tmp_slot)?;
    std::fs::rename(&tmp_slot, &slot_path)?;
    let pointer = PointerFile {
        format_version: PROJECTION_FORMAT_VERSION.to_string(),
        slot: slot.to_string(),
        local_seq: seq,
        checksum: checksum.clone(),
        previous: current.map(|p| PointerGeneration {
            slot: p.slot,
            local_seq: p.local_seq,
            checksum: p.checksum,
        }),
    };
    let tmp_pointer = root.join("projection.pointer.tmp");
    std::fs::write(
        &tmp_pointer,
        serde_json::to_string_pretty(&pointer).map_err(internal)?,
    )?;
    fsync_file(&tmp_pointer)?;
    std::fs::rename(&tmp_pointer, &paths.pointer)?;
    // Ordered commit notification, appended only for a durable generation.
    let mut spool = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.spool)?;
    let event = serde_json::json!({"seq": seq, "slot": slot, "checksum": checksum});
    writeln!(spool, "{event}")?;
    spool.sync_all()?;
    Ok(())
}

impl OwnerService {
    pub fn start(root: impl AsRef<Path>) -> Result<Self, OwnerError> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;
        let canonical = root.canonicalize()?;
        {
            let mut owned = owned_roots().lock().expect("ownership registry");
            if owned.contains(&canonical) {
                return Err(OwnerError::AlreadyOwned(format!(
                    "{} is already owned in this process",
                    canonical.display()
                )));
            }
            owned.insert(canonical.clone());
        }
        let guard = OwnedRootGuard(Some(canonical.clone()));

        let paths = OwnerPaths::new(&root);
        let db = Database::create(&paths.db).map_err(|e| {
            OwnerError::AlreadyOwned(format!("redb refused the handle: {e}"))
        })?;

        // Auth token: created once with owner-only permissions.
        if !paths.token.exists() {
            let token = random_token()?;
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&paths.token)?;
            file.write_all(token.as_bytes())?;
            file.sync_all()?;
        }
        let token = std::fs::read_to_string(&paths.token)?.trim().to_string();

        // Crash replay: if the projection lags the database (a crash landed
        // between commit and flip), republish before serving.
        let db_seq = db_local_seq(&db)?;
        let pointer_seq = read_pointer(&paths)?.map(|p| p.local_seq).unwrap_or(0);
        if db_seq > pointer_seq {
            publish_projection(&root, &db)?;
        }

        // Stale socket from a crashed predecessor is safe to replace: the
        // database handle above is the ownership arbiter.
        if paths.socket.exists() {
            std::fs::remove_file(&paths.socket)?;
        }
        let listener = UnixListener::bind(&paths.socket)?;

        let shared = Arc::new(Shared {
            root: root.clone(),
            db,
            token,
            stop: AtomicBool::new(false),
            publish_crash: AtomicBool::new(false),
            write_lock: Mutex::new(()),
            connections: Mutex::new(Vec::new()),
        });
        let workers: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::new()));

        listener.set_nonblocking(true)?;
        let accept_shared = Arc::clone(&shared);
        let accept_workers = Arc::clone(&workers);
        let accept = std::thread::spawn(move || {
            loop {
                if accept_shared.stop.load(Ordering::SeqCst) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        if stream.set_nonblocking(false).is_err() {
                            continue;
                        }
                        if let Ok(clone) = stream.try_clone() {
                            accept_shared
                                .connections
                                .lock()
                                .expect("connection registry")
                                .push(clone);
                        }
                        let worker_shared = Arc::clone(&accept_shared);
                        let handle = std::thread::spawn(move || {
                            serve_connection(worker_shared, stream);
                        });
                        accept_workers.lock().expect("worker registry").push(handle);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        std::mem::forget(guard);
        Ok(Self {
            shared: Some(shared),
            accept: Some(accept),
            workers,
            root: canonical,
        })
    }

    /// Test failpoint: the next publication dies after the redb commit and
    /// before the projection flip.
    pub fn inject_publish_crash(&self) {
        if let Some(shared) = &self.shared {
            shared.publish_crash.store(true, Ordering::SeqCst);
        }
    }
}

struct OwnedRootGuard(Option<PathBuf>);

impl Drop for OwnedRootGuard {
    fn drop(&mut self) {
        if let Some(root) = self.0.take() {
            owned_roots().lock().expect("ownership registry").remove(&root);
        }
    }
}

impl Drop for OwnerService {
    fn drop(&mut self) {
        if let Some(shared) = &self.shared {
            shared.stop.store(true, Ordering::SeqCst);
            for connection in shared
                .connections
                .lock()
                .expect("connection registry")
                .iter()
            {
                let _ = connection.shutdown(std::net::Shutdown::Both);
            }
        }
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        let workers = std::mem::take(&mut *self.workers.lock().expect("worker registry"));
        for worker in workers {
            let _ = worker.join();
        }
        // Drop the database handle before releasing in-process ownership.
        self.shared.take();
        owned_roots()
            .lock()
            .expect("ownership registry")
            .remove(&self.root);
    }
}

fn serve_connection(shared: Arc<Shared>, stream: UnixStream) {
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let mut writer = stream;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        let response = handle_request(&shared, line.trim());
        let rendered = serde_json::to_string(&response)
            .unwrap_or_else(|_| "{\"ok\":false,\"error\":\"render\"}".to_string());
        if writeln!(writer, "{rendered}").is_err() {
            break;
        }
    }
}

fn handle_request(shared: &Shared, line: &str) -> Response {
    let reject = |message: &str| Response {
        ok: false,
        error: Some(message.to_string()),
        seq: None,
        value: None,
    };
    let request: Request = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(error) => return reject(&format!("malformed request: {error}")),
    };
    if request.protocol_version != PROTOCOL_VERSION {
        return reject(&format!(
            "unsupported protocol version {} (expected {PROTOCOL_VERSION})",
            request.protocol_version
        ));
    }
    if request.token != shared.token {
        return reject("authentication failed");
    }
    match request.op.as_str() {
        "put" => {
            let (Some(key), Some(value)) = (request.key, request.value) else {
                return reject("put requires key and value");
            };
            let _write_guard = shared.write_lock.lock().expect("write lock");
            let seq = match write_state(&shared.db, &key, &value) {
                Ok(seq) => seq,
                Err(error) => return reject(&format!("write failed: {error}")),
            };
            if shared.publish_crash.swap(false, Ordering::SeqCst) {
                return reject(
                    "publication crashed after commit; the projection will replay on restart",
                );
            }
            if let Err(error) = publish_projection(&shared.root, &shared.db) {
                return reject(&format!("publication failed: {error}"));
            }
            Response {
                ok: true,
                error: None,
                seq: Some(seq),
                value: None,
            }
        }
        "get" => {
            let Some(key) = request.key else {
                return reject("get requires key");
            };
            match read_state(&shared.db, &key) {
                Ok(value) => Response {
                    ok: true,
                    error: None,
                    seq: None,
                    value,
                },
                Err(error) => reject(&format!("read failed: {error}")),
            }
        }
        "status" => match db_local_seq(&shared.db) {
            Ok(seq) => Response {
                ok: true,
                error: None,
                seq: Some(seq),
                value: None,
            },
            Err(error) => reject(&format!("status failed: {error}")),
        },
        other => reject(&format!("unknown op {other:?}")),
    }
}

fn write_state(db: &Database, key: &str, value: &str) -> Result<u64, OwnerError> {
    let write_txn = db.begin_write().map_err(internal)?;
    let seq;
    {
        let mut state = write_txn.open_table(STATE).map_err(internal)?;
        state.insert(key, value).map_err(internal)?;
        let mut meta = write_txn.open_table(META).map_err(internal)?;
        let current = meta
            .get(LOCAL_SEQ_KEY)
            .map_err(internal)?
            .map(|v| v.value())
            .unwrap_or(0);
        seq = current + 1;
        meta.insert(LOCAL_SEQ_KEY, seq).map_err(internal)?;
    }
    write_txn.commit().map_err(internal)?;
    Ok(seq)
}

fn read_state(db: &Database, key: &str) -> Result<Option<String>, OwnerError> {
    let read_txn = db.begin_read().map_err(internal)?;
    match read_txn.open_table(STATE) {
        Ok(state) => Ok(state
            .get(key)
            .map_err(internal)?
            .map(|v| v.value().to_string())),
        Err(redb::TableError::TableDoesNotExist(_)) => Ok(None),
        Err(err) => Err(internal(err)),
    }
}

/// A UDS client speaking the versioned authenticated protocol.
pub struct OwnerClient {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    token: String,
    protocol_version: String,
}

impl OwnerClient {
    pub fn connect(root: impl AsRef<Path>) -> Result<Self, OwnerError> {
        let paths = OwnerPaths::new(root.as_ref());
        let token = std::fs::read_to_string(&paths.token)?.trim().to_string();
        let stream = UnixStream::connect(&paths.socket)?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self {
            stream,
            reader,
            token,
            protocol_version: PROTOCOL_VERSION.to_string(),
        })
    }

    pub fn override_token(&mut self, token: &str) {
        self.token = token.to_string();
    }

    pub fn override_protocol_version(&mut self, version: &str) {
        self.protocol_version = version.to_string();
    }

    fn round_trip(&mut self, request: serde_json::Value) -> Result<Response, OwnerError> {
        let line = serde_json::to_string(&request).map_err(internal)?;
        writeln!(self.stream, "{line}")?;
        let mut response = String::new();
        self.reader.read_line(&mut response)?;
        if response.is_empty() {
            return Err(internal("owner closed the connection"));
        }
        serde_json::from_str(&response).map_err(|e| internal(format!("bad response: {e}")))
    }

    fn request(&mut self, op: &str, key: Option<&str>, value: Option<&str>) -> Result<Response, OwnerError> {
        let response = self.round_trip(serde_json::json!({
            "protocol_version": self.protocol_version,
            "token": self.token,
            "op": op,
            "key": key,
            "value": value,
        }))?;
        if !response.ok {
            return Err(OwnerError::Rejected(
                response.error.unwrap_or_else(|| "unspecified".into()),
            ));
        }
        Ok(response)
    }

    pub fn put(&mut self, key: &str, value: &str) -> Result<u64, OwnerError> {
        let response = self.request("put", Some(key), Some(value))?;
        response
            .seq
            .ok_or_else(|| internal("put response lacked a sequence"))
    }

    pub fn get(&mut self, key: &str) -> Result<Option<String>, OwnerError> {
        Ok(self.request("get", Some(key), None)?.value)
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

fn read_slot_mmap(
    root: &Path,
    generation_slot: &str,
    expected_checksum: &str,
) -> Result<(u64, BTreeMap<String, String>), OwnerError> {
    let slot_path = OwnerPaths::slot(root, generation_slot);
    let file = std::fs::File::open(&slot_path)?;
    // The projection is consumed through a read-only memory mapping — the
    // exact live-read path LifeOS uses — and verified against the pointer's
    // checksum before a single byte is trusted.
    let mmap = unsafe { memmap2::Mmap::map(&file) }.map_err(OwnerError::Io)?;
    if sha256_hex(&mmap) != expected_checksum {
        return Err(OwnerError::Corrupt(format!(
            "slot {generation_slot} does not match its witnessed checksum"
        )));
    }
    let text = std::str::from_utf8(&mmap)
        .map_err(|e| OwnerError::Corrupt(format!("slot {generation_slot}: {e}")))?;
    let mut lines = text.lines();
    let header: serde_json::Value = serde_json::from_str(
        lines
            .next()
            .ok_or_else(|| OwnerError::Corrupt("empty projection".into()))?,
    )
    .map_err(|e| OwnerError::Corrupt(format!("projection header: {e}")))?;
    if header["format_version"] != PROJECTION_FORMAT_VERSION {
        return Err(OwnerError::Corrupt(format!(
            "unsupported projection format: {}",
            header["format_version"]
        )));
    }
    let seq = header["local_seq"]
        .as_u64()
        .ok_or_else(|| OwnerError::Corrupt("projection header lacks local_seq".into()))?;
    let mut entries = BTreeMap::new();
    for line in lines {
        let row: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| OwnerError::Corrupt(format!("projection row: {e}")))?;
        entries.insert(
            row["k"]
                .as_str()
                .ok_or_else(|| OwnerError::Corrupt("row lacks k".into()))?
                .to_string(),
            row["v"]
                .as_str()
                .ok_or_else(|| OwnerError::Corrupt("row lacks v".into()))?
                .to_string(),
        );
    }
    Ok((seq, entries))
}

impl ProjectionReader {
    pub fn read(root: impl AsRef<Path>) -> Result<Projection, OwnerError> {
        let root = root.as_ref();
        let paths = OwnerPaths::new(root);
        let pointer = read_pointer(&paths)?
            .ok_or_else(|| OwnerError::Corrupt("no projection has been published".into()))?;
        match read_slot_mmap(root, &pointer.slot, &pointer.checksum) {
            Ok((seq, entries)) => Ok(Projection {
                local_seq: seq,
                slot: pointer.slot,
                checksum: pointer.checksum,
                degraded: false,
                entries,
            }),
            Err(active_error) => {
                let Some(previous) = pointer.previous else {
                    return Err(active_error);
                };
                let (seq, entries) =
                    read_slot_mmap(root, &previous.slot, &previous.checksum)?;
                Ok(Projection {
                    local_seq: seq,
                    slot: previous.slot,
                    checksum: previous.checksum,
                    degraded: true,
                    entries,
                })
            }
        }
    }
}

/// One ordered commit notification.
#[derive(Debug, Clone, Deserialize)]
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
    let paths = OwnerPaths::new(root.as_ref());
    let text = match std::fs::read_to_string(&paths.spool) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    let mut events = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let event: CommitEvent = serde_json::from_str(line)
            .map_err(|e| OwnerError::Corrupt(format!("event spool: {e}")))?;
        if event.seq > after_seq {
            events.push(event);
        }
    }
    Ok(events)
}
