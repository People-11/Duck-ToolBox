#!/system/bin/sh
# KernelSU late-load mode replaces post-fs-data with this stage, so the early stage runs here.
MODDIR=${0%/*}

PIF_ENGINE_MODULE="$MODDIR/play-integrity"   "$MODDIR/play-integrity/pif-engine" stage early >/dev/null 2>&1
