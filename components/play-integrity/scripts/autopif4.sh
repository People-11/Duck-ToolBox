#!/system/bin/sh
# Derived from Play Integrity Fork, osm0sis, chiteroman and contributors.
# Source: https://github.com/osm0sis/PlayIntegrityFork/tree/23ec56c2440c4fd1a5e893a23ef7861d6366a828
# SPDX-License-Identifier: GPL-3.0-only
# Modified 2026-10-24: independent engine packaging, bounded configuration and runtime operations.

if [ "$USER" != "root" -a "$(whoami 2>/dev/null)" != "root" ]; then
  echo "autopif4: need root permissions"; exit 1;
fi;
case "$HOME" in
  *termux*) echo "autopif4: need su root environment"; exit 1;;
esac;

until [ -z "$1" ]; do
  case "$1" in
    -h|--help|help) echo "sh autopif4.sh [-a|-s] [-m]"; exit 0;;
    -a|--advanced|advanced) ARGS="-a"; shift;;
    -s|--strong|strong) FORCE_STRONG=1; shift;;
    -m|--match|match) FORCE_MATCH=1; shift;;
    *) break;;
  esac;
done;

echo "Pixel Canary pif.prop generator script \
  \n  by osm0sis @ xda-developers";

case "$0" in
  *.sh) DIR="$0";;
  *) DIR="$(lsof -p $$ 2>/dev/null | grep -o '/.*autopif4.sh$')";;
esac;
DIR=$(dirname "$(readlink -f "$DIR")");

item() { echo "\n- $*"; }
die() { echo "\nError: $*!"; exit 1; }
die_bb() { die "$*, install busybox"; }
warn() { echo "\nWarning: $*!"; }

wget() { "$PIF_ENGINE_BIN" fetch "$@"; }

find_busybox() {
  for path in /data/adb/ksu/bin/busybox /data/adb/ap/bin/busybox /data/adb/magisk/busybox; do
    if [ -x "$path" ]; then BUSYBOX="$path"; return 0; fi;
  done;
  return 1;
}

if date -D '%s' -d "$(date '+%s')" 2>&1 | grep -qE "bad date|invalid option"; then
  if ! find_busybox; then
    die_bb "date broken";
  else
    date() { $BUSYBOX date "$@"; }
  fi;
fi;

if ! echo "A\nB" | grep -m1 -A1 "A" | grep -q "B"; then
  if ! find_busybox; then
    die_bb "grep broken";
  else
    grep() { $BUSYBOX grep "$@"; }
  fi;
fi;

if [ "$DIR" = ${PIF_ENGINE_DATA} ]; then
  DIR=$DIR/autopif4;
  mkdir -p $DIR;
fi;
cd "$DIR";

