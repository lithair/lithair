#!/usr/bin/env bash
# Bootstrap the Lithair dev environment. Idempotent — safe to re-run.
#
# Installs (only if missing):
#   - rustup + the pinned toolchain (rust-toolchain.toml picks the version)
#   - go-task (Taskfile runner) into ~/.local/bin
#   - cidx (containerized CI runner) — from a local clone if present, else via Go
#   - probatum (black-box check runner) — from a local clone if present, else a release binary
#
# Usage: ./scripts/setup.sh   (works without `task` installed — chicken-egg safe)
set -euo pipefail

BIN_DIR="$HOME/.local/bin"
CIDX_CLONE="$HOME/projects/cidx-org/cidx"
PROBATUM_CLONE="$HOME/projects/probatum-org/probatum"
mkdir -p "$BIN_DIR"
export PATH="$BIN_DIR:$PATH"   # use freshly installed tools within this run

log()  { printf '==> %s\n' "$*"; }
warn() { printf 'WARNING: %s\n' "$*" >&2; }

# ─── rustup ───
if command -v rustup >/dev/null 2>&1 || [ -x "$HOME/.cargo/bin/rustup" ]; then
    log "rustup already installed"
else
    log "installing rustup (toolchain pinned by rust-toolchain.toml)"
    curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs \
        | sh -s -- -y --default-toolchain none
fi
# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

# Make cargo available in future shells.
for profile in "$HOME/.profile" "$HOME/.bashrc"; do
    if [ -f "$profile" ] && ! grep -q '.cargo/env' "$profile"; then
        printf '\n. "$HOME/.cargo/env"\n' >> "$profile"
        log "added cargo env to $profile"
    fi
done

# ─── go-task ───
if command -v task >/dev/null 2>&1; then
    log "go-task already installed"
else
    log "installing go-task into $BIN_DIR"
    sh -c "$(curl -fsSL https://taskfile.dev/install.sh)" -- -d -b "$BIN_DIR"
fi

# ─── cidx ───
if [ -d "$CIDX_CLONE" ]; then
    log "building cidx from local clone ($CIDX_CLONE)"
    make -C "$CIDX_CLONE" build
    ln -sf "$CIDX_CLONE/bin/cidx" "$BIN_DIR/cidx"
elif command -v cidx >/dev/null 2>&1; then
    log "cidx already installed"
elif command -v go >/dev/null 2>&1; then
    log "installing cidx via go install"
    GOBIN="$BIN_DIR" go install github.com/cidx-org/cidx/cmd/cidx@latest
else
    warn "cidx not installed: no local clone at $CIDX_CLONE and Go is missing."
    warn "Install Go (https://go.dev/dl/) then re-run, or clone cidx to $CIDX_CLONE."
fi

# ─── probatum ───
if [ -d "$PROBATUM_CLONE" ]; then
    log "building probatum from local clone ($PROBATUM_CLONE)"
    (cd "$PROBATUM_CLONE" && cargo build --release)
    ln -sf "$PROBATUM_CLONE/target/release/probatum" "$BIN_DIR/probatum"
elif command -v probatum >/dev/null 2>&1; then
    log "probatum already installed"
else
    log "downloading probatum release binary"
    curl -fsSL -o "$BIN_DIR/probatum" \
        https://github.com/probatum-org/probatum/releases/latest/download/probatum-x86_64-linux
    chmod +x "$BIN_DIR/probatum"
fi

# ─── docker (required by cidx, not installed here) ───
if ! command -v docker >/dev/null 2>&1; then
    warn "Docker not found — cidx needs it to run CI containers ('task ci' / 'task pr')."
    warn "Install it from https://docs.docker.com/engine/install/ — everything else works without it."
fi

# ─── PATH sanity ───
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) warn "$BIN_DIR is not in your PATH — add: export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
esac

# ─── summary ───
echo
log "environment summary:"
ver() {
    if command -v "$1" >/dev/null 2>&1; then
        printf '    %-10s %s\n' "$1" "$("$@" 2>&1 | head -n1)"
    else
        printf '    %-10s MISSING\n' "$1"
    fi
}
ver rustup --version
ver cargo --version
ver task --version
ver cidx --version
ver probatum --version
ver docker --version
log "done."
