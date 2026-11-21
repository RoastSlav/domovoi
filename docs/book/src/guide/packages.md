# Packages

A package is a signed archive of skills that you install on one or more hosts. It is the only way to add skills to a host without copying files there by hand. Installing a package never enables it or runs anything in it.

A package has two files: the archive, `<name>-<version>.tar.zst`, and a detached signature file, `<name>-<version>.sig.toml`. The signature file must be format 1 and use the `ed25519` algorithm.

Packages are off by default. Turn them on with `[packages] enabled = true` in `hub.toml`, and list the owner keys the hub may use to check signatures. See [Hub reference: Packages](../reference/hub.md#packages).

## In the web UI

Open Capabilities, then the Packages tab. After you turn packages on (Settings, Packages, which asks for your password), import the archive and its signature file there. The result names who signed it and says the hub checked the signature; each host checks it again against its own keys. Then choose Install, pick the package and the hosts, review the plan code shown as `Plan` followed by eight characters, and confirm with your password and the package name typed by you. The progress page shows each host and refreshes while one is working. Retry needs a new password and a typed name, and Check host asks the agent what is really installed. The owner key lists and size limits stay in `hub.toml`.

## Trust comes from the agent

Two lists decide whether a package is accepted, and they do different jobs:

- **The hub's owner keys** (`[[packages.owner_keys]]` in `hub.toml`) let the hub refuse a package that no host would accept, and show you who signed it. This is a convenience. The hub cannot add a key to an agent.
- **Each agent's trusted keys** (`[[packages.trusted_keys]]` in `agent.toml`) are the real decision. The agent checks the signature itself and refuses anything not signed by one of its own keys.

So a package installs on a host only when its signing key is in that host's `agent.toml`. Add the key to each agent that should accept the package, and restart the agent. The hub's list alone does nothing on a host.

## The workflow

### 1. Sign the archive

Sign the archive with the private key that matches one of your owner keys, and write the signature file next to it. The signature file names the archive's SHA-256 digest and the key id.

### 2. Import it

Import the pair in one of two ways:

- **Upload** the archive and the signature file through the hub's API (`POST /api/v1/packages/import`, multipart fields `archive` and `signature`). This needs a signed-in session.
- **Command line**, on the hub host while the hub is stopped or with the offline tool:

  ```text
  domovoi package import <name>-<version>.tar.zst <name>-<version>.sig.toml
  ```

  `domovoi package list` shows what the hub holds.

The hub checks, in this order: the signature file's shape, that the key is in its owner keys and not revoked, the signature, and that the archive's digest matches the one the signature names. It streams the archive to disk and never opens it. A package that fails any check is refused with a short reason code, and nothing is stored.

Importing the same archive again does nothing. A different archive with the same name and version is refused, so a version is never replaced under you.

### 3. Preview

Ask for a plan before you install. The plan lists, for each host you pick:

- whether the host can take the package now,
- what is installed there already, if anything,
- warnings, such as a host that is offline or runs an older agent.

A plan changes a lot when hosts change. Each host has its own plan hash, and the job has one too. The install step needs the hash you saw, so you cannot install a plan you have not looked at.

### 4. Install

Start the install with:

- the list of hosts (1 to 16, each once),
- the package name and version,
- the plan hash from the preview,
- your password,
- the package name typed again in the confirm field.

The hub checks the password, checks the typed name, recomputes the plan and compares its hash with yours. If anything differs it refuses and asks you to preview again. Otherwise it creates one job with one target per host. The approval lasts for the approval window (10 minutes by default), and the job must start within it.

The hub relays to at most four hosts at once. A host can have only one unfinished target at a time, so a second install for that host is refused until the first one ends.

### 5. Check

Read the job, or the host's package list. Each target shows its status (below). A job is:

- **running** while any target is still open,
- **succeeded** when every target succeeded,
- **failed** when no target succeeded,
- **partial** otherwise.

A host's package list shows what the agent reports. Refresh it to ask the agent again.

### 6. Retry or cancel

- **Cancel** a target only while it is still queued. Once it has started, it cannot be cancelled.
- **Retry** a target that failed, did not run, or has an outcome you do not know. A retry is a new attempt with a new approval, and it re-checks the plan for that host. Only the latest attempt can be retried.

### 7. Uninstall

Uninstall works the same way, without a file transfer. It removes the package from each chosen host and deletes that host's record of it. The archive stays on the hub, and you can delete it from the hub once no job still uses it.

## Target states

| State | Meaning |
| --- | --- |
| `queued` | Waiting to start. It can still be cancelled. |
| `transferring` | The hub is sending the archive and signature to the host. |
| `dispatched` | The install request has been sent to the agent. The hub is waiting for its answer. |
| `succeeded` | The agent says the package is installed and the details match. |
| `failed` | The agent refused or failed the install. Nothing was changed. |
| `not_run` | Nothing was installed or removed. The hub stopped before the agent acted. |
| `outcome_unknown` | The request reached the agent, but the hub has no clear answer. See below. |
| `cancelled` | Cancelled while queued. |

## Failure codes

Each failed or not-run target has a code and a short message.

| Code | Meaning |
| --- | --- |
| `approval_expired` | The approval ran out before the install started. |
| `host_unavailable` | The host was not connected when the install was due to start. |
| `agent_incompatible` | The agent is too old for packages. |
| `not_advertised` | The agent does not offer the package capability, or it is turned off on that host. |
| `artifact_missing` | The archive file is gone from the hub's store. Import it again to repair the store. |
| `artifact_corrupt` | The archive on disk does not match its recorded digest. Nothing was sent. |
| `transfer_failed` | Sending the archive or signature failed, or the agent refused what it received. |
| `session_changed` | The connection to the host changed during the install. |
| `handoff_failed` | The request could not be sent to the agent. |
| `hub_restarted` | The hub restarted while the files were still being sent. |
| `result_mismatch` | The agent's answer does not match the package, version, digest or key the job asked for. |
| `agent_refused` | The agent answered with a reason the hub does not recognise. |
| `contract_unknown`, `allow_risk_denied`, `authorization_expired`, `rate_limited`, `journal_corrupt` | The agent refused the request for that reason. |

When the agent refuses, the code is one of the agent's own reasons, such as `untrusted_key`, `bad_signature` or `digest_mismatch`. The [agent protocol reference](../reference/protocol.md#package-transfers) lists them.

## What "outcome unknown" means

The hub sent the install request, then lost the answer. The agent may have installed the package, or may not have started at all. The hub does not know which, so it does not guess.

When the connection comes back, the hub asks the agent what happened, using the agent's journal. Then:

- If the journal shows the install finished, the target becomes `succeeded` and is marked as confirmed only by the journal.
- If the journal shows it failed, the target becomes `failed` with the agent's code.
- If the agent has no record, the target becomes `not_run`.
- If the journal cannot answer, or the agent restarted in a way that loses the record, the target stays `outcome_unknown`.

To settle one yourself, look at the host's package list first. If the package is there and correct, nothing more is needed. If you are not sure, refresh the host's list. Only then retry the target. A retry is a new install, and it is safe only when the package is not already on the host.

The hub never retries a target on its own after the request has been sent.

## Installing does not enable

After a successful install, the package is on disk but not enabled. Its skills do not appear for the triage agent, and nothing in the package runs. No step in the hub enables a package.

## Keeping records

Finished jobs are kept for `job_history_days` (180 by default) and then removed. A job is removed only when all its targets are finished. Archives, host records and the audit log are not removed automatically. Deleting an archive is refused while a job still uses it.

See [Security model](../security.md) for who can do what, and [Backups](../reference/backup.md) for what a restore brings back.
