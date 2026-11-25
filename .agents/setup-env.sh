#!/usr/bin/env bash
# This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
# If a copy of the MPL was not distributed with this file, You can obtain one at
# https://mozilla.org/MPL/2.0/.
#
# Idempotent, non-interactive cloud environment setup for Fine Print.
# Invoked automatically on fresh cloud VMs via .agents/environment.json.
#
# Installs and verifies the Linux tooling needed across the repository:
#   - Pinned Rust toolchain from rust-toolchain.toml + rustfmt, clippy,
#     llvm-tools, and the wasm32-unknown-unknown target
#   - cargo-llvm-cov pinned to match .github/workflows/ci.yml (for
#     `cargo xtask coverage` 90% / 75% per-crate line-coverage floors)
#   - Linux build and script prerequisites (build-essential, pkg-config,
#     curl, ca-certificates, python3, jq, git)
#   - Pre-fetched locked Cargo crate caches for the root workspace and
#     spikes/bindings
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$REPO_ROOT"

CARGO_BIN_DIR="${CARGO_HOME:-$HOME/.cargo}/bin"
mkdir -p "$CARGO_BIN_DIR"
export PATH="$CARGO_BIN_DIR:$PATH"

# ---------------------------------------------------------------------------
# 1. Non-interactive ~/.bashrc persistence
# ---------------------------------------------------------------------------
# Non-interactive command shells source ~/.bashrc via BASH_ENV; ensure Cargo's
# bin directory is unconditionally exported on line 1 (before any
# interactive-shell guard or conditional) without modifying or deleting any
# existing lines or shell constructs in ~/.bashrc.
touch "$HOME/.bashrc"

PATH_EXPORT="export PATH=\"${CARGO_BIN_DIR}:\$PATH\""
FIRST_LINE=$(head -n 1 "$HOME/.bashrc" || true)

if [ "$FIRST_LINE" != "$PATH_EXPORT" ]; then
  TMP_BASHRC=$(mktemp "$HOME/.bashrc.tmp.XXXXXX")
  trap 'rm -f "$TMP_BASHRC"' EXIT
  cp -p "$HOME/.bashrc" "$TMP_BASHRC"
  {
    printf '%s\n' "$PATH_EXPORT"
    cat "$HOME/.bashrc"
  } > "$TMP_BASHRC"
  mv "$TMP_BASHRC" "$HOME/.bashrc"
  trap - EXIT
fi

# ---------------------------------------------------------------------------
# 2. Linux system packages (build tools, curl for `cargo xtask cr fetch`,
#    Python 3 for `scripts/req_coverage.py`, jq for agent PR scripts)
# ---------------------------------------------------------------------------
if command -v apt-get >/dev/null 2>&1; then
  REQUIRED_APT_PKGS=(
    build-essential
    pkg-config
    curl
    ca-certificates
    python3
    jq
    git
  )

  MISSING_PKGS=()
  for pkg in "${REQUIRED_APT_PKGS[@]}"; do
    PKG_STATUS=$(dpkg-query -W -f='${Status}' "$pkg" 2>/dev/null || true)
    if [ "$PKG_STATUS" != "install ok installed" ]; then
      MISSING_PKGS+=("$pkg")
    fi
  done

  if [ "${#MISSING_PKGS[@]}" -gt 0 ]; then
    SUDO=""
    if [ "$(id -u)" -ne 0 ]; then
      SUDO="sudo"
    fi
    echo "==> Installing missing apt packages: ${MISSING_PKGS[*]}"
    $SUDO apt-get update
    DEBIAN_FRONTEND=noninteractive $SUDO apt-get install -y --no-install-recommends "${MISSING_PKGS[@]}"
  else
    echo "==> All required apt packages are already installed."
  fi
fi

# ---------------------------------------------------------------------------
# 3. Pinned Rust toolchain from rust-toolchain.toml + llvm-tools + wasm32
# ---------------------------------------------------------------------------
RUST_VERSION=$(sed -n '/^\[toolchain\]/,/^\[/{s/^channel *= *"\([^"]*\)".*/\1/p}' "$REPO_ROOT/rust-toolchain.toml" | head -n 1)
if [ -z "$RUST_VERSION" ]; then
  echo "error: could not parse [toolchain] channel from rust-toolchain.toml" >&2
  exit 1
fi

if ! command -v rustup >/dev/null 2>&1; then
  echo "error: rustup not found on PATH ($PATH)" >&2
  exit 1
fi

INSTALLED_TOOLCHAINS=$(RUSTUP_AUTO_INSTALL=0 rustup toolchain list 2>/dev/null || true)
NEEDS_TOOLCHAIN=0
if ! grep -q "^${RUST_VERSION}-" <<<"$INSTALLED_TOOLCHAINS"; then
  NEEDS_TOOLCHAIN=1
else
  CURRENT_RUSTC=$(RUSTUP_AUTO_INSTALL=0 rustc --version 2>/dev/null | awk '{print $2}' || true)
  if [ "$CURRENT_RUSTC" != "$RUST_VERSION" ]; then
    NEEDS_TOOLCHAIN=1
  else
    SYSROOT=$(RUSTUP_AUTO_INSTALL=0 rustc --print sysroot)
    if [ ! -d "$SYSROOT/lib/rustlib/wasm32-unknown-unknown" ]; then
      NEEDS_TOOLCHAIN=1
    fi
    INSTALLED_COMPONENTS=$(RUSTUP_AUTO_INSTALL=0 rustup component list --installed 2>/dev/null || true)
    for comp in rustfmt clippy llvm-tools; do
      if ! grep -q "^${comp}" <<<"$INSTALLED_COMPONENTS"; then
        NEEDS_TOOLCHAIN=1
      fi
    done
  fi
