# SPDX-License-Identifier: GPL-3.0-only
# Modified 2026-10-24: acknowledge only the stop revision selected by an explicit enable.
pif_running() {
  if [ -n "${PIF_ENGINE_ROOT:-}" ]; then
    [ ! -e "$PIF_ENGINE_ROOT/disable" ] && [ ! -e "$PIF_ENGINE_ROOT/remove" ] || return 1
  fi
  [ -e "$PIF_ENGINE_DATA/disabled" ] || return 0
  [ -s "$PIF_ENGINE_DATA/disabled" ] || return 1
  [ -f "$PIF_ENGINE_DATA/enabled-token" ] || return 1
  cmp -s "$PIF_ENGINE_DATA/disabled" "$PIF_ENGINE_DATA/enabled-token"
}
