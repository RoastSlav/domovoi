# Hub reference

Hub behaviour that does not belong to one guide. Settings are listed in the [configuration reference](configuration.md).

## Packages

Packages are off until `[packages] enabled = true`. Reads (status, list) work either way. Writes need the setting on. For how to use them, see the [packages guide](../guide/packages.md).

### Where packages live

The hub keeps archives under `<data_dir>/packages/`, in a folder named by the first two characters of the digest, as `<sha256>.tar.zst`. The folder is readable by the hub user only. Partial uploads are written to `<data_dir>/packages/.tmp/` and renamed into place once complete. At start-up the hub deletes anything left in `.tmp/` and any file no record refers to.

The index (name, version, digest, key id, signer label, who imported it and when) and the signature file text are in the database. Backups hold the index and the signature text but not the archive files. After a restore, a package whose file is missing shows `file_status: missing`, and an install of it is refused with `artifact_missing`. Import the archive again to repair it.

### Import

Import is an HTTP upload (`POST /api/v1/packages/import`) or `domovoi package import` on the hub host. Both run the same checks, and both store the archive before recording it. Importing the same digest again is a no-op. Importing a different archive under a name and version already in use is refused with `package_conflict`.

The hub streams the archive and never opens it. It does not decompress or unpack it.

### The hub's key list

`[[packages.owner_keys]]` lists the keys whose signatures the hub accepts on import. A key in `revoked` is refused even when it is also listed. Both lists are read from `hub.toml` only, at start-up. The web UI and the API cannot change them.

The list is a convenience. It stops the hub from storing a package no host would accept, and it names the signer. It is not trust. An agent accepts a package only when its own `[[packages.trusted_keys]]` contains the signing key, and the hub cannot add, replace or remove those keys. To install a package from an owner, put that owner's key in each agent that should accept it.

### Limits

| Setting | Default | Range |
| --- | --- | --- |
| `max_artifact_bytes` | 20 MiB | 1 KiB to 256 MiB |
| `max_store_bytes` | 512 MiB | 1 MiB to 16 GiB |
| `max_artifacts` | 64 | 1 to 1000 |
| `approval_window` | `"10m"` | 1 minute to 1 hour |
| `job_history_days` | 180 | 7 to 3650 |

An archive over `max_artifact_bytes` is refused with `too_large`. An import that would pass `max_store_bytes` or `max_artifacts` is refused with `store_full`. A signature file over 16 KiB is refused.

### Retention

Finished install jobs are removed `job_history_days` after they finish. A job is removed only when every one of its targets is finished, and the purge runs with the daily retention job. Archives, host package records and the audit log are never removed automatically. An archive is deleted only by an explicit delete, and only when no job still uses it and no other record has the same digest.

## Helper trust

For each host with a helper the hub keeps the key the owner pinned, the latest attestation outcome and the highest accepted report. Everything is computed on the hub: a fingerprint is the SHA-256 of the 32 key bytes the hub received, never a value the agent sent. Nothing is pinned by itself, including on first contact.

### Asking the helper

The hub asks a connected host's helper for a signed report every 15 minutes, when a host connects or reports a new helper status, and on **Re-check**. A re-check is refused with `recheck_too_soon` (HTTP 429) when the helper was asked less than 10 seconds before. A host without a pinned key is not asked. A nonce is valid for under 60 seconds and used once.

### What the hub accepts

A report counts as the host's verified state only when the key in the answer is the pinned key, the signature verifies, the report is complete, its hashes and generation are consistent and the generation is not lower than the highest accepted one (an equal generation must carry the same hashes). Trust follows the **latest outcome**: a different key, an incomplete report or a generation that went backwards revokes the verified state at once, and the next good report restores it. A report stays fresh for under five minutes, counted from when the hub issued its nonce; after that the host reads `attestation_stale`.

Why a host is not verified is one of nine reasons: `key_not_pinned`, `key_mismatch`, `attestation_missing`, `attestation_stale`, `signature_invalid`, `report_incomplete`, `generation_regressed`, `path_uncovered` and `confirmation_stale`. Finer detail goes to the audit log. The [helper guide](../guide/helper.md) says what each means.

### Endpoints

| Endpoint | Purpose |
| --- | --- |
| `GET /api/v1/hosts/{name}/helper` | The helper's state as the agent said it, the received key and its fingerprint, the pin, the key state and the attestation state, and the verified set while it is verified and fresh. |
| `POST /api/v1/hosts/{name}/helper/recheck` | Ask now. Answers 202; 409 when the host is not connected; 429 `recheck_too_soon`. |
| `PUT /api/v1/hosts/{name}/helper/key` | Pin the received key. Needs the account password again and the full fingerprint typed from `domovoi helper fingerprint`. 409 `no_helper_key`, 422 `fingerprint_mismatch`, 401 `wrong_password`. |
| `DELETE /api/v1/hosts/{name}/helper/key` | Remove the pin. Needs the password again. Containment stays blocked until a key is pinned again. |

Pinning and unpinning work from the web UI only, never from chat. The pin, the unpin and each change of outcome (key mismatch, revoked, restored, generation regressed) are audited. Deleting a host deletes its pin. The Helper tab of the Capability Center shows all of this per host.
