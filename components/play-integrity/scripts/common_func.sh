# Derived from Play Integrity Fork, osm0sis, chiteroman and contributors.
# Source: https://github.com/osm0sis/PlayIntegrityFork/tree/23ec56c2440c4fd1a5e893a23ef7861d6366a828
# SPDX-License-Identifier: GPL-3.0-only
# Modified 2026-10-24: independent engine packaging, bounded configuration and runtime operations.
# A separate stop gate remains usable even when configuration files are invalid.
. "$PIF_ENGINE_SCRIPTS/power.sh"

SKIPDELPROP=false
[ -f "$PIF_ENGINE_DATA/skipdelprop" ] && SKIPDELPROP=true

# delprop_if_exist <prop name>
delprop_if_exist() {
    pif_running || return 0
    local NAME="$1"

    if [ -n "$(resetprop "$NAME")" ]; then
        resetprop --delete "$NAME"
        resetprop -c $(resetprop -Z "$NAME") >/dev/null 2>&1 || true
    fi
}

SKIPPERSISTPROP=false
[ -f "$PIF_ENGINE_DATA/skippersistprop" ] && SKIPPERSISTPROP=true

# Quote property values as data in the restoration script, including apostrophes.
shell_quote() { printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"; }

# persistprop <prop name> <new value>
persistprop() {
    pif_running || return 0
    local NAME="$1"
    local NEWVALUE="$2"
    local CURVALUE="$(resetprop "$NAME")"
    local RESTORE="$PIF_ENGINE_DATA/uninstall.sh"

    if ! grep -Fqx "# saved: $NAME" "$RESTORE" 2>/dev/null; then
        printf '# saved: %s\n' "$NAME" >> "$RESTORE"
        if resetprop | grep -Fq "[$NAME]:"; then
            printf 'resetprop -n -p %s %s\n' "$(shell_quote "$NAME")" "$(shell_quote "$CURVALUE")" >> "$RESTORE"
        else
            printf 'resetprop -p --delete %s\n' "$(shell_quote "$NAME")" >> "$RESTORE"
        fi
    fi
    resetprop -n -p "$NAME" "$NEWVALUE"
}

RESETPROP="resetprop -n"
[ -f /data/adb/magisk/util_functions.sh ] && [ "$(grep MAGISK_VER_CODE /data/adb/magisk/util_functions.sh | cut -d= -f2)" -lt 27003 ] && RESETPROP=resetprop_hexpatch

# resetprop_hexpatch [-f|--force] <prop name> <new value>
resetprop_hexpatch() {
    pif_running || return 0
    case "$1" in
        -f|--force) local FORCE=1; shift;;
    esac

    local NAME="$1"
    local NEWVALUE="$2"
    local CURVALUE="$(resetprop "$NAME")"

    [ ! "$NEWVALUE" -o ! "$CURVALUE" ] && return 1
    [ "$NEWVALUE" = "$CURVALUE" -a ! "$FORCE" ] && return 2

    local NEWLEN=${#NEWVALUE}
    if [ -f /dev/__properties__ ]; then
        local PROPFILE=/dev/__properties__
    else
        local PROPFILE="/dev/__properties__/$(resetprop -Z "$NAME")"
    fi
    [ ! -f "$PROPFILE" ] && return 3
    local NAMEOFFSET=$(echo $(strings -t d "$PROPFILE" | grep "$NAME") | cut -d\  -f1)

    #<hex 2-byte change counter><flags byte><hex length of prop value><prop value + nul padding to 92 bytes><prop name>
    local NEWHEX="$(printf '%02x' "$NEWLEN")$(printf "$NEWVALUE" | od -A n -t x1 -v | tr -d ' \n')$(printf "%$((92-NEWLEN))s" | sed 's/ /00/g')"

    printf "Patch '$NAME' to '$NEWVALUE' in '$PROPFILE' @ 0x%08x -> \n[0000??$NEWHEX]\n" $((NAMEOFFSET-96))

    echo -ne "\x00\x00" \
        | dd obs=1 count=2 seek=$((NAMEOFFSET-96)) conv=notrunc of="$PROPFILE"
    echo -ne "$(printf "$NEWHEX" | sed -e 's/.\{2\}/&\\x/g' -e 's/^/\\x/' -e 's/\\x$//')" \
        | dd obs=1 count=93 seek=$((NAMEOFFSET-93)) conv=notrunc of="$PROPFILE"
}

# resetprop_if_diff <prop name> <expected value>
resetprop_if_diff() {
    pif_running || return 0
    local NAME="$1"
    local EXPECTED="$2"
    local CURRENT="$(resetprop "$NAME")"

    [ -z "$CURRENT" ] || [ "$CURRENT" = "$EXPECTED" ] || $RESETPROP "$NAME" "$EXPECTED"
}

# resetprop_if_empty <prop name> <expected value>
resetprop_if_empty() {
    pif_running || return 0
    # The legacy hexpatch path can only edit an existing property.
    [ -n "$(resetprop "$1")" ] || resetprop -n "$1" "$2"
}

# resetprop_if_match <prop name> <value match string> <new value>
resetprop_if_match() {
    pif_running || return 0
    local NAME="$1"
    local CONTAINS="$2"
    local VALUE="$3"

    [[ "$(resetprop "$NAME")" = *"$CONTAINS"* ]] && $RESETPROP "$NAME" "$VALUE"
}

# stub for boot-time
if [ "$(getprop sys.boot_completed)" != "1" ]; then
    ui_print() { return; }
fi
