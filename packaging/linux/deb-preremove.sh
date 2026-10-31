#!/bin/sh
set -eu
if [ "${1:-}" = remove ] || [ "${1:-}" = deconfigure ]; then
    # Removal needs an explicit reset. Stopping the service no longer clears a
    # saved native threshold, so only `reset` returns the hardware to normal
    # charging and disables the saved limit and schedules before they could be reapplied.
    if ! systemctl is-active --quiet batterybridge.service; then
        systemctl start batterybridge.service
        attempt=0
        until /usr/bin/batterybridge config >/dev/null 2>&1; do
            attempt=$((attempt + 1))
            [ "$attempt" -lt 10 ] || {
                echo 'BatteryBridge did not start, so charging could not be restored; removal stopped.' >&2
                exit 1
            }
            sleep 1
        done
    fi
    /usr/bin/batterybridge reset || {
        echo 'Normal charging could not be restored; removal stopped.' >&2
        exit 1
    }
    systemctl stop batterybridge.service
    # The daemon exits nonzero if its orderly shutdown cleanup failed.
    [ "$(systemctl show --property=ExecMainStatus --value batterybridge.service)" = 0 ] || {
        echo 'The service did not stop cleanly; removal stopped.' >&2
        exit 1
    }
    systemctl disable batterybridge.service
fi
