#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

bash -n services/nexus-domains-helper

grep -q 'forbid_webmail_ddns' server/src/api/nexus/domains.rs
grep -q 'forbid_mail_proxy' server/src/api/nexus/domains.rs
grep -q 'webmail hostname is still DDNS-managed' services/nexus-domains-helper
if grep -Eq 'cf_upsert_single_dns .* A .*mail_host' services/nexus-domains-helper; then
  echo 'mail.* A records must remain DDNS-owned' >&2
  exit 1
fi

cargo fmt --all -- --check

# Debian package builds intentionally replace crates.io with
# /usr/share/cargo/registry. GitHub CI must not depend on that host-specific
# directory, so use an isolated Cargo home and temporarily remove the project
# source replacement while tests/compilation resolve the locked graph.
ci_cargo_home="${RUNNER_TEMP:-/tmp}/nexus-domains-cargo"
mkdir -p "$ci_cargo_home"
export CARGO_HOME="$ci_cargo_home"

cargo_config=".cargo/config.toml"
cargo_config_backup=".cargo/config.toml.nexus-ci-backup"
restore_cargo_config() {
  if [[ -f "$cargo_config_backup" ]]; then
    mv "$cargo_config_backup" "$cargo_config"
  fi
}
trap restore_cargo_config EXIT

if [[ -f "$cargo_config" ]]; then
  mv "$cargo_config" "$cargo_config_backup"
fi

cargo test -p server api::nexus::domains::tests --lib
cargo check -p server