item "Crawling Android Developers for latest Pixel Beta device list ...";
wget -q -T 10 -O PIXEL_VERSIONS_HTML "https://developer.android.com/about/versions" 2>&1 || exit 1;
LATEST_BETA=$(grep -B4 -A2 'data-icon="preview' PIXEL_VERSIONS_HTML | grep -o 'href="/about/versions/.*[0-9]"' | cut -d\" -f2);
[ "$LATEST_BETA" ] || LATEST_BETA=$(grep -oE 'href="/about/versions/[0-9]{2}"' PIXEL_VERSIONS_HTML | cut -d\" -f2 | sort -ru | head -n1);
wget -q -T 10 -O PIXEL_LATEST_HTML "https://developer.android.com$LATEST_BETA" 2>&1 || exit 1;
wget -q -T 10 -O PIXEL_FI_HTML "https://developer.android.com$(grep -o 'href=".*download.*"' PIXEL_LATEST_HTML | grep -v 'ota' | cut -d\" -f2 | sort -ru | head -n1)" 2>&1 || exit 1;
wget -q -T 10 -O PIXEL_OTA_HTML "https://developer.android.com$(grep -o 'href=".*download-ota.*"' PIXEL_LATEST_HTML | cut -d\" -f2 | sort -ru | head -n1)" 2>&1 || exit 1;
SRC=FI; [ "$(grep 'tr id=' PIXEL_FI_HTML | sed 's;.*<tr id="\(.*\)">.*;\1;' | wc -w)" -lt "$(grep 'tr id=' PIXEL_OTA_HTML | sed 's;.*<tr id="\(.*\)">.*;\1;' | wc -w)" ] && SRC=OTA;
MODEL_LIST="$(grep -A1 'tr id=' PIXEL_${SRC}_HTML | grep 'td' | sed 's;.*<td>\(.*\)</td>.*;\1;')";
PRODUCT_LIST="$(grep 'tr id=' PIXEL_${SRC}_HTML | sed 's;.*<tr id="\(.*\)">.*;\1_beta;')";
echo "$PRODUCT_LIST" | wc -w;

item "Selecting Pixel Beta device ...";
if [ "$FORCE_MATCH" ]; then
  DEVICE="$(getprop ro.product.device)";
  case "$(echo ' '$PRODUCT_LIST' ')" in
    *" ${DEVICE}_beta "*)
      MODEL="$(getprop ro.product.model)";
      PRODUCT="${DEVICE}_beta";
    ;;
  esac;
fi;
set_random_beta() {
  local list_count="$(echo "$MODEL_LIST" | wc -l)";
  local list_rand="$((RANDOM % $list_count + 1))";
  local IFS=$'\n';
  set -- $MODEL_LIST;
  MODEL="$(eval echo \${$list_rand})";
  set -- $PRODUCT_LIST;
  PRODUCT="$(eval echo \${$list_rand})";
  DEVICE="$(echo "$PRODUCT" | sed 's/_beta//')";
}
if [ -z "$PRODUCT" ]; then
  set_random_beta;
fi;
echo "$MODEL ($PRODUCT)";

get_canary_info() {
  item "Crawling Android Flash Tool for latest Pixel Canary build info ...";
  wget -q -T 10 -O PIXEL_FLASH_HTML "https://flash.android.com/" 2>&1 || exit 1;
  wget -q -T 10 -O PIXEL_STATION_JSON --header "Referer: https://flash.android.com" "https://content-flashstation-pa.googleapis.com/v1/builds?product=$PRODUCT&key=$(grep -o '<body data-client-config=.*' PIXEL_FLASH_HTML | cut -d\; -f2 | cut -d\& -f1)" 2>&1 || exit 1;
  tac PIXEL_STATION_JSON | grep -m1 -A13 '"canary": true' > PIXEL_CANARY_JSON;
  ID="$(grep 'releaseCandidateName' PIXEL_CANARY_JSON | cut -d\" -f4)";
  INCREMENTAL="$(grep 'buildId' PIXEL_CANARY_JSON | cut -d\" -f4)";
}
get_canary_info;
if [ -z "$ID" -o -z "$INCREMENTAL" ] && [ "$FORCE_MATCH" ]; then
  warn "Failed to extract matching build info from JSON";
  item "Retrying with random Pixel Beta device ...";
  set_random_beta;
  echo "$MODEL ($PRODUCT)";
  get_canary_info;
fi;
[ -z "$ID" -o -z "$INCREMENTAL" ] && die "Failed to extract build info from JSON";
echo "Android $(grep 'releaseTrackVersionName' PIXEL_CANARY_JSON | cut -d\" -f4)";

FI="$(grep 'factoryImageDownloadUrl' PIXEL_CANARY_JSON | cut -d\" -f4)";
# Certificate-verified HTTPS is also required for metadata-only HEAD requests.
if [ "$FI" ]; then
  wget -q -T 10 -S --spider -o PIXEL_ZIP_HEADERS "$FI" 2>&1 || exit 1;
else
  warn "Failed to extract Factory Image URL from JSON";
fi;
if [ -f PIXEL_ZIP_HEADERS ] && grep -q 'Last-Modified' PIXEL_ZIP_HEADERS; then
  CANARY_REL_DATE="$(date -D '%a, %d %b %Y %H:%M:%S %Z' -d "$(grep -o 'Last-Modified.*' PIXEL_ZIP_HEADERS | cut -d\  -f2-)" '+%Y-%m-%d')";
  CANARY_EXP_DATE="$(date -D '%s' -d "$(($(date -D '%Y-%m-%d' -d "$CANARY_REL_DATE" '+%s') + 60 * 60 * 24 * 7 * 6))" '+%Y-%m-%d')";
  echo "Canary Released: $CANARY_REL_DATE \
    \nEstimated Expiry: $CANARY_EXP_DATE";
else
  warn "Failed to determine Release Date from HTTP headers";
  CANARY_REL_DATE="Unknown";
  CANARY_EXP_DATE="Unknown";
fi;

item "Crawling Pixel Update Bulletins for corresponding security patch level ...";
CANARY_ID="$(grep '"id"' PIXEL_CANARY_JSON | sed -e 's;.*canary-\(.*\)".*;\1;' -e 's;^\(.\{4\}\);\1-;')";
[ -z "$CANARY_ID" ] && die "Failed to extract build info from JSON";
wget -q -T 10 -O PIXEL_SECBULL_HTML "https://source.android.com/docs/security/bulletin/pixel" 2>&1 || exit 1;
SECURITY_PATCH="$(grep "<td>$CANARY_ID" PIXEL_SECBULL_HTML | sed 's;.*<td>\(.*\)</td>;\1;')";
if [ -z "$SECURITY_PATCH" ]; then
  warn "Failed to determine exact security patch level from Pixel Update Bulletins";
  item "Assuming probable security patch level from Canary build info ...";
  SECURITY_PATCH="${CANARY_ID}-05";
fi;
echo "$SECURITY_PATCH";

item "Adding corresponding first API level ...";
case $DEVICE in
  bluejay) DEVICE_INITIAL_SDK_INT=32;; # Pixel 6a
  panther|cheetah) DEVICE_INITIAL_SDK_INT=33;; # Pixel 7 Series
  lynx) DEVICE_INITIAL_SDK_INT=33;; # Pixel 7a
  felix) DEVICE_INITIAL_SDK_INT=33;; # Pixel Fold
  tangorpro) DEVICE_INITIAL_SDK_INT=33;; # Pixel Tablet
  shiba|husky) DEVICE_INITIAL_SDK_INT=34;; # Pixel 8 Series
  akita) DEVICE_INITIAL_SDK_INT=34;; # Pixel 8a
  tokay|caiman|komodo|comet) DEVICE_INITIAL_SDK_INT=34;; # Pixel 9 Series
  tegu) DEVICE_INITIAL_SDK_INT=35;; # Pixel 9a
  frankel|blazer|mustang|rango) DEVICE_INITIAL_SDK_INT=36;; # Pixel 10 Series
  stallion) DEVICE_INITIAL_SDK_INT=35;; # Pixel 10a; Actual 36
  cubs|grizzly|kodiak|yogi) DEVICE_INITIAL_SDK_INT=37;; # Pixel 11 Series
  *) DEVICE_INITIAL_SDK_INT=37;;
