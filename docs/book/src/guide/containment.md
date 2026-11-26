# Blocking a network source

`firewall.block_source` drops inbound traffic from one external address or a small network for a limited time. It is a containment step: it limits a source you have decided to keep out, and it never changes your services. You approve it in the web UI, or a policy rule can allow it to run without you. Both paths end in the same place: the host's own helper decides whether it runs.

A block is applied by the root helper in a firewall table of its own, with a timeout kept by the kernel. Nothing blocked survives a reboot.

## The two steps

A block is made in two steps, so that a rule can never cut the host off from the hub for good.

1. **Apply.** The helper adds the source to a set with a 30 second timeout.
2. **Confirm.** Once the hub has seen the answer, it sends a second request within 20 seconds. That request reaching the agent and being answered proves the path works in both directions. The helper then replaces the element with the lifetime you asked for.

If the hub never confirms, because the link broke, the hub restarted or the rule itself cut the path, the kernel removes the block after 30 seconds. The hub does not apply it again by itself. A new block needs a new request, a fresh safety check and a new approval.

Both steps and an early release are phases of one block. They share one approval and one plan hash, and each phase has its own execution id.

A flood from the blocked source can delay the confirmation past 30 seconds. The block then expires. That is safe for management and does not contain the source.

## What an approval binds

The approval names the source network in its canonical form (`203.0.113.0/24`, always with its length), the lifetime, the host and the safety identity the hub checked. The lifetime is above 30 seconds and at most the limit set on the host.

**Verified path.** When the hub holds a fresh, verified report from the host's helper (see [Helper key and attestation](helper.md)), the approval carries that identity. The helper compares it with its own at every step and refuses with `safety_changed` if the set of protected paths changed.

**Unverified path.** When the report cannot be verified (no key pinned, a stale report, an incomplete one, a path not covered), only you can approve. The card says that coverage cannot be verified, gives the reason, and lists every path that is uncovered or unverified under "Not protected". You type the source network and your password, and you acknowledge the warning. The hub records that the block was approved unverified. A policy rule can never use this path.

An address that overlaps a protected range is refused on either path.

## What you set up

These local files decide whether a block can run. The hub cannot change any of them.

| Where | Setting | Effect |
| --- | --- | --- |
| `helper.toml` | `groups` includes `"nft"` | Turns the firewall operations on. Off by default. |
| `helper.toml` | `[safety] management_cidrs` | The networks you manage the host from. No block can overlap them. While the list is empty, no block runs. |
| `helper.toml` | `[safety.containment] max_active` | The most blocks held at once. The default, 0, refuses every block. |
| `helper.toml` | `[safety.containment] max_ttl` | The longest block. |
| `agent.toml` | `[[unattended.allow]]` for `firewall.block_source` | What a policy may block unattended: the `targets` (`"*"` or `ipnet:` and a canonical network), `max_per_hour`, `max_active`, `max_ttl`, and the shortest prefixes in `args` (`prefix_len_v4`, `prefix_len_v6`). |

The full keys are in [Helper](../reference/helper.md#firewall-operations) and [Configuration](../reference/configuration.md). An Untested platform, which every platform is today, also needs `allow_write_groups_on_untested = true` in `helper.toml`.

## The self-test and the drills

The helper checks, once per kernel and `nft` version, that replacing an element never leaves a window in which the source is not blocked. `domovoi helper nft-selftest` does it in a throwaway network namespace and takes minutes; the installer and a one-shot unit run it. Until it has passed for this kernel and `nft` version, the helper refuses every block (`containment_blocked`). If only single addresses passed, a network is refused with `prefix_unsupported_platform`: "Blocking whole networks is not enabled on this host; block single addresses."

`domovoi helper containment-drill` runs the helper's own operations through a set of scenarios, such as an unconfirmed block, an excluded address, a repeated request and the active limit. A policy-authorized block is dispatched only on a host where the drills have passed. A block you approve yourself does not need them.

A passing self-test is weak evidence. It does not cover a physical network card, several receive queues, a conntrack rule, lower packet rates or the live set's contents. Domovoi does not claim a block stops every packet.

## What you see

Each block shows its phase: applying, applied, confirming, confirmed until a time, expired, rolled back, lost or unknown. It also shows two facts about the host:

- `existing_flows`: `cut` means connections that were already open were removed; `n/a` means the host has no nftables flowtable, so the drop itself ends them; `may_persist` means a flowtable exists and the connections could not be removed. A block with `may_persist` is not reported as contained.
- `bridge_gap`: a physical interface is a port of a bridge, and traffic the bridge forwards does not pass the table.

Hardware offload is not covered. **Release block** on a step removes a block early; it asks for your password and the source network typed again.

**Refresh** asks the helper for the blocks it holds. It is only ever started by you; the hub does not poll.

## When it is refused

| What you see | What it means and what to do |
| --- | --- |
| `containment_blocked` | The self-test did not pass for this kernel and `nft` version, or the platform has not been enabled for this host. |
| `prefix_unsupported_platform` | Only single addresses may be blocked here. |
| `exclusion_overlap` | The source overlaps a protected range: a management network, the hub, the host's own addresses, a gateway, loopback, link-local or multicast. |
| `prefix_too_wide` | The network is shorter than a `/24` (IPv4) or `/64` (IPv6). |
| `no_management_cidrs`, `exclusions_incomplete` | The helper cannot say what it must never block. Set `management_cidrs`. |
| `ttl_too_short`, `ttl_above_maximum` | The lifetime is 30 seconds or less, or above `max_ttl`. It is never shortened for you. |
| `max_active_reached` | The limit is used up. The default of 0 refuses every block. |
| `mutation_limit` | The helper's hourly limit is used up. |
| `safety_changed`, `malformed_safety` | The protected set changed since you approved, or the request carried no safety identity. Ask again. |
| `table_drift` | Something else changed the helper's table. New blocks stop; nothing is repaired automatically. |
| `containment_lost` | The table was removed, for example by a firewall reload, while a block was live. The block is gone. The helper re-creates an empty table and never applies a lost block again; the host shows an attention item. |
| `journal_corrupt` | The helper's journal cannot be read. All blocks are refused until you repair it on the host. |
| `drills_not_passed` | A policy tried to block on a host whose drills have not passed. |

## Validated on

These are measurements, not support statements. No platform is Supported.

- Namespace tests of the helper's `nft` group ran against `nft` 1.1.7 on Linux 7.2.15, rootless in a network namespace.
- One load run on that machine, a debug build with the single-address layout and one scenario, sent 88,068,864 packets over 7,510 replacements and leaked none. The control, which replaces the element in two separate steps, leaked 73.6%, so the test can see a window. The run hit its time cap before the minimum number of replacements, so the self-test would have reported it as inconclusive and blocked containment.
- The network layout (`blockednet4`, `blockednet6`) has not been measured on any host.
- The self-test unit's capability set has not been measured on a real host.
- A kernel and `nft` version pair that has not been measured must pass the self-test on the host before it can block anything. The validation matrix is yours to run on your own hosts.
