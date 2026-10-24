#!/system/bin/sh
# Derived from Play Integrity Fork, osm0sis, chiteroman and contributors.
# Source: https://github.com/osm0sis/PlayIntegrityFork/tree/23ec56c2440c4fd1a5e893a23ef7861d6366a828
# SPDX-License-Identifier: GPL-3.0-only
# Modified 2026-10-23: independent engine packaging, bounded configuration and runtime operations.
# killpi.sh by osm0sis @ xda-developers
#
# Kill the Google Play services DroidGuard and Play Store processes
# (com.google.android.gms.unstable and com.android.vending)

if [ "$USER" != "root" -a "$(whoami 2>/dev/null)" != "root" ]; then
  echo "killpi: need root permissions";
  exit 1;
fi;

killall -v com.google.android.gms.unstable;
killall -v com.android.vending;
