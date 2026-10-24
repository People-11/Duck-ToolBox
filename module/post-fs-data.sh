#!/system/bin/sh
# post-fs-data mode: blocking, before Zygote (KernelSU module guide). Only the early prop
# pass runs here.
MODDIR=${0%/*}

sh "$MODDIR/prop.sh"
if [ -x "$MODDIR/play-integrity/pif-engine" ]; then
  PIF_ENGINE_MODULE="$MODDIR/play-integrity" \
    "$MODDIR/play-integrity/pif-engine" stage early >/dev/null 2>&1 \
    || sh "$MODDIR/prop.sh" --fallback
fi
