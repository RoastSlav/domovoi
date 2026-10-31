# Repository setup (maintainers)

Do these once after creating the GitHub repository and pushing the scaffold.

## 1. Replace placeholders

Replace `OWNER` with your GitHub user or organisation in: `Cargo.toml` (`repository`, `homepage`), `README.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md` (also set a real contact address), `.github/CODEOWNERS`, `.github/ISSUE_TEMPLATE/config.yml`, `docs/book/book.toml`.

```sh
grep -rl OWNER --exclude-dir=target --exclude-dir=node_modules . | xargs sed -i 's/OWNER/your-github-name/g'
```

## 2. Repository settings

- **Settings → General:** allow squash merging only; default commit message "Pull request title"; enable "Automatically delete head branches"; enable Discussions.
- **Settings → Actions → General:** workflow permissions "Read repository contents"; tick **Allow GitHub Actions to create and approve pull requests** (release-plz opens the release PR).
- **Settings → Pages:** Source = GitHub Actions (the Docs workflow deploys the mdBook site).
- **Settings → Code security:** enable private vulnerability reporting, Dependabot alerts and security updates, secret scanning and push protection.
- **Settings → Rules → Rulesets** for `main`: require a pull request, require status checks `Web (lint, typecheck, test, build)`, `Rust (fmt, clippy, tests, docs, arch-check)`, `Supply chain (cargo-deny)`, `MSRV`, `Static musl build check`, `Workflow lint`, `PR title follows Conventional Commits`; require linear history; block force pushes.
- **Packages:** after the first release, set the `domovoi` GHCR package visibility to public.

## 3. Labels

Create `release` (used by release-plz PRs), `bug`, `enhancement`, `triage`, `security`, `good first issue`.

## 4. First release

1. Merge feature work until v0.1 is done.
2. release-plz keeps a release PR open. Before merging the v0.1 release PR, set the version to `0.1.0` in it (pre-1.0, release-plz otherwise bumps the patch number).
3. Merging tags `v0.1.0`; the Release workflow builds both musl binaries with the embedded UI, attaches archives, checksums and provenance attestations to the GitHub release, and pushes `ghcr.io/RoastSlav/domovoi:0.1.0` and `:latest`.
4. Verify: `gh attestation verify domovoi-v0.1.0-x86_64-unknown-linux-musl.tar.gz --repo RoastSlav/domovoi`.

On the very first release-plz run, check the release PR carefully: confirm all crates moved to the same version and the changelog lists commits from every crate. If not, adjust `release-plz.toml` before merging.
