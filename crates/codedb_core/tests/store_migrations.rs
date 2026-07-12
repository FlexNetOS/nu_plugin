use codedb_core::SchemaVersion;
use codedb_core::store::{
    CURRENT_STORE_SCHEMA_VERSION, LEGACY_STORE_SCHEMA_VERSION, StoreBackend, StoreMigrationStep,
    parse_schema_version, plan_store_migration,
};

#[test]
fn schema_versions_parse_strictly_and_render_canonically() {
    let parsed = parse_schema_version("12.34.56").expect("valid semantic store version");
    assert_eq!(parsed, SchemaVersion::new(12, 34, 56));
    assert_eq!(parsed.to_string(), "12.34.56");

    for invalid in ["", "1", "1.0", "1.0.0.0", "v1.0.0", "1.x.0", "65536.0.0"] {
        assert!(
            parse_schema_version(invalid).is_err(),
            "{invalid:?} must not be accepted as a store schema version"
        );
    }
}

#[test]
fn planner_resolves_a_dynamic_multi_step_path_without_backend_branching() {
    let middle = SchemaVersion::new(0, 10, 0);
    let steps = [
        StoreMigrationStep::new(
            "legacy_to_intermediate",
            LEGACY_STORE_SCHEMA_VERSION,
            middle,
        ),
        StoreMigrationStep::new(
            "intermediate_to_current",
            middle,
            CURRENT_STORE_SCHEMA_VERSION,
        ),
    ];

    for backend in [StoreBackend::Redb, StoreBackend::PostgreSql] {
        let plan = plan_store_migration(
            backend,
            LEGACY_STORE_SCHEMA_VERSION,
            CURRENT_STORE_SCHEMA_VERSION,
            &steps,
        )
        .expect("known migration chain");
        assert_eq!(plan.backend, backend);
        assert_eq!(plan.observed_version, LEGACY_STORE_SCHEMA_VERSION);
        assert_eq!(plan.target_version, CURRENT_STORE_SCHEMA_VERSION);
        assert_eq!(plan.steps, steps);
    }
}

#[test]
fn planner_refuses_unknown_downgrade_ambiguous_and_cyclic_routes() {
    let known = StoreMigrationStep::new(
        "legacy_to_current",
        LEGACY_STORE_SCHEMA_VERSION,
        CURRENT_STORE_SCHEMA_VERSION,
    );
    let unknown = SchemaVersion::new(0, 8, 0);
    let future = SchemaVersion::new(99, 0, 0);

    for (observed, target, steps, expected) in [
        (
            unknown,
            CURRENT_STORE_SCHEMA_VERSION,
            vec![known],
            "no supported migration",
        ),
        (
            future,
            CURRENT_STORE_SCHEMA_VERSION,
            vec![known],
            "downgrade",
        ),
        (
            LEGACY_STORE_SCHEMA_VERSION,
            CURRENT_STORE_SCHEMA_VERSION,
            vec![
                known,
                StoreMigrationStep::new(
                    "ambiguous",
                    LEGACY_STORE_SCHEMA_VERSION,
                    CURRENT_STORE_SCHEMA_VERSION,
                ),
            ],
            "ambiguous",
        ),
        (
            LEGACY_STORE_SCHEMA_VERSION,
            CURRENT_STORE_SCHEMA_VERSION,
            vec![StoreMigrationStep::new(
                "cycle",
                LEGACY_STORE_SCHEMA_VERSION,
                LEGACY_STORE_SCHEMA_VERSION,
            )],
            "advance",
        ),
    ] {
        let error = plan_store_migration(StoreBackend::Redb, observed, target, &steps)
            .expect_err("unsafe migration route must fail closed");
        assert!(
            error.message().contains(expected),
            "unexpected planner error for {observed:?}: {error}"
        );
    }
}
