//! No-orphan-feature guard.
//!
//! Every `.feature` file must be claimed by a runner — either a per-PR test
//! binary (part of the CI gate) or a documented `task bdd:*` target. This
//! is the regression guard for the incoherence PR #178 cleaned up: 10
//! feature files existed that nothing executed, and one of them hid a
//! data-loss bug (#176) for months. Adding a feature without deciding who
//! runs it now fails CI with this message.

use std::collections::BTreeSet;
use std::path::Path;

/// feature path (relative to cucumber-tests/) → its runner.
/// Per-PR gate binaries run in CI on every PR; task targets are manual or
/// nightly by design (perf/stress/cluster are long-running).
const CLAIMED: &[(&str, &str)] = &[
    // — per-PR CI gate —
    ("features/persistence/event_sourcing.feature", "test:cucumber_tests"),
    ("features/persistence/hash_chain.feature", "test:cucumber_tests"),
    ("features/persistence/retention.feature", "test:cucumber_tests"),
    ("features/persistence/retention_deferred.feature", "test:cucumber_tests"),
    ("features/models/user.feature", "test:behavior_models_test"),
    ("features/models/order.feature", "test:behavior_models_test"),
    ("features/models/invoice.feature", "test:behavior_models_test"),
    ("features/models/document.feature", "test:behavior_models_test"),
    ("features/core/scaffolding.feature", "test:scaffolding_test"),
    ("features/performance/serialization_modes.feature", "test:serialization_test"),
    // — dedicated long-running binaries (task bdd:* / nightly) —
    ("features/performance/bench_isolated.feature", "test:bench_isolated"),
    (
        "features/performance/cluster_stress_multi_model.feature",
        "test:cluster_stress_test",
    ),
    ("features/performance/database_performance.feature", "test:database_perf_test"),
    ("features/performance/durability_test.feature", "test:durability_test"),
    ("features/performance/engine_direct_test.feature", "test:engine_direct_test"),
    (
        "features/performance/engine_reliability_test.feature",
        "test:engine_reliability_test",
    ),
    (
        "features/performance/multi_file_durability.feature",
        "test:multi_file_durability_test",
    ),
    (
        "features/performance/snapshot_durability.feature",
        "test:snapshot_durability_test",
    ),
    ("features/performance/stress_1m_test.feature", "test:stress_1m_test"),
    ("features/performance/stress_snapshot_1m.feature", "test:stress_snapshot_test"),
    // — claimed by task targets only (run via the generic `cucumber` bin) —
    ("features/performance/event_sourcing_benchmark.feature", "task bdd:performance"),
    ("features/performance/retention_performance.feature", "task bdd:performance"),
    ("features/core/distribution_clustering.feature", "task bdd:distribution"),
    ("features/core/real_cluster_test.feature", "task bdd:distribution"),
];

#[test]
fn every_feature_file_has_a_runner() {
    let claimed: BTreeSet<&str> = CLAIMED.iter().map(|(f, _)| *f).collect();

    let mut on_disk = BTreeSet::new();
    collect_features(Path::new("features"), &mut on_disk);

    let orphans: Vec<_> = on_disk.iter().filter(|f| !claimed.contains(f.as_str())).collect();
    assert!(
        orphans.is_empty(),
        "feature file(s) with NO runner: {:?}\n\
         Decide who executes each one (a per-PR test binary or a task bdd:* \
         target) and add it to CLAIMED in {} — unrun specs rot, and one hid \
         a data-loss bug for months (#176).",
        orphans,
        file!()
    );

    let ghosts: Vec<_> = claimed.iter().filter(|f| !on_disk.contains(**f)).collect();
    assert!(
        ghosts.is_empty(),
        "CLAIMED entry for deleted feature file(s): {:?} — remove them from {}",
        ghosts,
        file!()
    );
}

fn collect_features(dir: &Path, out: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).expect("read features dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_features(&path, out);
        } else if path.extension().is_some_and(|e| e == "feature") {
            out.insert(path.to_string_lossy().into_owned());
        }
    }
}
