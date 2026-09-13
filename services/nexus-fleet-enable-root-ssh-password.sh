#!/usr/bin/env bash
# nexus-fleet:name=Allow root SSH login with password
# nexus-fleet:description=Sets PermitRootLogin/PasswordAuthentication to yes and restarts sshd -- most templates ship with password root login disabled by default.
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
    echo "ERROR: Run this script as root."
    exit 1
fi

CONFIG="/etc/ssh/sshd_config"
if [[ ! -f "$CONFIG" ]]; then
    echo "ERROR: $CONFIG not found."
    exit 1
fi

set_directive() {
    local key="$1" value="$2"
    if grep -qE "^[[:space:]]*${key}[[:space:]]+" "$CONFIG"; then
        sed -i -E "s|^[[:space:]]*${key}[[:space:]]+.*|${key} ${value}|" "$CONFIG"
    elif grep -qE "^[[:space:]]*#[[:space:]]*${key}[[:space:]]+" "$CONFIG"; then
        sed -i -E "s|^[[:space:]]*#[[:space:]]*${key}[[:space:]]+.*|${key} ${value}|" "$CONFIG"
    else
        echo "${key} ${value}" >> "$CONFIG"
    fi
}

set_directive "PermitRootLogin" "yes"
set_directive "PasswordAuthentication" "yes"

# Some distros ship an sshd_config.d drop-in that overrides the main file with
# stricter defaults (e.g. Debian's cloud images) -- neutralize it too.
if [[ -d /etc/ssh/sshd_config.d ]]; then
    for override in /etc/ssh/sshd_config.d/*.conf; do
        [[ -e "$override" ]] || continue
        sed -i -E 's/^[[:space:]]*(PermitRootLogin|PasswordAuthentication)[[:space:]]+.*/# \0 (disabled by Nexus, see sshd_config)/' "$override"
    done
fi

sshd -t

if command -v systemctl >/dev/null 2>&1 && [[ -d /run/systemd/system ]]; then
    systemctl restart ssh.service 2>/dev/null || systemctl restart sshd.service
else
    service ssh restart 2>/dev/null || service sshd restart
fi

echo "Root SSH login with password is now allowed."
