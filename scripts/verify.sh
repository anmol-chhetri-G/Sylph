#!/usr/bin/env bash
# Sylph verification ladder (docs/improvement-plan.md §3), usable by any
# agent or by hand. Runs every stage and prints PASS/FAIL per stage; exits
# non-zero if any stage failed. Never launches the GUI and never touches git.
#
#   scripts/verify.sh           V0 + V1 + V2 + headless export proofs
#   scripts/verify.sh python    also the Python bridge suite (needs .venv)
set -u
cd "$(dirname "$0")/.."
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT
failed=0
stage() {
  local name="$1"; shift
  if "$@" >"$out/$name.log" 2>&1; then
    echo "PASS  $name"
  else
    echo "FAIL  $name"
    grep -E "^(error|warning)|FAILED|panicked|left:|right:|Diff in" "$out/$name.log" | head -30
    failed=1
  fi
}

stage fmt cargo fmt --all -- --check
stage clippy cargo clippy --workspace --all-targets -- -D warnings
stage test-core-storage cargo test -p sylph-core -p sylph-storage
stage test-desktop cargo test -p sylph-desktop
if [[ "${1:-}" == python ]]; then
  stage clippy-python cargo clippy -p sylph-py-bridge --all-targets --features python-tests -- -D warnings
  stage test-python cargo test -p sylph-py-bridge --features python-tests
fi

# Headless export proofs: the CLI must export the kitchen-sink fixture.
if cargo build -q -p sylph-desktop >"$out/build.log" 2>&1; then
  for fmt in pdf docx md; do
    if ./target/debug/sylph-desktop "--export-$fmt" fixtures/kitchen-sink.md "$out/k.$fmt" \
        >"$out/export-$fmt.log" 2>&1 && [[ -s "$out/k.$fmt" ]]; then
      echo "PASS  export-$fmt"
    else
      echo "FAIL  export-$fmt"; tail -5 "$out/export-$fmt.log"; failed=1
    fi
  done
else
  echo "FAIL  build"; tail -20 "$out/build.log"; failed=1
fi

[[ $failed -eq 0 ]] && echo "== ALL PASS" || echo "== FAILED"
exit $failed
