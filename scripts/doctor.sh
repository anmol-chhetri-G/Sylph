#!/usr/bin/env bash
# Sylph environment check: is this machine able to run scripts/verify.sh?
# Prints one line per check; failures start with "error:" and say how to
# fix them. Exits non-zero if a required check failed. Read-only: it never
# installs, deletes or launches anything.
#
#   scripts/doctor.sh
set -u
cd "$(dirname "$0")/.."
failed=0
ok() { echo "ok     $1"; }
err() { echo "error: $1"; failed=1; }
warn() { echo "note:  $1"; }

# Rust: the pinned toolchain, complete (an interrupted rustup auto-install
# leaves one without cargo: "the 'cargo' binary ... is not applicable").
pin="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)"
if ! cargo --version >/dev/null 2>&1; then
  err "cargo does not run for toolchain $pin; repair it with: rustup toolchain uninstall $pin && rustup toolchain install $pin --component rustfmt,clippy"
elif [[ "$(rustc --version)" != "rustc $pin "* ]]; then
  err "rustc is $(rustc --version), rust-toolchain.toml pins $pin"
else
  ok "rust $pin with cargo"
fi
for tool in cargo-fmt cargo-clippy; do
  command -v "$tool" >/dev/null 2>&1 || err "$tool missing: rustup component add rustfmt clippy"
done

# Native libraries GPUI links against (AGENTS.md "System setup").
missing=()
for lib in xkbcommon xkbcommon-x11 fontconfig freetype2; do
  pkg-config --exists "$lib" 2>/dev/null || missing+=("$lib")
done
if ((${#missing[@]})); then
  err "no development files for: ${missing[*]} (Debian: apt install libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev libfreetype-dev; Arch: pacman -S libxkbcommon fontconfig freetype2)"
else
  ok "native libraries (xkbcommon, xkbcommon-x11, fontconfig, freetype2)"
fi

# GPU: the app draws with Vulkan. On Intel + NVIDIA laptops under Wayland,
# NVIDIA's driver can fail every surface, and the window cannot open. A
# note, not an error: verify.sh never opens a window.
if ! command -v vulkaninfo >/dev/null 2>&1 || [[ -z "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]]; then
  warn "GPU not checked (needs vulkaninfo from vulkan-tools, and a display)"
elif vulkaninfo --summary >/dev/null 2>&1; then
  ok "vulkan draws to this display"
elif VK_LOADER_DRIVERS_DISABLE='*nvidia*' vulkaninfo --summary >/dev/null 2>&1; then
  warn "vulkan fails with NVIDIA's driver and works without it; run the app as: VK_LOADER_DRIVERS_DISABLE='*nvidia*' cargo run -p sylph-desktop"
else
  warn "vulkan cannot draw to this display (vulkaninfo --summary fails), so the app cannot open its window"
fi

# Python: the bridge embeds the system python3, so the repo .venv must be
# built by that same version, and its site-packages must not be group- or
# world-writable (python_dirs refuses those, and debug exports then fail).
fix_venv="mv .venv target/venv-old && (umask 022; python3 -m venv .venv && .venv/bin/pip install -r python/requirements.txt)"
if ! command -v python3 >/dev/null 2>&1; then
  err "python3 missing"
else
  system="$(python3 -c 'import sys; print("%d.%d" % sys.version_info[:2])')"
  if [[ ! -x .venv/bin/python ]] || ! venv="$(.venv/bin/python -c 'import sys; print("%d.%d" % sys.version_info[:2])' 2>/dev/null)"; then
    err ".venv missing or broken (no working .venv/bin/python); rebuild: $fix_venv"
  elif [[ "$venv" != "$system" ]]; then
    err ".venv is Python $venv but python3 is $system; rebuild: $fix_venv"
  elif ! .venv/bin/python -c 'import fpdf, docx, markdown, uharfbuzz, lxml.etree' 2>/dev/null; then
    err ".venv lacks the exporters' packages: .venv/bin/pip install -r python/requirements.txt"
  else
    site=".venv/lib/python$venv/site-packages"
    if [[ -n "$(find "$site" "$(dirname "$site")" -maxdepth 0 -perm /022 2>/dev/null)" ]]; then
      err "$site or its parent is group/world-writable, so the bridge refuses it: chmod go-w $site $(dirname "$site")"
    else
      ok "python $system and .venv with the exporters"
    fi
  fi
fi
if (( ($(umask) & 020) == 0 )); then
  warn "umask $(umask): new directories are group-writable; create venvs with (umask 022; ...)"
fi

# Tools the export checks and the plan's manual checks use.
for tool in jq pdftotext; do
  command -v "$tool" >/dev/null 2>&1 || warn "$tool missing (optional; Debian: jq poppler-utils)"
done

exit $failed
