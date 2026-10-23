#!/system/bin/sh
# Action button: issue a keybox with this device's KeyMint TA now, install it in the active
# keystore module and show the state that is also written to module.prop.

MODPATH=${0%/*}

echo "- Issuing a keybox with this device's KeyMint TA..."
echo "  Only some Qualcomm devices work, and only RKP certificates are issued."
if "$MODPATH/bin/duckctl.sh" renew --force >/dev/null 2>&1; then
  status=0
  echo "- Keybox installed."
else
  status=1
  echo "! Issuance failed."
fi
echo "- $(sed -n 's/^description=//p' "$MODPATH/module.prop")"
# Some managers close the log when the script ends; leave time to read it.
sleep 30
exit "$status"
