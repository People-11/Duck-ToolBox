# SPDX-License-Identifier: GPL-3.0-only
# Modified 2026-10-24: runtime validation also protects manually edited lists.
pif_valid_app_path() {
  case "$1" in
    /*/app/* | /*/priv-app/* | /*/overlay/*) ;;
    *) return 1 ;;
  esac
  partition=${1#/}; partition=${partition%%/*}
  case "$partition" in system|product|system_ext|vendor|odm|oem) ;; *) return 1 ;; esac
  tail=${1#/*/}; category=${tail%%/*}; entry=${tail#*/}
  case "$category" in app|priv-app|overlay) ;; *) return 1 ;; esac
  case "$entry" in ""|.|..|*[!a-zA-Z0-9_.+*-]* ) return 1 ;; esac
  if [ "$category" = overlay ]; then
    case "$entry" in config|*\**) return 1 ;; esac
  fi
}
pif_validate_list() {
  while IFS= read -r pattern || [ -n "$pattern" ]; do
    case "$pattern" in ""|\#*) continue ;; esac
    pif_valid_app_path "$pattern" || { ui_print "! Invalid ROM replacement path: $pattern"; return 1; }
    # Expansion occurs only after validating fixed partition and directory segments.
    for expanded in $pattern; do
      pif_valid_app_path "$expanded" || return 1
    done
  done < "$1"
}
