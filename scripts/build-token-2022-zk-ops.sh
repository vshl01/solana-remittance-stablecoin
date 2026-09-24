#!/usr/bin/env bash
# Rebuilds tests/fixtures/spl_token_2022_zk_ops.so.
#
# LiteSVM ships the mainnet Token-2022 v10.0.0 binary, which is compiled
# without the `zk-ops` feature: Deposit, ApplyPendingBalance, Withdraw and
# Transfer(WithFee) all return InvalidInstructionData. This builds the same
# v10.0.0 source with its default features (which include `zk-ops`) so the
# confidential lifecycle can run in tests.
set -euo pipefail

VERSION="10.0.0"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${OUT:-$ROOT/programs/solana-remittance-stablecoin/tests/fixtures/spl_token_2022_zk_ops.so}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

cd "$WORK"
curl -sSfL "https://static.crates.io/crates/spl-token-2022/spl-token-2022-$VERSION.crate" | tar xz
cd "spl-token-2022-$VERSION"

# Only the on-chain program is needed: drop test/bench targets, dev-deps and
# the optional serde feature so the SBF toolchain resolves less.
python3 - Cargo.toml <<'EOF'
import re, sys
path = sys.argv[1]
parts = re.split(r'(?m)^(?=\[)', open(path).read())
drop = ('[dev-dependencies', '[[test]]', '[[bench]]',
        '[dependencies.serde]', '[dependencies.serde_with]', '[dependencies.base64]')
text = ''.join(p for p in parts if not p.startswith(drop))
text = re.sub(r'(?ms)^serde-traits = \[.*?\]\n', '', text)
open(path, 'w').write(text)
EOF
LOCK="$ROOT/scripts/token-2022-zk-ops.Cargo.lock"
if [ -f "$LOCK" ]; then
  cp "$LOCK" Cargo.lock
else
  rm -f Cargo.lock
  cargo generate-lockfile
  # v10.0.0 was built against spl-token-group-interface 0.7.1 (0.7.2 changed a type).
  cargo update -p spl-token-group-interface --precise 0.7.1
  cp Cargo.lock "$LOCK"
fi

cargo build-sbf --tools-version v1.52
cp target/deploy/spl_token_2022.so "$OUT"
shasum -a 256 "$OUT"
