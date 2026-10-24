#!/bin/sh
# Derived from Play Integrity Fork, osm0sis, chiteroman and contributors.
# Source: https://github.com/osm0sis/PlayIntegrityFork/tree/23ec56c2440c4fd1a5e893a23ef7861d6366a828
# SPDX-License-Identifier: GPL-3.0-only
# Modified 2026-10-24: independent engine packaging, bounded configuration and runtime operations.
# Modified 2026-10-24: delegate parsing to the GPL CLI; never evaluate file values.
set -eu
engine=${PIF_ENGINE_BIN:-/data/adb/modules/duck-toolbox/play-integrity/pif-engine}
advanced= force= format=prop
while [ "$#" -gt 0 ]; do
  case "$1" in
    -i|--install) shift ;;
    -a|--advanced) advanced=--advanced; shift ;;
    -f|-o|--force|--override) force=--force; shift ;;
    -j|--json) format=json; shift ;;
    -p|--prop) format=prop; shift ;;
    -h|--help) printf '%s\n' 'migrate.sh [-a] [-f|-o] [-p|-j] input [output]'; exit 0 ;;
    -*) printf '%s\n' "Unsupported option: $1" >&2; exit 2 ;;
    *) break ;;
  esac
done
[ "$#" -ge 1 ] || { printf '%s\n' 'An input file is required' >&2; exit 2; }
source=$1
output=${2:-$(dirname "$source")/custom.pif.$format}
exec "$engine" migrate-file "$source" "$output" --format "$format" $advanced $force
