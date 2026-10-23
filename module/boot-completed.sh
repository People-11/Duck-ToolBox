#!/system/bin/sh
# Runs once boot has completed and `pm` works (KernelSU and APatch call it directly; on
# Magisk service.sh does). Nothing here may block or fail the boot.

MODPATH=${0%/*}
DUCKCTL="$MODPATH/bin/duckctl.sh"
TSPA_DIR="/data/adb/modules/tsupport-advance"
TSPA_FLAG="/storage/emulated/0/stop-tspa-auto-target"

# Play Integrity late stage: sensitive properties the system sets after post-fs-data.
PIF_ENGINE_MODULE="$MODPATH/play-integrity"   "$MODPATH/play-integrity/pif-engine" stage late >/dev/null 2>&1

# Every 10 minutes: renew the keybox when it expires within a day (renew also refreshes
# module.prop) and target newly installed apps. `sleep` stops while the device is in deep
# sleep, so checks follow the time it is awake, which is when attestation is used.
(
  "$DUCKCTL" tricky-store auto-apply --refresh >/dev/null 2>&1
  while true; do
    "$DUCKCTL" renew >/dev/null 2>&1
    sleep 600
    "$DUCKCTL" tricky-store auto-apply >/dev/null 2>&1
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
