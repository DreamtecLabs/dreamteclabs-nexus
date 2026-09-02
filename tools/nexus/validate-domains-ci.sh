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

# A compiled Yew component can still render as unstyled HTML if its stylesheet
# is never wired into the SCSS bundle. Keep the Domains surface and its import
# coupled so this regression is caught before packaging.
test -f ui/css/nexus-domains.scss
grep -q '^@import "nexus-domains";' ui/css/pdm.scss
grep -q '^\.nexus-domains {' ui/css/nexus-domains.scss
grep -q '^\.nexus-domain-table {' ui/css/nexus-domains.scss
grep -q '^\.nexus-domain-row {' ui/css/nexus-domains.scss
grep -q '^\.nexus-domain-action,' ui/css/nexus-domains.scss

cargo fmt --all -- --check
