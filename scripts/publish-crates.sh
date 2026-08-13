#!/usr/bin/env bash

set -euo pipefail

tag="${1:-${GITHUB_REF_NAME:-}}"
if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then
  echo "release tag must be semantic version prefixed with v, received: ${tag:-<empty>}" >&2
  exit 1
fi

version="${tag#v}"
crates=(
  bylaw-core
  bylaw-analyzer
  bylaw-config
  bylaw
  cargo-bylaw
)

cargo metadata --no-deps --format-version 1 |
  python3 -c '
import json
import sys

expected_version = sys.argv[1]
expected_names = set(sys.argv[2:])
metadata = json.load(sys.stdin)
workspace_members = set(metadata["workspace_members"])
packages = {
    package["name"]: package["version"]
    for package in metadata["packages"]
    if package["id"] in workspace_members
}

if set(packages) != expected_names:
    missing = sorted(expected_names - set(packages))
    unexpected = sorted(set(packages) - expected_names)
    raise SystemExit(
        f"workspace package mismatch; missing={missing}, unexpected={unexpected}"
    )

wrong = {
    name: package_version
    for name, package_version in packages.items()
    if package_version != expected_version
}
if wrong:
    raise SystemExit(
        f"tag version {expected_version} does not match workspace packages: {wrong}"
    )
' "$version" "${crates[@]}"

if [[ "${PUBLISH_DRY_RUN:-0}" == "1" ]]; then
  cargo package --locked --allow-dirty --no-verify -p bylaw-core
  for crate in "${crates[@]:1}"; do
    cargo package --locked --allow-dirty --list -p "$crate" >/dev/null
  done
  exit 0
fi

: "${CARGO_REGISTRY_TOKEN:?CARGO_REGISTRY_TOKEN must be set}"

user_agent="cargo-bylaw-release-workflow"
if [[ -n "${GITHUB_REPOSITORY:-}" ]]; then
  user_agent+=" (${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY})"
fi

crate_exists() {
  local crate="$1"
  local status
  status="$(
    curl --silent --show-error \
      --output /dev/null \
      --write-out "%{http_code}" \
      --user-agent "$user_agent" \
      "https://crates.io/api/v1/crates/${crate}/${version}"
  )" || {
    echo "failed to query crates.io for ${crate}@${version}" >&2
    return 2
  }

  case "$status" in
    200) return 0 ;;
    404) return 1 ;;
    *)
      echo "crates.io returned HTTP ${status} for ${crate}@${version}" >&2
      return 2
      ;;
  esac
}

wait_for_index() {
  local crate="$1"
  for attempt in $(seq 1 30); do
    if cargo info --registry crates-io "${crate}@${version}" >/dev/null 2>&1; then
      return 0
    fi
    echo "waiting for ${crate}@${version} to reach the crates.io index (${attempt}/30)"
    sleep 10
  done
  echo "timed out waiting for ${crate}@${version} in the crates.io index" >&2
  return 1
}

for crate in "${crates[@]}"; do
  if crate_exists "$crate"; then
    echo "${crate}@${version} is already published; skipping"
  else
    status=$?
    if [[ "$status" -ne 1 ]]; then
      exit "$status"
    fi
    cargo publish --locked --registry crates-io -p "$crate"
  fi
  wait_for_index "$crate"
done
