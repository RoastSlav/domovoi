# H. WSL2 research environment (owner-executed record)

Scope: the owner's existing Windows Subsystem for Linux 2 installation, recorded as the initial Linux
research environment for prompts 53, 54, 55 and 56. Nothing in this document was produced by a coding
agent and nothing was implemented: every command and result below was run by the owner in a terminal on
that machine and reported here verbatim.

Labels used in this record:

- **OBSERVED** — output the owner saw and reported.
- **ESTABLISHES** — what that output proves, stated as narrowly as the evidence allows.
- **NOT TESTED** — untested, and stays untested until someone runs it.

This is an environment record, not a research result. It makes WSL2 usable for scoped, preliminary
experiments. It does not accept R-2, R-3, R-8, R-9 or R-10, and it is not production evidence for any
supported distribution.

## 1. Environment

| Property | Observed value |
|---|---|
| Environment | Windows Subsystem for Linux 2 |
| Distribution | Ubuntu 26. Exact point release and `VERSION_ID` were not captured; read `/etc/os-release` before relying on one, and never infer a point release from "26" |
| Kernel | `6.18.58.1-microsoft-standard-WSL2` |
| Init process | `systemd` (PID 1) |
| systemd | `259.5-0ubuntu3.4` |
| systemd state | `running` |
| nftables | `1.1.6` |
| Mount namespace | supported |
| Network namespace | supported |
| WSL networking mode | NAT |
| Hyper-V Firewall | enabled |
| Localhost forwarding | enabled |
| DNS proxy | enabled |
| Auto proxy | enabled |

OBSERVED: `sudo nft list ruleset` printed nothing.

ESTABLISHES: at that moment, the root-visible nftables ruleset of that WSL2 instance was empty — no
tables, chains, sets or rules were displayed.

This is **not** a comprehensive firewall inspection, and it must not be read as one:

- WSL2 in NAT mode forwards through the Windows host, so packet filtering for real traffic can also
  happen in Hyper-V Firewall and in Windows firewall policy, which `nft` inside the distribution cannot
  see.
- The absence of rules says nothing about `firewalld`, `ufw` or Docker rule management, which were not
  installed or started, or not inspected.
- It says nothing about a conventional Linux host, where a base ruleset usually exists.
- It is a single observation at one time; the ruleset is not frozen.

## 2. Baseline checks

Run by the owner:

```bash
uname -r
ps -p 1 -o comm=
systemctl --version | head -n 1
systemctl is-system-running
sudo unshare --mount --net true
nft --version
sudo nft list ruleset
```

OBSERVED results:

| Command | Result |
|---|---|
| `uname -r` | `6.18.58.1-microsoft-standard-WSL2` |
| `ps -p 1 -o comm=` | `systemd` |
| `systemctl --version \| head -n 1` | systemd 259.5 (`259.5-0ubuntu3.4`) |
| `systemctl is-system-running` | `running` |
| `sudo unshare --mount --net true` | exited 0: both namespaces were created |
| `nft --version` | `nftables v1.1.6` |
| `sudo nft list ruleset` | no displayed output |

ESTABLISHES: systemd is the real init under WSL2 here, not the compatibility shim; `systemctl` can
report state; unprivileged-with-`sudo` creation of mount and network namespaces works; the nftables CLI
is installed and runnable as root.

## 3. Isolated nftables experiment

Run by the owner inside a throwaway network namespace:

```bash
sudo unshare --net bash
```

Inside that shell:

1. Created an `inet` table `domovoi_probe`.
2. Created an IPv4 set `blocked4` with a default timeout of 30 seconds.
3. Inserted `198.51.100.10` with a 30-second timeout.
4. In one `nft -f -` batch, deleted the existing element and added it again with a 60-second timeout.
5. Queried the set.

OBSERVED: the set listed the replacement element with about **59.98 seconds** remaining, i.e. the
element's timeout was the new 60 seconds, not the original 30.

The whole sequence succeeded.

### 3.1 What the result establishes

- The kernel in this WSL2 instance supports the tested `inet` table operation.
- IPv4 sets with the `timeout` flag work, including stateful per-element expiry.
- A per-element `timeout` overrides the set's default timeout.
- The delete-and-add batch was accepted and applied as one `nft -f` transaction: the replacement carried
  the new timeout, so the delete was not silently ignored and the add did not collide with a surviving
  element.
