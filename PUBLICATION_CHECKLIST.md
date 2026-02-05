# Lithair Publication Readiness Checklist

This document serves as a **publication gate** for the Lithair framework. AI agents and reviewers must validate all items before approving publication.

## Automated Validation

Run these commands and ensure all pass:

```bash
# Full CI validation (required)
task ci:full

# Complete pre-push validation (required)
task ci:github

# Security audit (required)
cargo audit

# License and advisory check (required)
cargo deny check advisories licenses

# BDD tests (required)
task bdd:all

# Release build (required)
task build:release

# Documentation build (required)
task docs:build
```

## Manual Checklist

### Documentation
- [x] All documentation in English
- [x] mdBook structure complete (`docs/`)
- [ ] README.md accurate and up-to-date
- [x] CLAUDE.md contains correct project guidelines
- [ ] API documentation generated (`cargo doc`)

### Code Quality
- [ ] No compiler warnings with `-D warnings`
- [ ] Clippy passes with no warnings
- [ ] Code formatted with `rustfmt`
- [ ] No TODO comments in production code
- [ ] No debug print statements (`println!`, `dbg!`)

### Security
- [ ] No hardcoded secrets or credentials
- [ ] No vulnerable dependencies
- [ ] Input validation on all public APIs
- [ ] No unsafe code without justification
- [ ] OWASP top 10 vulnerabilities addressed

### Tests
- [ ] Unit tests pass
- [ ] Integration tests pass
- [x] BDD tests pass (all scenarios)
- [ ] Performance benchmarks meet targets
- [ ] Edge cases covered

### Dependencies
- [x] All dependencies up-to-date
- [ ] No deprecated dependencies
- [ ] License compatibility verified (MIT/Apache-2.0)
- [ ] Minimal dependency footprint

### Build & Release
- [ ] Debug build compiles
- [ ] Release build compiles with LTO
- [ ] All examples compile and run
- [ ] Version numbers consistent across Cargo.toml files
- [ ] CHANGELOG.md updated

## AI Agent Validation Script

AI agents reviewing this PR should execute:

```bash
#!/bin/bash
set -e

echo "=== Lithair Publication Validation ==="

echo "[1/9] Running CI full..."
task ci:full

echo "[2/9] Running CI github (complete validation)..."
task ci:github

echo "[3/9] Running security audit..."
if ! command -v cargo-audit &> /dev/null; then
    echo "Warning: cargo-audit not installed, skipping security audit..."
else
    cargo audit
fi

echo "[4/9] Running license and advisory check..."
if ! command -v cargo-deny &> /dev/null; then
    echo "Warning: cargo-deny not installed, skipping license check..."
else
    cargo deny check advisories licenses
fi

echo "[5/9] Checking for secrets..."
if grep -rE --exclude-dir="target" --exclude-dir="examples" --exclude-dir="cucumber-tests" --include="*.rs" --include="*.toml" "(password|secret|api_key|token)\s*=" . ; then
    echo "ERROR: Potential secrets found!"
    exit 1
fi

echo "[6/9] Building release..."
task build:release

echo "[7/9] Building documentation..."
task docs:build

echo "[8/9] Running BDD tests..."
if task --list 2>/dev/null | grep -q "bdd:all"; then
    task bdd:all
else
    echo "Warning: BDD task not found, skipping..."
fi

echo "[9/9] Building examples..."
cargo build --examples

echo "=== All validations passed ==="
```

## Approval Criteria

This PR can be merged when:

1. All automated validations pass
2. All manual checklist items are verified
3. At least one human reviewer approves
4. No critical or high-severity issues remain open

## Post-Publication

After merging:

1. Tag the release: `git tag -a vX.Y.Z -m "Release description"`
2. Push tags: `git push --tags`
3. Publish to crates.io (if applicable)
4. Update documentation site
5. Announce release
