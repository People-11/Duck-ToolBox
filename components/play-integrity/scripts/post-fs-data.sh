# Derived from Play Integrity Fork, osm0sis, chiteroman and contributors.
# Source: https://github.com/osm0sis/PlayIntegrityFork/tree/23ec56c2440c4fd1a5e893a23ef7861d6366a828
# SPDX-License-Identifier: GPL-3.0-only
# Modified 2026-10-24: independent engine packaging, bounded configuration and runtime operations.
MODPATH="$PIF_ENGINE_ROOT"
. $PIF_ENGINE_SCRIPTS/common_func.sh

# Magisk DenyList is managed by the CLI with a persistent restoration journal.
pif_running || return 0
if [ "$PIF_ENGINE_MODE" != "scripts-only" ]; then
    . "$PIF_ENGINE_SCRIPTS/common_setup.sh"
fi

# The shared prop-handler flag controls OEM sensitive properties, not ROM conflict masks.
[ ! -f /data/adb/disable_prop_handler ] || return 0

# Conditional early sensitive properties

# Samsung
resetprop_if_diff ro.boot.warranty_bit 0
resetprop_if_diff ro.vendor.boot.warranty_bit 0
resetprop_if_diff ro.vendor.warranty_bit 0
resetprop_if_diff ro.warranty_bit 0

# Realme
resetprop_if_diff ro.boot.realmebootstate green

# OnePlus
resetprop_if_diff ro.is_ever_orange 0

# Microsoft
for PROP in $(resetprop | grep -oE 'ro.*.build.tags'); do
    resetprop_if_diff $PROP release-keys
done

# Other
for PROP in $(resetprop | grep -oE 'ro.*.build.type'); do
    resetprop_if_diff $PROP user
done
resetprop_if_diff ro.adb.secure 1
if ! $SKIPDELPROP; then
    delprop_if_exist ro.boot.verifiedbooterror
    delprop_if_exist ro.boot.verifyerrorpart
fi
resetprop_if_diff ro.boot.veritymode.managed yes
resetprop_if_diff ro.debuggable 0
resetprop_if_diff ro.force.debuggable 0
resetprop_if_diff ro.secure 1
