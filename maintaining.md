# Maintaining

## Publish a new version

Requires a toolchain at least as new as `rust-version` in `Cargo.toml` (run `rustup update stable`) and `cargo login` once.

1. Bump `version` in `Cargo.toml`.
2. Run `cargo update --workspace` to sync `Cargo.lock`.
3. Commit: `git commit -am "1.2.3"`
4. Do a dry run: `cargo publish --dry-run --locked`
5. Publish: `cargo publish --locked`
6. Tag and push: `git tag v1.2.3 && git push && git push --tags`
7. Create a release on GitHub.

A published version cannot be deleted or reused. It can only be yanked with `cargo yank --version 1.2.3`.
