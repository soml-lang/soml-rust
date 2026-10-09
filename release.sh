#!/usr/bin/env bash
# Releases a new version through the Release workflow.
# Usage: ./release.sh <version> (for example `./release.sh 1.2.3` or `./release.sh v1.2.3`)

set -euo pipefail

cd "$(dirname "$0")"

cargo_version() {
	sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml
}

# Checks that the first version is greater than the second, comparing the numeric `x.y.z` triple. A release is always newer than a prerelease of the same version.
version_newer() {
	local IFS=.
	local -a left
	local -a right
	read -r -a left <<< "${1%-*}"
	read -r -a right <<< "${2%-*}"
	for i in 0 1 2; do
		if ((left[i] > right[i])); then
			return 0
		fi
		if ((left[i] < right[i])); then
			return 1
		fi
	done
	if [[ $1 == *-* ]] && [[ $2 != *-* ]]; then
		return 1
	fi
	if [[ $1 != *-* ]] && [[ $2 == *-* ]]; then
		return 0
	fi
	return 1
}

if [[ $# -ne 1 ]]; then
	echo "Usage: $(basename "$0") <version>" >&2
	exit 1
fi

version=${1#v}
if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
	echo "Invalid version: $1" >&2
	exit 1
fi

if [[ -n $(git status --porcelain) ]]; then
	echo "The working tree is not clean." >&2
	exit 1
fi

branch=$(git branch --show-current)
if [[ $branch != main ]]; then
	echo "Releases are made from main, not $branch." >&2
	exit 1
fi

if ! command -v gh >/dev/null 2>&1; then
	echo "gh (GitHub CLI) is not installed." >&2
	exit 1
fi

if ! gh auth status --active >/dev/null 2>&1; then
	echo "gh is not authenticated. Run: gh auth login" >&2
	exit 1
fi

git fetch origin main --tags
if [[ $(git rev-parse HEAD) != $(git rev-parse origin/main) ]]; then
	echo "main is not in sync with origin/main." >&2
	exit 1
fi

if git rev-parse --verify --quiet "refs/tags/v$version" >/dev/null; then
	echo "The tag v$version already exists." >&2
	exit 1
fi

current_version=$(cargo_version)
if [[ -z $current_version ]]; then
	echo "Could not read the version from Cargo.toml." >&2
	exit 1
fi
if [[ $current_version == "$version" ]]; then
	echo "Cargo.toml is already at $version." >&2
	exit 1
fi
if ! version_newer "$version" "$current_version"; then
	echo "The new version must be greater than $current_version." >&2
	exit 1
fi

echo "Releasing $current_version -> $version"

perl -pi -e "s/^version = \"[^\"]*\"/version = \"$version\"/" Cargo.toml

if [[ $(cargo_version) != "$version" ]]; then
	echo "Failed to update the version in Cargo.toml." >&2
	exit 1
fi

# Update the dependencies and sync Cargo.lock with the new version.
cargo update

cargo test --locked --all-features

git add Cargo.toml Cargo.lock
git commit -m "$version"
git push origin main

head_sha=$(git rev-parse HEAD)

gh workflow run release.yml --ref main

run_id=
for ((i = 0; i < 60; i++)); do
	if ! run_id=$(gh run list --workflow release.yml --branch main --limit 10 --json databaseId,headSha --jq ".[] | select(.headSha == \"$head_sha\") | .databaseId" | head -n 1); then
		echo "Failed to query the release run." >&2
		exit 1
	fi
	if [[ -n $run_id ]]; then
		break
	fi
	sleep 2
done

if [[ -z $run_id ]]; then
	echo "Could not find the release run for this commit. The version bump is already pushed and the workflow may still publish it. Track it with: gh run list --workflow release.yml" >&2
	exit 1
fi

if ! gh run watch "$run_id" --exit-status; then
	echo "The Release workflow failed. Check the Actions tab. If the crate was published before the failure, it cannot be re-run; create the missing resources with: gh release create v$version --generate-notes" >&2
	exit 1
fi

git fetch origin main --tags
tag_sha=$(git rev-parse --verify --quiet "refs/tags/v$version")
if [[ -z $tag_sha ]]; then
	echo "The tag v$version was not pushed. The crate may not be published. Check the Actions tab." >&2
	exit 1
fi
if [[ $tag_sha != "$head_sha" ]]; then
	echo "The tag v$version does not point at the released commit." >&2
	exit 1
fi

if ! gh release create "v$version" --generate-notes; then
	echo "The crate is published and the tag is pushed, but creating the GitHub release failed. Retry with: gh release create v$version --generate-notes" >&2
	exit 1
fi

echo "Released v$version"
