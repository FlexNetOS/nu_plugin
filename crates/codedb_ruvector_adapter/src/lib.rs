//! ARCHBP-003: the live RuVector semantic and hybrid SQL adapter.
//!
//! Deterministic, provenance-bound embeddings persist through the approved
//! PostgreSQL RuVector extension path. Retrieval is one database-local plan:
//! lexical (tsvector), vector (ruvector `<=>`), dependency, and causal.
//! redb is never a geometry engine and there is no vector sidecar — every
//! vector lives in an `extensions.ruvector` column and the adapter is the
//! only vector authority.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Declared dependency edge kind.
pub const EDGE_KIND_DEPENDENCY: &str = "dependency";
/// Declared causal edge kind.
pub const EDGE_KIND_CAUSAL: &str = "causal";
/// Maximum content bytes accepted per document (fail-closed bound).
pub const MAX_CONTENT_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub struct AdapterError(String);

impl AdapterError {
    pub fn new(m: impl Into<String>) -> Self {
        Self(m.into())
    }
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for AdapterError {}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// A deterministic, provenance-bound feature-hash embedder. It downloads no
/// model (path law: no model outside the one profile) and is fully
/// reproducible: identical text always yields the identical vector, so the
/// model identity plus the input digest fully determine the embedding.
#[derive(Debug, Clone)]
pub struct EmbeddingModel {
    pub model_id: String,
    pub model_version: String,
    pub dimension: usize,
    pub repeatability_tolerance: f32,
}

#[derive(Debug, Clone)]
pub struct Embedding {
    pub model_id: String,
    pub model_version: String,
    pub dimension: usize,
    pub input_sha256: String,
    pub vector: Vec<f32>,
}

impl Default for EmbeddingModel {
    fn default() -> Self {
        Self {
            model_id: "codedb-featurehash".to_string(),
            model_version: "v0-fnv1a-8d".to_string(),
            dimension: 8,
            repeatability_tolerance: 0.9,
        }
    }
}

impl EmbeddingModel {
    /// Deterministic token feature-hash into a fixed-dimension L2-normalized
    /// vector. Tokenization is lowercase alphanumeric runs.
    pub fn embed(&self, text: &str) -> Embedding {
        let mut vector = vec![0f32; self.dimension];
        let mut token = String::new();
        let mut flush = |token: &mut String, vector: &mut [f32]| {
            if token.is_empty() {
                return;
            }
            // FNV-1a over the token, folded into a bucket and sign.
            let mut hash: u64 = 0xcbf29ce484222325;
            for byte in token.bytes() {
                hash ^= byte as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
            let bucket = (hash % self.dimension as u64) as usize;
            let sign = if (hash >> 63) & 1 == 1 { -1.0 } else { 1.0 };
            vector[bucket] += sign;
            token.clear();
        };
        for ch in text.chars() {
            if ch.is_ascii_alphanumeric() {
                token.push(ch.to_ascii_lowercase());
            } else {
                flush(&mut token, &mut vector);
            }
        }
        let _ = &mut flush; // RED BASELINE: no tokens folded in
        // L2 normalize; keep a tiny bias so an all-zero text stays finite.
        let norm = (vector.iter().map(|v| v * v).sum::<f32>()).sqrt();
        if norm > 0.0 {
            for v in &mut vector {
                *v /= norm;
            }
        } else {
            vector[0] = 1.0;
        }
        Embedding {
            model_id: self.model_id.clone(),
            model_version: self.model_version.clone(),
            dimension: self.dimension,
            input_sha256: sha256_hex(text.as_bytes()),
            vector,
        }
    }

    pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if na == 0.0 || nb == 0.0 {
            0.0
        } else {
            dot / (na * nb)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentInput {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct ProvenanceRow {
    pub model_id: String,
    pub model_version: String,
    pub input_sha256: String,
    pub dimension: i32,
}

#[derive(Debug, Clone)]
pub struct HybridHit {
    pub id: i64,
    pub path: String,
    pub lexical_score: f32,
    pub vector_score: f32,
    pub fused_score: f32,
}

#[cfg(feature = "pg-integration")]
mod live {
    use super::*;
    use postgres::{Client, NoTls};

    /// The live adapter over a RuVector-enabled PostgreSQL connection.
    pub struct Adapter {
        conn: String,
        model: EmbeddingModel,
    }

    fn vector_literal(vector: &[f32]) -> String {
        let parts: Vec<String> = vector.iter().map(|v| format!("{v}")).collect();
        format!("[{}]", parts.join(","))
    }

    impl Adapter {
        pub fn connect(conn: &str) -> Result<Self, AdapterError> {
            // Prove the connection and the extension path up front.
            let mut client = Client::connect(conn, NoTls)
                .map_err(|_| AdapterError::new("PostgreSQL connection failed; details redacted"))?;
            let installed: i64 = client
                .query_one(
                    "SELECT count(*) FROM pg_extension e JOIN pg_namespace n \
                     ON n.oid=e.extnamespace WHERE e.extname='ruvector' AND n.nspname='extensions'",
                    &[],
                )
                .map_err(|e| AdapterError::new(format!("extension probe failed: {e}")))?
                .get(0);
            if installed != 1 {
                return Err(AdapterError::new(
                    "ruvector is not installed in schema extensions; refusing to run",
                ));
            }
            Ok(Self {
                conn: conn.to_string(),
                model: EmbeddingModel::default(),
            })
        }

        fn client(&self) -> Result<Client, AdapterError> {
            Client::connect(&self.conn, NoTls)
                .map_err(|_| AdapterError::new("PostgreSQL connection failed; details redacted"))
        }

        pub fn reset(&self) -> Result<(), AdapterError> {
            let mut c = self.client()?;
            c.batch_execute(
                "DROP TABLE IF EXISTS ruvector_adapter_edges;\
                 DROP TABLE IF EXISTS ruvector_adapter_documents;",
            )
            .map_err(|e| AdapterError::new(format!("reset failed: {e}")))
        }

        pub fn ensure_schema(&self) -> Result<(), AdapterError> {
            let dim = self.model.dimension;
            let mut c = self.client()?;
            c.batch_execute(&format!(
                "CREATE TABLE IF NOT EXISTS ruvector_adapter_documents (\
                     id BIGSERIAL PRIMARY KEY,\
                     path TEXT NOT NULL,\
                     content TEXT NOT NULL,\
                     fts tsvector,\
                     embedding extensions.ruvector({dim}) NOT NULL,\
                     model_id TEXT NOT NULL,\
                     model_version TEXT NOT NULL,\
                     input_sha256 TEXT NOT NULL,\
                     dimension INTEGER NOT NULL\
                 );\
                 CREATE TABLE IF NOT EXISTS ruvector_adapter_edges (\
                     src BIGINT NOT NULL REFERENCES ruvector_adapter_documents(id),\
                     dst BIGINT NOT NULL REFERENCES ruvector_adapter_documents(id),\
                     kind TEXT NOT NULL,\
                     PRIMARY KEY (src, dst, kind)\
                 );"
            ))
            .map_err(|e| AdapterError::new(format!("schema failed: {e}")))
        }

        pub fn persist(&self, doc: &DocumentInput) -> Result<i64, AdapterError> {
            return Err(AdapterError::new("persist is not implemented"));
            #[allow(unreachable_code)]
            if doc.content.len() > MAX_CONTENT_BYTES {
                return Err(AdapterError::new(format!(
                    "content of {} bytes exceeds the {MAX_CONTENT_BYTES}-byte bound",
                    doc.content.len()
                )));
            }
            let embedding = self.model.embed(&doc.content);
            let literal = vector_literal(&embedding.vector);
            let mut c = self.client()?;
            let mut tx = c
                .transaction()
                .map_err(|e| AdapterError::new(format!("begin: {e}")))?;
            let id: i64 = tx
                .query_one(
                    "INSERT INTO ruvector_adapter_documents \
                     (path, content, fts, embedding, model_id, model_version, input_sha256, dimension) \
                     VALUES ($1, $2, to_tsvector('english', $2), $3::text::extensions.ruvector, \
                             $4, $5, $6, $7) RETURNING id",
                    &[
                        &doc.path,
                        &doc.content,
                        &literal,
                        &embedding.model_id,
                        &embedding.model_version,
                        &embedding.input_sha256,
                        &(embedding.dimension as i32),
                    ],
                )
                .map_err(|e| AdapterError::new(format!("persist insert failed: {e}")))?
                .get(0);
            tx.commit().map_err(|e| AdapterError::new(format!("commit: {e}")))?;
            Ok(id)
        }

        pub fn get_provenance(&self, id: i64) -> Result<ProvenanceRow, AdapterError> {
            let mut c = self.client()?;
            let row = c
                .query_one(
                    "SELECT model_id, model_version, input_sha256, dimension \
                     FROM ruvector_adapter_documents WHERE id=$1",
                    &[&id],
                )
                .map_err(|e| AdapterError::new(format!("provenance: {e}")))?;
            Ok(ProvenanceRow {
                model_id: row.get(0),
                model_version: row.get(1),
                input_sha256: row.get(2),
                dimension: row.get(3),
            })
        }

        pub fn self_distance(&self, id: i64) -> Result<f32, AdapterError> {
            let mut c = self.client()?;
            let d: f32 = c
                .query_one(
                    "SELECT (embedding OPERATOR(extensions.<=>) embedding)::real \
                     FROM ruvector_adapter_documents WHERE id=$1",
                    &[&id],
                )
                .map_err(|e| AdapterError::new(format!("self distance: {e}")))?
                .get(0);
            Ok(d)
        }

        pub fn hybrid_search(&self, query: &str, k: i64) -> Result<Vec<HybridHit>, AdapterError> {
            let embedding = self.model.embed(query);
            let literal = vector_literal(&embedding.vector);
            let mut c = self.client()?;
            // One database-local plan: lexical rank + vector similarity fused
            // by a simple average of normalized components.
            let rows = c
                .query(
                    "WITH scored AS ( \
                        SELECT id, path, \
                          ts_rank(fts, plainto_tsquery('english', $1))::real AS lexical, \
                          (1.0 - (embedding OPERATOR(extensions.<=>) \
                             $2::text::extensions.ruvector))::real AS vector \
                        FROM ruvector_adapter_documents \
                     ) \
                     SELECT id, path, lexical, vector, ((lexical + vector) / 2.0)::real AS fused \
                     FROM scored ORDER BY fused DESC, id LIMIT $3",
                    &[&query, &literal, &k],
                )
                .map_err(|e| AdapterError::new(format!("hybrid search: {e}")))?;
            Ok(rows
                .into_iter()
                .map(|r| HybridHit {
                    id: r.get(0),
                    path: r.get(1),
                    lexical_score: r.get::<_, f32>(2).max(0.0),
                    vector_score: r.get::<_, f32>(3).max(0.0),
                    fused_score: r.get::<_, f32>(4).max(0.0),
                })
                .collect())
        }

        pub fn add_edge(&self, src: i64, dst: i64, kind: &str) -> Result<(), AdapterError> {
            let mut c = self.client()?;
            c.execute(
                "INSERT INTO ruvector_adapter_edges (src, dst, kind) VALUES ($1, $2, $3) \
                 ON CONFLICT DO NOTHING",
                &[&src, &dst, &kind],
            )
            .map_err(|e| AdapterError::new(format!("add edge: {e}")))?;
            Ok(())
        }

        pub fn neighbors(&self, src: i64, kind: &str) -> Result<Vec<i64>, AdapterError> {
            let mut c = self.client()?;
            let rows = c
                .query(
                    "SELECT dst FROM ruvector_adapter_edges WHERE src=$1 AND kind=$2 ORDER BY dst",
                    &[&src, &kind],
                )
                .map_err(|e| AdapterError::new(format!("neighbors: {e}")))?;
            Ok(rows.into_iter().map(|r| r.get(0)).collect())
        }

        pub fn document_count(&self) -> Result<i64, AdapterError> {
            let mut c = self.client()?;
            Ok(c.query_one("SELECT count(*) FROM ruvector_adapter_documents", &[])
                .map_err(|e| AdapterError::new(format!("count: {e}")))?
                .get(0))
        }

        pub fn vector_column_type(&self) -> Result<(String, String), AdapterError> {
            let mut c = self.client()?;
            let row = c
                .query_one(
                    "SELECT n.nspname, t.typname FROM pg_attribute a \
                     JOIN pg_type t ON t.oid=a.atttypid \
                     JOIN pg_namespace n ON n.oid=t.typnamespace \
                     WHERE a.attrelid='ruvector_adapter_documents'::regclass \
                       AND a.attname='embedding'",
                    &[],
                )
                .map_err(|e| AdapterError::new(format!("column type: {e}")))?;
            Ok((row.get(0), row.get(1)))
        }
    }
}

#[cfg(feature = "pg-integration")]
pub use live::Adapter;
