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

# With set -u, values referenced by later assignments in the same local builtin
# are expanded before the earlier local variables exist. Keep DDNS inputs and
# derived values in separate declarations.
grep -Fq 'local zone="$1" record="$2"' services/nexus-domains-helper
grep -Fq 'local webmail="webmail.${zone}" line="${zone}|${record}|false"' services/nexus-domains-helper
if grep -Fq 'local zone="$1" record="$2" webmail="webmail.${zone}"' services/nexus-domains-helper; then
    echo 'DDNS derived locals must not share the declaration with zone/record under set -u' >&2
    exit 1
fi

# Reconciliation is intentionally idempotent. A transient provider/SSH/DNS
# failure must retry the entire sequence, while policy/configuration conflicts
# remain fail-closed and diagnostic output identifies the failed step.
grep -q '^RECONCILE_ATTEMPTS=' services/nexus-domains-helper
grep -q '^retryable_reconcile_rc() {' services/nexus-domains-helper
grep -q '^onboard_once() {' services/nexus-domains-helper
grep -q '^onboard() {' services/nexus-domains-helper
grep -q 'domain reconciliation attempt .* retrying idempotently' services/nexus-domains-helper
grep -q 'reconcile step.*failed' services/nexus-domains-helper
grep -q '3|5|42|43' services/nexus-domains-helper

# The proxmox API schema exposes the Rust hestia_user argument with its
# underscore intact. A kebab-case JSON key is rejected before the helper runs.
grep -q '"hestia_user":user' ui/src/nexus/domains.rs
if grep -q '"hestia-user":user' ui/src/nexus/domains.rs; then
    echo 'Domains UI must send the API parameter as hestia_user' >&2
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
