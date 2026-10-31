# 0008. release-plz, native musl builds, GHCR images; crates.io deferred

- Status: Accepted
- Date: 2026-10-31

## Context

Users install Domovoi as a static binary, a Docker image or (later) distribution packages. Releases should be automatic from Conventional Commits, reproducible and attested.

## Decision

- **Versioning and changelog:** release-plz opens a release PR from Conventional Commits; merging it tags `vX.Y.Z` and creates the GitHub release. Only the `domovoi` package is released; library crates have `publish = false` and `release = false`.
- **Binaries:** built in the same workflow run (so no personal access token is needed to trigger a second workflow), natively on `ubuntu-24.04` (x86_64) and `ubuntu-24.04-arm` (aarch64) for the `*-unknown-linux-musl` targets, with the web UI embedded. Each archive ships with a SHA-256 checksum and a build-provenance attestation.
- **Images:** a multi-arch image (`linux/amd64`, `linux/arm64`) built from the release binaries onto a distroless static base and pushed to `ghcr.io/<owner>/domovoi` with semver and `latest` tags, plus a provenance attestation.
- **Docs:** mdBook site deployed to GitHub Pages from `main`.
- **crates.io:** deferred until the library crates have stable APIs; revisit with a new ADR.

## Consequences

- Every merge to `main` that contains releasable commits produces or updates a release PR; humans decide when to release by merging it.
- Commit messages must follow Conventional Commits; CI checks PR titles.

## Alternatives considered

- cargo-dist: capable, but its generated workflow is harder for contributors to modify alongside the Node build.
- Cross-compiling aarch64 with `cross` or zig: slower and more fragile than native ARM runners.