esac;
echo "$DEVICE_INITIAL_SDK_INT";
[ "$DEVICE_INITIAL_SDK_INT" -gt 35 ] && warn "First API level 36 and greater may require an RKP keybox to pass";

# Network metadata becomes shell migration input; reject shell metacharacters.
for safe_value in "$PRODUCT" "$DEVICE" "$ID" "$INCREMENTAL"; do
  printf '%s\n' "$safe_value" | grep -Eq '^[A-Za-z0-9._-]+$' || die "Invalid build metadata";
done;
printf '%s\n' "$MODEL" | grep -Eq '^[-A-Za-z0-9 .()+_]+$' || die "Invalid device model";
printf '%s\n' "$SECURITY_PATCH" | grep -Eq '^[0-9]{4}-[0-9]{2}-[0-9]{2}$' || die "Invalid patch level";

item "Dumping values to minimal pif.prop ...";
cat <<EOF | tee pif.prop;
MANUFACTURER=Google
MODEL=$MODEL
FINGERPRINT=google/$PRODUCT/$DEVICE:CANARY/$ID/$INCREMENTAL:user/release-keys
PRODUCT=$PRODUCT
DEVICE=$DEVICE
SECURITY_PATCH=$SECURITY_PATCH
DEVICE_INITIAL_SDK_INT=$DEVICE_INITIAL_SDK_INT
EOF

# Migration and option retention are performed by the independent GPL CLI worker.
# Keep this script's documented Canary heuristics, including its first-API mapping.
cp pif.prop custom.pif.prop;
printf '\n# Canary Released: %s\n# Estimated Expiry: %s\n' "$CANARY_REL_DATE" "$CANARY_EXP_DATE" >> custom.pif.prop;
