#!/usr/bin/env bash
set -euo pipefail

base_sha="${1:?base SHA is required}"
head_sha="${2:-HEAD}"

mapfile -t additions < <(
  git diff --name-status "${base_sha}...${head_sha}" -- \
    server/src/api/nexus \
    ui/src/nexus \
  | awk '$1 == "A" { print $2 }'
)

if ((${#additions[@]})); then
  echo "New Nexus product files must not be added inside the PDM source tree." >&2
  echo "PDM is an infrastructure provider. Add product/domain code under nexus/ instead." >&2
  printf '  - %s\n' "${additions[@]}" >&2
  exit 1
fi

if git diff --quiet "${base_sha}...${head_sha}" -- server/src/api/nexus ui/src/nexus; then
  echo "PDM provider boundary: no legacy Nexus paths changed."
else
  echo "PDM provider boundary: existing legacy files changed, but no new product files were introduced."
  echo "Such changes are permitted only for migration, deletion, provider adaptation, or production-parity fixes."
fi
