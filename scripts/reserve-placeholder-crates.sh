#!/usr/bin/env bash
#
# Reserve qli-* support crate names on crates.io by publishing empty 0.0.0
# placeholders. Idempotent: already-published crates are skipped, so re-runs
# resume from where you left off.
#
# Why: locks the qli-* prefix while the main `qli` name is reclaimed via the
# crates.io inactive-owner process.
#
# Prerequisites:
#   - Verified email on crates.io (Settings → Email).
#   - `cargo login <token>` run with a token from https://crates.io/me.
#
# Rate limit: crates.io limits new-crate publishes (~1 per 10 minutes for
# unverified accounts; faster for verified). On rate-limit failure the script
# exits; wait, then re-run — idempotency handles resumption.

set -euo pipefail

CRATES=(
  qli-core
  qli-ext
  qli-lang
  qli-lang-python
  qli-lang-typescript
  qli-lang-csharp
  qli-lang-angular
  qli-outputs
  qli-lsp
  qli-scip
  qli-analyzers
)

REPO_URL="https://github.com/QLangstaff/qli"
LICENSE="MIT"
DESCRIPTION="Placeholder; reserved for the qli polyglot CLI project."

WORK_DIR="$(mktemp -d -t qli-reserve.XXXXXX)"
trap 'rm -rf "$WORK_DIR"' EXIT
echo "Working in $WORK_DIR"
echo

for name in "${CRATES[@]}"; do
  echo "─── $name ───"

  if cargo search --limit 1 "$name" 2>/dev/null | grep -qE "^${name} "; then
    echo "  already published — skipping"
    continue
  fi

  crate_dir="$WORK_DIR/$name"
  mkdir -p "$crate_dir/src"

  cat > "$crate_dir/Cargo.toml" <<EOF
[package]
name = "$name"
version = "0.0.0"
edition = "2021"
description = "$DESCRIPTION"
license = "$LICENSE"
repository = "$REPO_URL"
readme = "README.md"
EOF

  cat > "$crate_dir/src/lib.rs" <<EOF
//! Placeholder reservation for the qli polyglot CLI project.
//! See <$REPO_URL> for the real implementation.
EOF

  cat > "$crate_dir/README.md" <<EOF
# $name

Placeholder; this crate name is reserved for the [qli]($REPO_URL)
polyglot CLI project. Real content will land in a future release.
EOF

  if (cd "$crate_dir" && cargo publish --allow-dirty); then
    echo "  published"
  else
    echo "  publish failed for $name"
    echo "  if this was a rate limit, wait 10+ minutes and re-run the script."
    exit 1
  fi
done

echo
echo "All qli-* placeholder crates reserved on crates.io."
