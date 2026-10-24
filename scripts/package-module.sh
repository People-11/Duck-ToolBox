#!/usr/bin/env bash
# Packages the Duck ToolBox module directory into a distributable zip.
# Equivalent of scripts/package-module.ps1 for Linux/macOS.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BINARY_PATH="${1:-target/aarch64-linux-android/release/duckd}"
DIST_DIR="${2:-dist}"

BINARY_FULL="${REPO_ROOT}/${BINARY_PATH}"
MODULE_DIR="${REPO_ROOT}/module"
MODULE_PROP="${MODULE_DIR}/module.prop"
DIST_ROOT="${REPO_ROOT}/${DIST_DIR}"

if [[ ! -f "${BINARY_FULL}" ]]; then
    echo >&2 "Android backend binary not found at '${BINARY_FULL}'. Build it first or pass the path as \$1."
    exit 1
fi

if [[ ! -f "${MODULE_DIR}/webroot/index.html" ]]; then
    echo >&2 "WebUI build output is missing at '${MODULE_DIR}/webroot/index.html'. Run 'pnpm build' in ui first."
    exit 1
fi

VERSION="$(grep -m1 '^version=' "${MODULE_PROP}" | cut -d= -f2- | tr -d '[:space:]')"
if [[ -z "${VERSION}" ]]; then
    echo >&2 "Could not read version from '${MODULE_PROP}'."
    exit 1
fi

mkdir -p "${DIST_ROOT}"

cp "${REPO_ROOT}/README.md"    "${MODULE_DIR}/README.md"
cp "${REPO_ROOT}/CHANGELOG.md" "${MODULE_DIR}/CHANGELOG.md"
cp "${BINARY_FULL}"            "${MODULE_DIR}/bin/duckd"

ARCHIVE="${DIST_ROOT}/duck-toolbox-${VERSION}.zip"
rm -f "${ARCHIVE}"

(cd "${MODULE_DIR}" && zip -r -9 "${ARCHIVE}" .)

echo "Module archive created at ${ARCHIVE}"