- nftables operations can be exercised in a separate network namespace, so the experiment does not touch
  the instance's own ruleset.

### 3.2 What the result does not establish

- It does not show that packets traverse Domovoi's intended chains, hooks or priorities; no packet was
  sent, counted or dropped in this experiment.
- It does not show coexistence with an existing base ruleset, `firewalld`, `ufw` or Docker.
- It does not show that no packet observed an intermediate state during the batch. The batch was applied
  as one transaction, which is the property `04 §9.1` relies on; independently proving the netlink-level
  atomicity under live traffic is part of R-2, not of this test.
- It does not show behaviour under set size limits, resource pressure, failure cases, reboot or recovery.
- It says nothing about production safety or about any distribution other than this WSL2 instance.

### 3.3 Reproduction

Save as `probe-nft.sh` and run it as root inside a fresh network namespace:

```bash
sudo unshare --net ./probe-nft.sh
```

```bash
#!/usr/bin/env bash
# nftables timeout-replacement probe. Runs in whatever network namespace it is started in.
# The namespace owns its nftables state, so nothing survives the namespace; the trap is belt and braces.
set -euo pipefail

table=domovoi_probe
set4=blocked4

cleanup() { nft delete table inet "$table" 2>/dev/null || true; }
trap cleanup EXIT

nft add table inet "$table"
nft add set inet "$table" "$set4" '{ type ipv4_addr; timeout 30s; }'
nft add element inet "$table" "$set4" '{ 198.51.100.10 timeout 30s }'

echo '--- before'
nft list set inet "$table" "$set4"

# Give the first timeout some room to run down, so a replacement is visible in the output.
sleep 2

# One transaction: delete the element, add it back with the longer timeout.
nft -f - <<'EOF'
delete element inet domovoi_probe blocked4 { 198.51.100.10 }
add element inet domovoi_probe blocked4 { 198.51.100.10 timeout 60s }
EOF

echo '--- after'
nft list set inet domovoi_probe blocked4
```

Expected shape of the last listing: one element for `198.51.100.10` with `expires` just under `60s`
(the owner saw about `59.98s`). If the listing reports about `28s` instead, the delete-and-add batch did
not replace the element and R-2 must treat `replace_ttl` as broken on this environment.

Requirements: `sudo` rights, nftables 1.1.6 or later, and a kernel that permits `unshare --net` under
root, all of which this environment has. The probe must stay inside a network namespace: on a real host
it would modify the live ruleset.

## 4. Evidence limitations

Not tested in this environment, and not implied by anything above:

- actual inbound and outbound packet traversal through Domovoi's intended chains and hooks;
- hook priority interactions;
- coexistence with existing nftables tables and chains;
- `firewalld` integration;
- `ufw` integration;
- Docker networking and firewall integration;
- real physical NIC behaviour;
- VPN and tunnel route interactions;
- production firewall recovery and reboot behaviour;
- conventional Linux host behaviour outside WSL2;
- nftables failure cases under actual host network traffic.

Consequences for the research prompts:

- WSL2 in NAT mode has no physical NIC, no bridged LAN interface and no host firewall ruleset to
  coexist with. Any finding that depends on those is untestable here.
- WSL2 is a virtualised environment with a Microsoft kernel; a result that holds here is a result about
  this environment.
- WSL2 results must not be represented as evidence for every supported Linux distribution, and a WSL2
  result alone does not close any research item.

## 5. How this record is used

- Ubuntu WSL2 is the owner's available Linux research environment. It is sufficient to author and
  launch prompts 53, 54 and 56 for WSL2-scoped research, and prompt 55 needs no environment gate at all.
- Every research report that uses it must name the environment it ran in and label each finding as one
  of: **demonstrated on WSL2**, **not demonstrated**, **requiring conventional Linux validation**.
- R-2, R-3, R-8, R-9 and R-10 stay open. Acceptance is recorded in the ADRs and decision register, only
  after the evidence those items require exists, and none of it is satisfied by an environment being
  available.
- The H1 and M8/M9 implementation gates are unchanged: prompts 44, 45, 46 and 47 keep the gates in
  `07 §6.4`, and the milestone readiness table in `06` is not advanced by this record.
