#!/system/bin/sh
# post-fs-data mode: blocking, before Zygote. Play Integrity's early stage runs here.
MODDIR=${0%/*}

PIF_ENGINE_MODULE="$MODDIR/play-integrity"   "$MODDIR/play-integrity/pif-engine" stage early >/dev/null 2>&1
