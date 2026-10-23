#!/system/bin/sh
# Runs when the root manager removes the module (not on updates). Removes everything Duck
# ToolBox created, including issued keyboxes and key material in its data directory.

MODPATH=${0%/*}

# shellcheck source=util_functions.sh
. "$MODPATH/util_functions.sh"

remove_entry_links
rm -f /storage/emulated/0/stop-tspa-auto-target
rm -rf "$DATA_ROOT"
