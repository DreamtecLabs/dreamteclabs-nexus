#!/usr/bin/env bash
# nexus-fleet:name=Set timezone
# nexus-fleet:description=Sets the guest's system timezone (e.g. America/Sao_Paulo).
# nexus-fleet:params=TIMEZONE
set -euo pipefail

TIMEZONE="${TIMEZONE:?TIMEZONE is required, e.g. America/Sao_Paulo}"

if [[ $EUID -ne 0 ]]; then
    echo "ERROR: Run this script as root."
    exit 1
fi

if [[ ! -e "/usr/share/zoneinfo/${TIMEZONE}" ]]; then
    echo "ERROR: unknown timezone '${TIMEZONE}' (no /usr/share/zoneinfo/${TIMEZONE})."
    exit 1
fi

if command -v timedatectl >/dev/null 2>&1 && [[ -d /run/systemd/system ]]; then
    timedatectl set-timezone "${TIMEZONE}"
else
    ln -sf "/usr/share/zoneinfo/${TIMEZONE}" /etc/localtime
    echo "${TIMEZONE}" > /etc/timezone
fi

echo "Timezone set to ${TIMEZONE}."
