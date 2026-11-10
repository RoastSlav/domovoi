# 31 · Security: sign releases with minisign and verify them on upgrade

Decided by the maintainer: a minisign public key compiled into the binary. Needs an ADR
and a key pair that **the maintainer** generates (the model must not create or commit a
secret key).

## Background (verified on `main`)

- `domovoi upgrade` (`crates/domovoi/src/cmd/upgrade.rs:33-75`) downloads
  `<archive>.tar.gz` and `<archive>.tar.gz.sha256` from the same place and compares them
  (`upgrade/package.rs:13-35`). That proves the download is intact, not that the
  maintainer built it: whoever can replace the archive can replace the checksum.
- The base URL is GitHub releases, or anything in `DOMOVOI_RELEASE_BASE`
  (`upgrade/fetch.rs:13-20`), and the command usually runs as root.
- `install.sh` does the same `sha256sum -c` (`:256-262`) and only suggests
  `gh attestation verify`.
- `.github/workflows/release.yml` (`:140-170`) packages, writes the `.sha256`, attests
  provenance, then `gh release upload "$TAG" dist/*.tar.gz dist/*.sha256`.

## Before you start (maintainer)

1. `minisign -G -p domovoi.pub -s domovoi.key` on a trusted machine.
2. Add the secret key file's contents as the repository secret `MINISIGN_SECRET_KEY`
   and its password as `MINISIGN_PASSWORD`.
3. Paste the public key line (the base64 line of `domovoi.pub`) into the prompt where it
   says `<PUBLIC KEY>`.

## Steps

1. **ADR** `docs/adr/0020-signed-releases.md`: archives are signed with minisign; the
   binary and `install.sh` carry the public keys; `domovoi upgrade` refuses an
   unsigned or wrongly signed archive; more than one key may be listed so the key can be
   rotated (new releases signed with the new key, binaries carrying both for a while).
   Note that the SHA-256 files stay for people who only want an integrity check.
2. **Dependency.** Add `minisign-verify` (pure Rust, MIT) to `[workspace.dependencies]`
   and to `crates/domovoi` only. Check it passes `cargo deny check`.
3. **Keys.** New `crates/domovoi/src/cmd/upgrade/keys.rs`:
   `pub const RELEASE_KEYS: &[&str] = &["<PUBLIC KEY>"];` with a doc comment on rotation.
4. **Verify.** In `upgrade.rs`, after the download, fetch `<archive>.tar.gz.minisig`
   (cap 4 KiB with `fetch::download`) and verify the archive bytes against any key in
   `RELEASE_KEYS` (`minisign_verify::PublicKey::from_base64`, `Signature::decode`,
   `verify(&archive, false)`). Refuse with
   "the release signature does not match; nothing was changed" otherwise. Keep the
   SHA-256 check. `--dry-run` also prints the `.minisig` URL.
   Print the trusted comment from the signature (it holds the file name) and refuse if it
   does not name this archive.
5. **Release workflow.** After `Package`, install minisign (`sudo apt-get install -y
   minisign`) and sign: write the secret to a temp file from `MINISIGN_SECRET_KEY`, run
   `minisign -S -s key -m "dist/$name.tar.gz" -t "$name.tar.gz"` with the password from
   `MINISIGN_PASSWORD` on stdin, delete the temp file, and upload `dist/*.minisig` with
   the rest. Never echo the secret.
6. **install.sh.** Embed the same public key. After the checksum: if `minisign` is
   installed, download `.minisig` and run `minisign -Vm "$tmp/$name.tar.gz" -P "$KEY"`,
   failing on mismatch; if it is not installed, say so in one line and print the command
   to verify by hand. Add `--require-signature` that fails when `minisign` is missing.
7. **Docs.** `docs/book/src/install/upgrading.md` and `install/binary.md`: how releases
   are signed, the public key, and how to verify by hand. `docs/book/src/reference/cli.md`:
   add the missing `domovoi upgrade` section (flags `--version`, `--dry-run`, what it
   checks, that it never restarts anything). `SECURITY.md`: one line naming the key.

## Tests (unit tests in `crates/domovoi/src/cmd/upgrade/`)

- Generate a throwaway key pair in the test with a known test vector (minisign-verify's
  docs show one; or commit a test-only key pair under
  `crates/domovoi/tests/fixtures/upgrade/` clearly named `test-only`):
  - `a_correctly_signed_archive_passes`;
  - `an_archive_signed_with_another_key_is_refused`;
  - `a_changed_archive_is_refused`;
  - `a_signature_for_another_file_name_is_refused`.
- The verify function takes the key list as a parameter so tests do not use
  `RELEASE_KEYS`.

## Done when

- A tampered archive with a matching `.sha256` is refused by `domovoi upgrade`.
- The next release uploads a `.minisig` for every archive.
