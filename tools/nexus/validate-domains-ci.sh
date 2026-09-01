#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

bash -n services/nexus-domains-helper
bash -n tools/nexus/validate-domains-container.sh

grep -q 'forbid_webmail_ddns' server/src/api/nexus/domains.rs
grep -q 'forbid_mail_proxy' server/src/api/nexus/domains.rs
grep -q 'HELPER_TIMEOUT' server/src/api/nexus/domains.rs
grep -q 'validate_hestia_user' server/src/api/nexus/domains.rs
grep -q 'validate_hestia_user' services/nexus-domains-helper
grep -q 'webmail hostname is still DDNS-managed' services/nexus-domains-helper
grep -q 'refusing ambiguous update' services/nexus-domains-helper
if grep -Eq 'cf_upsert_single_dns .* A .*mail_host' services/nexus-domains-helper; then
    echo 'mail.* A records must remain DDNS-owned' >&2
    exit 1
fi

cargo fmt --all -- --check
