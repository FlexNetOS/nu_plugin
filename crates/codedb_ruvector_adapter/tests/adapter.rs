#![cfg(feature = "pg-integration")]

//! ARCHBP-003 red tests: the live RuVector semantic and hybrid SQL adapter.
//! Deterministic provenance-bound embeddings persist through the approved
//! PostgreSQL RuVector extension path; lexical, vector, dependency, and
//! causal retrieval all resolve through PostgreSQL; nothing becomes a
//! competing vector-store authority. Uses RUVECTOR_ADAPTER_PG_CONN against
//! the disposable cluster.

use codedb_ruvector_adapter::{
    Adapter, DocumentInput, EmbeddingModel, EDGE_KIND_CAUSAL, EDGE_KIND_DEPENDENCY,
};
use std::sync::Mutex;

static PG_LOCK: Mutex<()> = Mutex::new(());

fn conn() -> String {
    std::env::var("RUVECTOR_ADAPTER_PG_CONN")
        .expect("RUVECTOR_ADAPTER_PG_CONN must select the disposable RuVector-enabled service")
}

fn adapter() -> Adapter {
    let a = Adapter::connect(&conn()).expect("adapter connects");
    a.reset().expect("reset");
    a.ensure_schema().expect("schema");
    a
}

#[test]
fn embeddings_are_deterministic_and_provenance_bound() {
    let model = EmbeddingModel::default();
    assert_eq!(model.model_id, "codedb-featurehash");
    assert!(!model.model_version.is_empty());
    let a = model.embed("fn parse_config() {}");
    let b = model.embed("fn parse_config() {}");
    assert_eq!(a.vector, b.vector, "identical text must embed identically");
    assert_eq!(a.dimension, model.dimension);
    assert_eq!(a.model_id, model.model_id);
    assert_eq!(a.input_sha256.len(), 64);
    // Near-identical inputs stay within the declared cosine tolerance.
    let c = model.embed("fn parse_config()  {}");
    assert!(
        EmbeddingModel::cosine(&a.vector, &c.vector) >= model.repeatability_tolerance,
        "near-identical inputs must embed within tolerance"
    );
    // Distinct inputs are meaningfully separated.
    let d = model.embed("completely unrelated content about gardening");
    assert!(EmbeddingModel::cosine(&a.vector, &d.vector) < 0.99);
}

#[test]
fn persistence_goes_through_postgresql_ruvector_and_records_provenance() {
    let _g = PG_LOCK.lock().unwrap();
    let a = adapter();
    let id = a
        .persist(&DocumentInput {
            path: "src/config.rs".into(),
            content: "fn parse_config() -> Config { Config::default() }".into(),
        })
        .expect("persist");
    let row = a.get_provenance(id).expect("provenance");
    assert_eq!(row.model_id, "codedb-featurehash");
    assert!(!row.model_version.is_empty());
    assert_eq!(row.input_sha256.len(), 64);
    assert_eq!(row.dimension, EmbeddingModel::default().dimension as i32);
    // The vector lives in a real ruvector column, queryable by the operator.
    let self_distance = a.self_distance(id).expect("self distance");
    assert!(self_distance.abs() < 1e-5, "a document is closest to itself");
}

#[test]
fn hybrid_retrieval_fuses_lexical_and_vector_signals() {
    let _g = PG_LOCK.lock().unwrap();
    let a = adapter();
    for (path, content) in [
        ("src/config.rs", "fn parse_config() reads the configuration file"),
        ("src/net.rs", "fn open_socket() binds a TCP listener for networking"),
        ("src/db.rs", "fn connect_database() opens a PostgreSQL connection pool"),
        ("README.md", "project overview and gardening tips unrelated to code"),
    ] {
        a.persist(&DocumentInput { path: path.into(), content: content.into() })
            .expect("persist");
    }
    let hits = a.hybrid_search("configuration file parsing", 3).expect("hybrid search");
    assert!(!hits.is_empty());
    assert_eq!(hits[0].path, "src/config.rs", "the config doc must rank first");
    for hit in &hits {
        assert!(hit.lexical_score >= 0.0);
        assert!(hit.vector_score >= 0.0);
        assert!(hit.fused_score >= 0.0);
    }
}

#[test]
fn dependency_and_causal_edges_have_integrity() {
    let _g = PG_LOCK.lock().unwrap();
    let a = adapter();
    let config = a.persist(&DocumentInput { path: "src/config.rs".into(), content: "config".into() }).unwrap();
    let app = a.persist(&DocumentInput { path: "src/app.rs".into(), content: "app uses config".into() }).unwrap();
    let failure = a.persist(&DocumentInput { path: "logs/crash.txt".into(), content: "crash".into() }).unwrap();

    a.add_edge(app, config, EDGE_KIND_DEPENDENCY).expect("dependency edge");
    a.add_edge(config, failure, EDGE_KIND_CAUSAL).expect("causal edge");

    let deps = a.neighbors(app, EDGE_KIND_DEPENDENCY).expect("deps");
    assert!(deps.contains(&config), "app depends on config");
    let caused = a.neighbors(config, EDGE_KIND_CAUSAL).expect("causal");
    assert!(caused.contains(&failure), "config change caused the failure record");
    // Edges are directional: config does not depend on app.
    let reverse = a.neighbors(config, EDGE_KIND_DEPENDENCY).expect("reverse");
    assert!(!reverse.contains(&app), "dependency edges are directional");
}

#[test]
fn failed_persist_rolls_back_with_no_partial_row() {
    let _g = PG_LOCK.lock().unwrap();
    let a = adapter();
    let before = a.document_count().expect("count");
    // An oversized content that violates the declared bound must fail closed
    // with no row and no orphaned vector.
    let huge = "x".repeat(2 * 1024 * 1024);
    let result = a.persist(&DocumentInput { path: "src/huge.rs".into(), content: huge });
    assert!(result.is_err(), "oversized content must fail closed");
    assert_eq!(a.document_count().expect("count"), before, "no partial row survives");
}

#[test]
fn adapter_is_the_only_vector_authority_no_sidecar() {
    let _g = PG_LOCK.lock().unwrap();
    let a = adapter();
    a.persist(&DocumentInput { path: "src/x.rs".into(), content: "x".into() }).unwrap();
    // Every vector lives in the PostgreSQL ruvector column; the adapter
    // exposes no second store and the column type is the extension's.
    let (schema, typename) = a.vector_column_type().expect("column type");
    assert_eq!(schema, "extensions");
    assert_eq!(typename, "ruvector", "vectors are ruvector-typed, never a redb geometry or sidecar");
}