fi

if [ "$NEEDS_TOOLCHAIN" -eq 1 ]; then
  echo "==> Installing Rust $RUST_VERSION (rustfmt, clippy, llvm-tools, wasm32-unknown-unknown)..."
  rustup toolchain install "$RUST_VERSION" --profile minimal \
    --component rustfmt,clippy,llvm-tools \
    --target wasm32-unknown-unknown
else
  echo "==> Rust $RUST_VERSION with required components and wasm32 target is already active."
fi

INSTALLED_RUSTC=$(rustc --version | awk '{print $2}')
if [ "$INSTALLED_RUSTC" != "$RUST_VERSION" ]; then
  echo "error: active rustc is $INSTALLED_RUSTC, expected $RUST_VERSION" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# 4. cargo-llvm-cov (pinned to match .github/workflows/ci.yml)
# ---------------------------------------------------------------------------
LLVM_COV_VERSION=$(sed -n 's/.*cargo install cargo-llvm-cov --locked --version \([0-9][0-9.]*\).*/\1/p' "$REPO_ROOT/.github/workflows/ci.yml" | head -n 1)
LLVM_COV_VERSION="${LLVM_COV_VERSION:-0.9.1}"

CURRENT_LLVM_COV=$(cargo llvm-cov --version 2>/dev/null | awk '{print $2}' || true)
if [ "$CURRENT_LLVM_COV" != "$LLVM_COV_VERSION" ]; then
  echo "==> Installing cargo-llvm-cov $LLVM_COV_VERSION..."
  ARCH=$(uname -m)
  case "$ARCH" in
    x86_64)        LLVM_COV_TARGET="x86_64-unknown-linux-gnu" ;;
    aarch64|arm64) LLVM_COV_TARGET="aarch64-unknown-linux-gnu" ;;
    *)             LLVM_COV_TARGET="" ;;
  esac

  INSTALLED_PREBUILT=0
  if [ -n "$LLVM_COV_TARGET" ]; then
    TMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT
    URL="https://github.com/taiki-e/cargo-llvm-cov/releases/download/v${LLVM_COV_VERSION}/cargo-llvm-cov-${LLVM_COV_TARGET}.tar.gz"
    if curl -fsSL --retry 3 --retry-delay 2 "$URL" -o "$TMP_DIR/cargo-llvm-cov.tar.gz" \
      && tar -xzf "$TMP_DIR/cargo-llvm-cov.tar.gz" -C "$TMP_DIR" \
      && [ -f "$TMP_DIR/cargo-llvm-cov" ]; then
      install -m 0755 "$TMP_DIR/cargo-llvm-cov" "$CARGO_BIN_DIR/cargo-llvm-cov"
      INSTALLED_PREBUILT=1
    fi
    rm -rf "$TMP_DIR"
    trap - EXIT
  fi

  if [ "$INSTALLED_PREBUILT" -eq 0 ]; then
    cargo install --locked --version "$LLVM_COV_VERSION" cargo-llvm-cov
  fi

  INSTALLED_LLVM_COV=$(cargo llvm-cov --version | awk '{print $2}')
  if [ "$INSTALLED_LLVM_COV" != "$LLVM_COV_VERSION" ]; then
    echo "error: installed cargo-llvm-cov is $INSTALLED_LLVM_COV, expected $LLVM_COV_VERSION" >&2
    exit 1
  fi
else
  echo "==> cargo-llvm-cov $LLVM_COV_VERSION is already installed."
fi

# ---------------------------------------------------------------------------
# 5. Pre-fetch locked Cargo crate dependencies
# ---------------------------------------------------------------------------
echo "==> Fetching locked Cargo dependencies..."
cargo fetch --locked --manifest-path "$REPO_ROOT/Cargo.toml"
cargo fetch --locked --manifest-path "$REPO_ROOT/spikes/bindings/Cargo.toml"

# ---------------------------------------------------------------------------
# 5b. Commit attribution hooks (credit the human contributor on agent commits)
# ---------------------------------------------------------------------------
echo "==> Enabling git hooks (.githooks/)"
"$REPO_ROOT/scripts/install-git-hooks.sh" || echo "WARN: scripts/install-git-hooks.sh failed" >&2

# ---------------------------------------------------------------------------
# 6. Summary
# ---------------------------------------------------------------------------
echo "==> Environment ready:"
echo "    rustc:          $(rustc --version)"
echo "    cargo:          $(cargo --version)"
echo "    cargo-llvm-cov: $(cargo llvm-cov --version)"
echo "    wasm32 target:  $(rustc --print sysroot)/lib/rustlib/wasm32-unknown-unknown"
echo "    cc:             $(cc --version | head -n 1)"
echo "    python3:        $(python3 --version)"
echo "    jq:             $(jq --version)"
echo "    curl:           $(curl --version | head -n 1)"
