# Maintaining

## Publish a new version

The [Release workflow](.github/workflows/release.yml) publishes the crate with [trusted publishing](https://crates.io/docs/trusted-publishing).

Run `./release.sh 1.2.3` from `main`. It updates the dependencies, bumps `version` in `Cargo.toml`, syncs `Cargo.lock`, tests, commits and pushes, runs the Release workflow, and creates the GitHub release.

A published version cannot be deleted or reused. It can only be yanked with `cargo yank --version 1.2.3`.
