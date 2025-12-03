# Lithair CI Workflow Guide

## 🎯 Quick Reference

**During Development:**
```bash
task ci:full    # Fast code quality check (~2-3min)
```

**Before Commit/Push:**
```bash
task ci:github  # Complete GitHub-equivalent validation (~10-15min)
```

## 📋 CI Task Breakdown

### `task ci:full` - Core CI Pipeline
**Purpose:** Fast development feedback loop
**Duration:** ~2-3 minutes
**What it does:**
- ✅ Code formatting check (`cargo fmt --check`)
- ✅ Build with deny warnings (`RUSTFLAGS="-D warnings" cargo build`)
- ✅ Clippy analysis with deny warnings (`cargo clippy -- -D warnings`)
- ✅ Test suite execution (`cargo test`)

**When to use:** During active development to catch code quality issues quickly

### `task ci:smoke` - Functional Validation
**Purpose:** End-to-end functional testing
**Duration:** ~5-10 minutes
**What it does:**
- 🔥 Firewall demo validation (configurable + declarative)
- 🛡️ HTTP hardening demo
- 📊 Frontend benchmark (static + API)
- ⚡ Stateless performance benchmarks
- 🗜️ GZIP negotiation tests

**When to use:** Rarely standalone - included in `ci:github`

### `task ci:github` - Complete GitHub Pipeline
**Purpose:** Pre-commit validation that mirrors GitHub Actions exactly
**Duration:** ~10-15 minutes
**What it does:**
- 🎯 **Everything from `ci:full`** (code quality)
- 🎯 **Everything from `ci:smoke`** (functional validation)

**When to use:** **ALWAYS before committing/pushing** to ensure GitHub CI will pass

## 🚀 Development Workflow

### 1. Active Development Cycle
```bash
# Make changes
git add .
task ci:full        # Quick validation (~2-3min)
# Fix any issues, repeat until clean
```

### 2. Pre-Commit Validation
```bash
# Ready to commit
task ci:github      # Complete validation (~10-15min)
git commit -m "feat: implement feature X"
git push
```

### 3. Emergency/Hotfix
```bash
# For urgent fixes, minimum validation
task ci:full        # At least ensure code quality
git commit -m "fix: critical bug"
git push
# Monitor GitHub Actions for any smoke test failures
```

## ✅ Guarantees

- **If `task ci:github` passes locally → GitHub Actions will pass**
- **If `task ci:full` passes → Code quality is clean**
- **Local and GitHub environments are identical**

## 🔧 CI Architecture

```
task ci:github
├── task ci:full (Code Quality)
│   ├── Format check
│   ├── Build with warnings as errors
│   ├── Clippy analysis
│   └── Test suite
└── task ci:smoke (Functional Validation)
    ├── Firewall demos
    ├── HTTP hardening
    ├── Frontend benchmarks
    └── Performance tests
```

## 📊 Time Investment vs Risk

| Task | Time | Risk Caught | When to Use |
|------|------|-------------|-------------|
| `ci:full` | 2-3min | Code quality issues | Development iterations |
| `ci:github` | 10-15min | All possible CI failures | Before commit/push |

## 💡 Pro Tips

1. **Run `ci:full` frequently** during development - it's fast and catches most issues
2. **Always run `ci:github` before pushing** - prevents failed GitHub Actions
3. **If `ci:smoke` fails locally**, debug with individual demo commands
4. **Use `task help`** to see all available CI tasks and options

## 🚨 Common Issues

**Problem:** `ci:full` passes but `ci:smoke` fails
**Solution:** Functional issue in demos - check endpoint mismatches or port conflicts

**Problem:** GitHub Actions fail but local CI passed
**Solution:** Environment difference - check dependencies or timing issues

**Problem:** CI takes too long
**Solution:** Use `ci:full` for development, `ci:github` only for final validation