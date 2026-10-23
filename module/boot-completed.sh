#!/system/bin/sh
# Runs once boot has completed and `pm` works (KernelSU and APatch call it directly; on
# Magisk service.sh does). Nothing here may block or fail the boot.

MODPATH=${0%/*}
DUCKCTL="$MODPATH/bin/duckctl.sh"
TSPA_DIR="/data/adb/modules/tsupport-advance"
TSPA_FLAG="/storage/emulated/0/stop-tspa-auto-target"
SETTLE=10

watch_packages() {
  inotifyd - /data/system:wy 2>/dev/null | while read -r _ _ name; do
    [ "$name" = packages.list ] || continue
    deadline=$(($(date +%s) + SETTLE))
    # shellcheck disable=SC3045 # Boot scripts run in BusyBox ash, which has read -t.
    while remaining=$((deadline - $(date +%s))) && [ "$remaining" -gt 0 ] &&
      read -r -t "$remaining" _ _ name; do
      [ "$name" = packages.list ] && deadline=$(($(date +%s) + SETTLE))
    done
    "$DUCKCTL" tricky-store auto-apply >/dev/null 2>&1
  done
}

# Play Integrity late stage: sensitive properties the system sets after post-fs-data.
PIF_ENGINE_MODULE="$MODPATH/play-integrity" "$MODPATH/play-integrity/pif-engine" stage late >/dev/null 2>&1

(
  "$DUCKCTL" tricky-store auto-apply --refresh >/dev/null 2>&1
  command -v inotifyd >/dev/null 2>&1 && watch_packages
  while sleep 600; do
    "$DUCKCTL" tricky-store auto-apply >/dev/null 2>&1
  done
) &

(
  while true; do
    "$DUCKCTL" renew >/dev/null 2>&1
    sleep 1800
  done
) &

# TSupport-Advance rewrites the target list on its own unless this flag exists (as Tricky
# Addon does). Shared storage appears only after the user unlocks the device.
tries=0
until [ -d /storage/emulated/0/Android ] || [ "$tries" -ge 90 ]; do
  sleep 2
  tries=$((tries + 1))
done
if [ -d "$TSPA_DIR" ]; then
  touch "$TSPA_FLAG" 2>/dev/null || true
else
  rm -f "$TSPA_FLAG"
fi
