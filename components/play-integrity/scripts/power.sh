# SPDX-License-Identifier: GPL-3.0-only
# Modified 2026-10-24: acknowledge only the stop revision selected by an explicit enable.
# Modified 2026-10-26: only the root manager's disable/remove flags stop the engine.
pif_running() {
  [ -n "${PIF_ENGINE_ROOT:-}" ] || return 0
  [ ! -e "$PIF_ENGINE_ROOT/disable" ] && [ ! -e "$PIF_ENGINE_ROOT/remove" ]
}
