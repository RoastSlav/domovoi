# Helper key and attestation

On a host installed with the three-identities layout, a small root program, the [helper](../reference/helper.md), can say what it protects: the management networks, the protected units and the routes that keep the hub and you reachable. The hub trusts that statement only if it is signed with a key you pinned yourself. This page covers enrolling that key, rotating it and reading the status. For why it works this way, see [Helper attestation](../security.md#helper-attestation).

Nothing here is needed to monitor a host. An unverified helper blocks only unattended containment on that host.

## Enrol a helper

1. On the host, make sure the helper has a key. The installer runs `sudo domovoi helper init`, which creates it and prints its fingerprint. Run it yourself if the installer did not; running it again when the key already exists changes nothing.
2. If the hub is reached by a name (`hub_url = "wss://hub.lan:7421"`) the helper cannot name the hub's address. Add the hub's network to `management_cidrs` in `/etc/domovoi/helper.toml` (for example `["192.168.1.0/24"]`). The next report picks it up. A hub on the same machine as the agent (single-box mode) is reached through the local routing table, which a report cannot express, so the helper signs an incomplete report (`route_unrepresentable`) and that host stays unverified.
3. In the web UI open Capabilities, then the Helper tab, and pick the host. Once the agent has connected, the hub shows the key as received and not pinned, with its fingerprint in groups of four. That key is unverified: the agent sent it.
4. On the host run `sudo domovoi helper fingerprint`. Read the whole fingerprint, type it into the hub and enter your account password. Both are needed. Look at every group of four, not only the start.
5. The hub pins the key and asks the helper for a report. After a moment the host reads as verified, with the generation, the age of the report and the effective set: networks, protected units and containers, the limits and the routes found.

The hub asks again every 15 minutes. **Re-check** asks now, at most once every 10 seconds. A report is fresh for five minutes after it was requested; a helper that has not answered by then reads `attestation_stale`.

## Rotate the key

Rotate when you suspect the key was read, or as housekeeping.

1. On the host run `sudo domovoi helper rotate-key`. It keeps the old public key and state under `retired/` for audit, deletes the old private key and prints the old and new fingerprints.
2. The hub shows that the helper key changed and the host is unverified (`key_mismatch`). The old pin is kept; the hub never swaps it by itself.
3. Compare the new fingerprint as in "Enrol a helper" and pin it again. The old pin is replaced and the change is audited.

To stop trusting a helper altogether, unpin it (password required). Containment stays blocked until a key is pinned again. A host deleted from the hub loses its pin. Re-enrolling the agent does not change the pin.

`sudo domovoi helper recover` is for a helper whose state file is missing, damaged or foreign. It always makes a new key, so you pin again afterwards.

## What each status means

| Status | Meaning and what to do |
| --- | --- |
| `key_not_pinned` | No key is pinned for this host. Enrol it as above. |
| `key_mismatch` | The helper presents a key other than the pinned one. If you rotated or recovered, pin the new key. If you did not, treat the host as suspect: the pinned key is kept and the next honest report from it restores the status. |
| `attestation_missing` | The hub holds no verified report. The helper may be absent, not initialised, unreachable or unable to sign (`uninitialized`, `key_missing`, `key_corrupt`, `helper_state_permissions`, `lock_unavailable`, `helper_unavailable`). The Helper tab shows the token the agent sent; it is a hint, not proof. See the [token table](../reference/helper.md#safety-attestation). |
| `attestation_stale` | The last verified report is more than five minutes old. Use Re-check, and check that the host is connected. |
| `signature_invalid` | A report did not verify with the pinned key, or was not in the one accepted form. Re-check; if it persists, look at the helper and the agent's logs. |
| `report_incomplete` | The helper signed a report that says it could not compute the set. The reason is shown, such as `route_unrepresentable` (a blackhole route, a multipath route or a hub on the same machine), `state_missing` or `state_corrupt` (run `sudo domovoi helper recover`) or `config_invalid` (fix `helper.toml`). |
| `generation_regressed` | The helper's generation went backwards, or repeated with different contents, which means its state was rolled back or replaced. Investigate on the host. If you restored it on purpose, run `recover` and pin the new key. |
| `path_uncovered` | A management path you want to confirm is not covered by the verified set. Add it to `management_cidrs` or `protected_units` in `helper.toml`. |
| `confirmation_stale` | You confirmed a management path earlier and the helper's configuration or routes have since changed, or the confirmation is over 30 days old. Confirm it again. |

A status can change from one report to the next. The hub follows the latest outcome: a bad answer revokes at once, and the next good one restores.

## Retracting a management path

Retracting a management path does not need your password. It only removes the path from use, and the action is logged. Accepting and confirming a path do need the password.
