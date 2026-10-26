# Derived from Play Integrity Fork, osm0sis, chiteroman and contributors.
# Source: https://github.com/osm0sis/PlayIntegrityFork/tree/23ec56c2440c4fd1a5e893a23ef7861d6366a828
# SPDX-License-Identifier: GPL-3.0-only
# Modified 2026-10-24: independent engine packaging, bounded configuration and runtime operations.
# Modified 2026-10-26: fixed default options; no scripts-only mode, skips or stop gate.
pif_running || return 0
# Replace/hide conflicting custom ROM injection app folders/files to disable them
MAGISK_WHITEOUT=false
[ -f /data/adb/magisk/util_functions.sh ] && [ "$(grep MAGISK_VER_CODE /data/adb/magisk/util_functions.sh | cut -d= -f2)" -ge 28102 ] && MAGISK_WHITEOUT=true;

LIST=$PIF_ENGINE_SCRIPTS/app_replace_list.txt
. "$PIF_ENGINE_SCRIPTS/path_rules.sh"
pif_validate_list "$LIST" || exit 1
for APP in $(grep -v '^[[:space:]]*#' "$LIST"); do
    pif_running || return 0
    if [ -e "$APP" ]; then
        PREFIX=""
        HIDECFG=""
        PKGNAME=""
        case $APP in /system/*) ;; *) PREFIX=/system;; esac
        HIDEPATH=$MODPATH$PREFIX$APP
        printf '%s\n' "${HIDEPATH#"$MODPATH"/}" >> "$PIF_ENGINE_DATA/overlay-paths.txt"
        if [ -d "$APP" ]; then
            $MAGISK_WHITEOUT || mkdir -p "$HIDEPATH"
            if [ "$KSU" = "true" -o "$APATCH" = "true" ]; then
                setfattr -n trusted.overlay.opaque -v y "$HIDEPATH"
            elif $MAGISK_WHITEOUT; then
                mkdir -p "$(dirname "$HIDEPATH")"
                mknod "$HIDEPATH" c 0 0
            else
                touch "$HIDEPATH/.replace"
            fi
        else
            mkdir -p "$(dirname "$HIDEPATH")"
            if [ "$KSU" = "true" -o "$APATCH" = "true" -o "$MAGISK_WHITEOUT" = "true" ]; then
                mknod "$HIDEPATH" c 0 0
            else
                touch "$HIDEPATH"
            fi
        fi
        case $APP in
            */overlay/*)
                CFG=$(echo "$APP" | grep -oE '.*/overlay')/config/config.xml
                if [ -f "$CFG" ]; then
                    if [ -d "$APP" ]; then
                        APK=$(readlink -f "$APP"/*.apk);
                    elif [[ "$APP" = *".apk" ]]; then
                        APK=$(readlink -f "$APP");
                    fi
                    if [ -s "$APK" ]; then
                        PKGNAME=$(unzip -p "$APK" AndroidManifest.xml | tr -d '\0' | grep -oE 'android.*overlay' | strings | tr -d '\n' | sed -e 's/^android//' -e 's/application//' -e 's;*http.*res/android;;' -e 's/manifest//' -e 's/overlay$//' | grep -oE '[[:alnum:].-_].*overlay' | cut -d\  -f2)
                        if [ "$PKGNAME" ] && grep -q "overlay package=\"$PKGNAME" "$CFG"; then
                            HIDECFG=$MODPATH$PREFIX$CFG
                            printf '%s\n' "${HIDECFG#"$MODPATH"/}" >> "$PIF_ENGINE_DATA/overlay-paths.txt"
                            if [ ! -f "$HIDECFG" ]; then
                                mkdir -p "$(dirname "$HIDECFG")"
                                cp -af "$CFG" "$HIDECFG"
                            fi
                            sed -i 's;<overlay \(package="'"$PKGNAME"'".*\) />;<!-- overlay \1 -->;' "$HIDECFG"
                        fi
                    fi
                fi
            ;;
        esac
        if [[ -d "$APP" || "$APP" = *".apk" ]]; then
            ui_print "! $(basename "$APP" .apk) ROM app disabled, please uninstall any user app versions/updates after next reboot"
            [ "$HIDECFG" ] && ui_print "!  + $PKGNAME entry commented out in copied overlay config"
        fi
    fi
done

# Work around custom ROM PropImitationHooks conflict when their persist props don't exist
if resetprop | grep -qE "ro.aospa.version|net.pixelos.version|ro.afterlife.version" || [ -f /data/system/gms_certified_props.json ]; then
    for PROP in persist.sys.pihooks.first_api_level persist.sys.pihooks.security_patch; do
        resetprop | grep -q "\[$PROP\]" || persistprop "$PROP" ""
    done
fi

# Work around supported custom ROM PropImitationHooks/PixelPropsUtils (and hybrids) conflict when spoofProvider is disabled
if resetprop | grep -qE "persist.sys.pihooks|persist.sys.entryhooks|persist.sys.spoof|persist.sys.pixelprops|persist.sys.pp" || [ -f /data/system/gms_certified_props.json ]; then
    persistprop persist.sys.pihooks.disable.gms_props true
    persistprop persist.sys.pihooks.disable.gms_key_attestation_block true
    persistprop persist.sys.entryhooks_enabled false
    persistprop persist.sys.spoof.gms false
    persistprop persist.sys.pixelprops.gms false
    persistprop persist.sys.pixelprops.gapps false
    persistprop persist.sys.pixelprops.google false
    persistprop persist.sys.pixelprops.pi false
    persistprop persist.sys.pp.gms false
    persistprop persist.sys.pp.finsky false
fi
