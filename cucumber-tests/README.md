# 🥒 Cucumber Tests for Lithair

Complete BDD (Behavior-Driven Development) test suite for the Lithair framework.

## 🎯 Objective

**Use Cucumber as central pillar** for:
- ✅ Testing all features (features + bugs)
- ✅ Documenting expected behavior (readable Gherkin)
- ✅ Validating complete integration (real tests, not stubs)
- ✅ Tracking discovered bugs with technical context

## 📁 Structure

```
cucumber-tests/
├── features/                   # Gherkin specifications (.feature)
│   ├── basic.feature          # Basic tests
│   ├── core/                  # Core framework features
│   ├── persistence/           # Persistence & event sourcing
│   ├── integration/           # Integrations (sessions, web, models)
│   └── observability/         # Monitoring, logs, metrics
│
├── src/features/
│   ├── world.rs              # LithairWorld (shared state + real engine)
│   └── steps/                # Step implementations
│       ├── basic_steps.rs
│       ├── advanced_persistence_steps.rs
│       ├── distribution_steps.rs
│       ├── security_steps.rs
│       └── ...
│
├── tests/
│   └── cucumber_tests.rs     # Main runner
│
├── TESTING_STACK.md          # 📊 Complete technical documentation
├── BUG_REPORTS.md            # 🐛 History of discovered bugs
└── README.md                 # 📖 This file
```

## 🚀 Quick Start

### Run all tests

```bash
cd cucumber-tests
cargo test --test cucumber_tests
```

### Run a specific feature

```bash
# Only advanced persistence
cargo test --test cucumber_tests -- features/persistence/advanced_persistence.feature

# Only basic
cargo test --test cucumber_tests -- features/basic.feature
```

### Enable detailed logs

```bash
export RUST_LOG=debug
export LT_OPT_PERSIST=1
cargo test --test cucumber_tests
```

## 📝 Workflow: Add a new test

### 1. Create the Gherkin feature

`features/my_module/new_feature.feature`:

```gherkin
# Stack: Lithair Core + MyModule v1.0
# Known bugs: None

Feature: My New Feature
  As a developer
  I want to test MyModule
  In order to ensure it works correctly

  Background:
    Given a Lithair server with MyModule enabled

  @critical @my_module
  Scenario: Nominal case
    When I perform action X
    Then the result should be Y
    And state should be consistent
```

### 2. Create the steps

`src/features/steps/my_module_steps.rs`:

```rust
use cucumber::{given, when, then};
use crate::features::world::LithairWorld;

/// Initialize MyModule for tests
///
/// # Technical Stack
/// - Uses MyModule::new() with test config
/// - Creates temporary directory for data
///
/// # Performance
/// - Time: ~100ms
#[given(expr = "a Lithair server with MyModule enabled")]
async fn given_my_module_enabled(world: &mut LithairWorld) {
    // Real initialization, not a stub!
    let temp_path = world.init_temp_storage().await
        .expect("Init storage failed");

    // TODO: Initialize MyModule here

    println!("✅ MyModule enabled: {:?}", temp_path);
}

#[when(expr = "I perform action X")]
async fn when_action_x(world: &mut LithairWorld) {
    // REAL TEST: Call MyModule
    // let result = world.my_module.do_action_x().await?;

    println!("🔧 Action X performed");
}

#[then(expr = "the result should be Y")]
async fn then_result_is_y(world: &mut LithairWorld) {
    // REAL ASSERTION
    // let actual = world.my_module.get_result();
    // assert_eq!(actual, "Y", "Incorrect result");

    println!("✅ Result validated: Y");
}

#[then(expr = "state should be consistent")]
async fn then_state_consistent(world: &mut LithairWorld) {
    // REAL VERIFICATION
    let checksum = world.compute_memory_checksum().await;
    println!("✅ Consistent state (checksum: 0x{:08x})", checksum);
}
```

### 3. Register the module

`src/features/steps/mod.rs`:

```rust
pub mod my_module_steps;
```

### 4. Run the tests

