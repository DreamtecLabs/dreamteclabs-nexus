#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

bash -n services/nexus-domains-helper
bash -n tools/nexus/validate-domains-container.sh
bash -n tools/nexus/validate-monitoring-ci.sh

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

# Cloudflare failures must be actionable in the UI: validate credentials before
# any provider mutation and preserve the HTTP status plus Cloudflare error body.
grep -q '^validate_cloudflare_config() {' services/nexus-domains-helper
grep -q 'validate_cloudflare_config' services/nexus-domains-helper
grep -q 'NEXUS_CF_ACCOUNT_ID must be a 32-character Cloudflare account ID' services/nexus-domains-helper
grep -q 'NEXUS_CF_TUNNEL_ID must be a Cloudflare tunnel UUID' services/nexus-domains-helper
grep -q 'Cloudflare API .* failed with HTTP' services/nexus-domains-helper
if grep -q -- '-fsS' services/nexus-domains-helper; then
    echo 'Cloudflare requests must not hide API error bodies with curl -f' >&2
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

# Reconciliation retries belong to the API process so its timeout owns the
# active helper directly. The helper remains a single idempotent pass and emits
# step-aware diagnostics; configuration/policy conflicts remain fail-closed.
grep -q '^const HELPER_RECONCILE_ATTEMPTS:' server/src/api/nexus/domains.rs
grep -q '^const HELPER_RETRY_DELAY:' server/src/api/nexus/domains.rs
grep -q '^fn helper_exit_code_is_retryable' server/src/api/nexus/domains.rs
grep -q 'onboard-retry' server/src/api/nexus/domains.rs
grep -Fq 'Some(3) | Some(5) | Some(42) | Some(43)' server/src/api/nexus/domains.rs
grep -q 'reconcile step.*failed' services/nexus-domains-helper
grep -q 'BASH_SUBSHELL == 0' services/nexus-domains-helper
grep -q 'trap - ERR' services/nexus-domains-helper
grep -q 'trap reconcile_error ERR' services/nexus-domains-helper
if grep -q 'onboard-once' services/nexus-domains-helper; then
    echo 'Domains helper must not spawn a nested reconciliation process' >&2
    exit 1
fi

# Existing configurations are a policy decision, never an implicit destructive
# repair. Validation must surface the decision, adoption must persist a baseline,
# and destructive replacement must require the explicit migrate action.
grep -q 'API_METHOD_ADOPT_EXISTING_DOMAIN' server/src/api/nexus/domains.rs
grep -q 'configuration_mode' server/src/api/nexus/domains.rs
grep -q 'adopted_checks' server/src/api/nexus/domains.rs
grep -q 'decision_required' server/src/api/nexus/domains.rs
grep -q 'replace_existing' server/src/api/nexus/domains.rs
grep -q 'let helper_action = if replace_existing {' server/src/api/nexus/domains.rs
grep -q '^cf_delete_record() {' services/nexus-domains-helper
grep -q '^    migrate)' services/nexus-domains-helper
grep -q 'refusing destructive replacement' services/nexus-domains-helper
grep -q '"/domains/adopt"' ui/src/nexus/domains.rs
grep -q '"replace_existing":true' ui/src/nexus/domains.rs
grep -q 'Keep existing' ui/src/nexus/domains.rs
grep -q 'Use Nexus standard' ui/src/nexus/domains.rs

# The proxmox API schema exposes Rust arguments with their underscores intact.
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
grep -q '^\.nexus-domain-choice {' ui/css/nexus-domains.scss

bash tools/nexus/validate-monitoring-ci.sh
