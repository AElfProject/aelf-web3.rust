#!/usr/bin/env bash

set -euo pipefail

ordered_packages=(
  aelf-proto
  aelf-crypto
  aelf-client
  aelf-keystore
  aelf-contract
  aelf-sdk
)

declare -A allowed_packages=()
declare -A requested_packages=()

for pkg in "${ordered_packages[@]}"; do
  allowed_packages["$pkg"]=1
done

if [[ -n "${SELECTED_PACKAGES:-}" && -n "${SELECTED_PACKAGES//[[:space:]]/}" ]]; then
  IFS=',' read -r -a raw_packages <<< "$SELECTED_PACKAGES"
  for raw_pkg in "${raw_packages[@]}"; do
    pkg="${raw_pkg//[[:space:]]/}"
    if [[ -z "$pkg" ]]; then
      continue
    fi
    if [[ -z "${allowed_packages[$pkg]:-}" ]]; then
      echo "::error::Unsupported package '$pkg'. Allowed packages: ${ordered_packages[*]}"
      exit 1
    fi
    requested_packages["$pkg"]=1
  done

  if [[ "${#requested_packages[@]}" -eq 0 ]]; then
    echo "::error::No valid packages were provided."
    exit 1
  fi
fi

package_selected() {
  local pkg="$1"
  if [[ "${#requested_packages[@]}" -eq 0 ]]; then
    return 0
  fi
  [[ -n "${requested_packages[$pkg]:-}" ]]
}

crate_version() {
  local pkgid
  local version

  pkgid="$(cargo pkgid -p "$1")"
  version="${pkgid##*@}"
  if [[ "$version" == "$pkgid" ]]; then
    version="${pkgid##*#}"
  fi
  echo "$version"
}

crate_registry_state() {
  local pkg="$1"
  local version="$2"
  local output
  local cmd_status

  set +e
  output="$(cargo info "${pkg}@${version}" --registry crates-io 2>&1)"
  cmd_status=$?
  set -e

  if [[ "$cmd_status" -eq 0 ]]; then
    echo "present"
    return 0
  fi

  if grep -qi "could not find" <<< "$output"; then
    echo "missing"
    return 0
  fi

  echo "::error::Failed to query crates.io index for ${pkg} ${version}: ${output}" >&2
  return 1
}

ensure_crate_visible() {
  local pkg="$1"
  local version="$2"
  local max_attempts=30
  local sleep_seconds=10

  for ((attempt = 1; attempt <= max_attempts; attempt++)); do
    state="$(crate_registry_state "$pkg" "$version")"
    case "$state" in
      present)
        echo "${pkg} ${version} is visible on crates.io."
        return 0
        ;;
      missing)
        echo "Waiting for ${pkg} ${version} to become visible on crates.io (${attempt}/${max_attempts})..."
        sleep "$sleep_seconds"
        ;;
      *)
        echo "::error::Unexpected crates.io registry state '${state}' while checking ${pkg} ${version}."
        return 1
        ;;
    esac
  done

  echo "::error::Timed out waiting for ${pkg} ${version} to become visible on crates.io."
  return 1
}

if [[ "${DRY_RUN:-false}" != "true" && -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
  echo "::error::CARGO_REGISTRY_TOKEN is required when DRY_RUN=false."
  exit 1
fi

if [[ "${DRY_RUN:-false}" == "true" && "${#requested_packages[@]}" -eq 0 ]]; then
  echo "Running workspace dry-run for the full publish set."
  cargo publish --workspace --dry-run --locked
  exit 0
fi

for pkg in "${ordered_packages[@]}"; do
  if ! package_selected "$pkg"; then
    continue
  fi

  version="$(crate_version "$pkg")"
  echo "Processing ${pkg} ${version}"

  if [[ "${DRY_RUN:-false}" == "true" ]]; then
    cargo publish -p "$pkg" --dry-run --locked
    continue
  fi

  state="$(crate_registry_state "$pkg" "$version")"
  case "$state" in
    present)
      if [[ "${SKIP_PUBLISHED:-true}" == "true" ]]; then
        echo "Skipping ${pkg} ${version}; version already exists on crates.io."
        continue
      fi
      echo "::error::${pkg} ${version} already exists on crates.io."
      exit 1
      ;;
    missing)
      ;;
    *)
      echo "::error::Unexpected crates.io registry state '${state}' while checking ${pkg} ${version}."
      exit 1
      ;;
  esac

  cargo publish -p "$pkg" --locked --token "$CARGO_REGISTRY_TOKEN"
  ensure_crate_visible "$pkg" "$version"
done