```bash
cargo test --test cucumber_tests
```

## 🐛 Document a discovered bug

### When a test fails

1. **Identify** the failing scenario
2. **Reproduce** manually
3. **Document** in `BUG_REPORTS.md`:

```markdown
## 🐛 Bug #XXX: Descriptive title

**Status:** 🔴 CRITICAL
**Discovered by:** `feature.feature:42` - Scenario name
**Date:** 2024-11-11
**Reproducible:** ✅ Yes

### Symptom
...

### Technical Stack Involved
...

### Root Cause
\`\`\`rust
// Buggy code
\`\`\`

### Applied Fix
\`\`\`rust
// Fixed code
\`\`\`
```

4. **Add a regression test** in the steps
5. **Reference** the bug in the Gherkin feature:

```gherkin
Scenario: Regression test Bug #XXX
  # BUG #XXX: Description
  # FIX: Commit hash
  When ...
  Then ...
```

## 📊 Consult the technical stack

### Complete documentation

See [`TESTING_STACK.md`](./TESTING_STACK.md) for:
- Test architecture
- Tested Lithair components
- Dependencies and versions
- Coverage metrics
- Debugging guide

### Bug history

See [`BUG_REPORTS.md`](./BUG_REPORTS.md) for:
- All discovered bugs
- Complete technical context
- Applied fixes
- Regression tests

## 🔍 Debugging

### Specific failing test

```bash
# See complete details
RUST_LOG=trace cargo test --test cucumber_tests -- features/my_feature.feature

# Keep temporary files
export LITHAIR_KEEP_TEMP=1
cargo test --test cucumber_tests

# Inspect files after
ls -la /tmp/lithair-test-*/
cat /tmp/lithair-test-*/events.raftlog | jq .
```

### Add a debug step

```rust
#[then(expr = "I debug the full state")]
async fn debug_full_state(world: &mut LithairWorld) {
    let articles = world.get_articles().await;
    let checksum = world.compute_memory_checksum().await;

    eprintln!("🐛 DEBUG STATE:");
    eprintln!("  Articles count: {}", articles.len());
    eprintln!("  Articles: {:#?}", articles);
    eprintln!("  Checksum: 0x{:08x}", checksum);

    // Dump files
    if let Some(dir) = world.temp_dir.lock().await.as_ref() {
        eprintln!("  Temp dir: {:?}", dir.path());
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let entry = entry.unwrap();
            eprintln!("    - {:?} ({} bytes)",
                entry.file_name(),
                entry.metadata().unwrap().len());
        }
    }
}
```

## 📈 Metrics & Reports

### Generate HTML report

```bash
# TODO: To implement with cucumber-html-formatter
cargo test --test cucumber_tests -- --format json > report.json
```

### Coverage statistics

See [`TESTING_STACK.md`](./TESTING_STACK.md#test-metrics) for:
- Coverage per component
- Execution time
- Success rate

## 🎯 Best Practices

### ✅ DO

- **Write real tests** with actual assertions
- **Document the stack** in comments
- **Track bugs** in BUG_REPORTS.md
- **Add regression tests** for each bug
- **Use TempDir** for test isolation
- **Calculate checksums** to verify integrity

### ❌ DON'T

- **No `println!()` alone** without assertions
- **No empty stubs** (always test for real)
- **No hardcoded files** (use TempDir)
- **No dependent tests** (complete isolation)
- **No secrets** in tests

## 🤝 Contributing

1. Create a branch `feature/test-my-module`
2. Add the `.feature` files + steps
3. Document in TESTING_STACK.md if new component
4. Validate that all tests pass
5. Create a PR with description of added tests

## 📚 Resources

- **Cucumber Book:** <https://cucumber.io/docs/guides/>
- **Lithair Docs:** `../docs/`
- **Rust async:** <https://tokio.rs/>
- **Event Sourcing:** Martin Fowler

---

**Maintainer:** Lithair Team
**Last update:** 2024-11-11
**Questions?** Open a GitHub issue
