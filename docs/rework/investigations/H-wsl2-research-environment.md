# H. WSL2 research environment (recorded experiments)

Scope: the owner's existing Windows Subsystem for Linux 2 installation, recorded as the initial Linux
research environment for prompts 53, 54, 55 and 56. Nothing was implemented. §1 to §3.3 record commands
and results the owner ran in a terminal on that machine and reported verbatim; §3.4 was run later by a
coding agent in the same environment and is labelled as such. No command below changed a production
ruleset: every nftables experiment ran inside a throwaway network namespace.

Labels used in this record:

- **OBSERVED** — output that was seen and reported; the subsection says who ran it.
- **ESTABLISHES** — what that output proves, stated as narrowly as the evidence allows.
- **NOT TESTED** — untested, and stays untested until someone runs it.

This is an environment record, not a research result. It makes WSL2 usable for scoped, preliminary
experiments. It does not accept R-2, R-3, R-8, R-9 or R-10, and it is not production evidence for any
supported distribution.

## 1. Environment

| Property | Observed value |
|---|---|
| Environment | Windows Subsystem for Linux 2 |
| Distribution | Ubuntu 26.04.1 LTS (Resolute Raccoon), `VERSION_ID="26.04"` from `/etc/os-release` (read on 2026-11-13) |
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
report state; creation of mount and network namespaces with `sudo` works (namespace creation without root
privileges was not tested); the nftables CLI is installed and runnable as root.

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
  element. §3.4 tests the failing case of the same mechanism.
- nftables operations can be exercised in a separate network namespace, so the experiment does not touch
  the instance's own ruleset.

### 3.2 What the result does not establish

- It does not show that packets traverse Domovoi's intended chains, hooks or priorities; no packet was
  sent, counted or dropped in this experiment.
- It does not show coexistence with an existing base ruleset, `firewalld`, `ufw` or Docker.
- It does not show that no packet observed an intermediate state during the batch. §3.4 shows that a
  failed batch commits nothing at the ruleset level; it says nothing about what a packet sees while a
  successful batch is applied, which is the property `04 §9.1` relies on and part of R-2 under live
  traffic.
- It does not show behaviour outside the single failure case in §3.4: set size limits, resource
  pressure, other failure modes, reboot and recovery are untested.
- It says nothing about production safety or about any distribution other than this WSL2 instance.

### 3.3 Reproduction

Save as `probe-nft.sh`, then run it as root inside a fresh network namespace. Invoking it through `bash`
means the file needs no executable bit and no `chmod` step:

```bash
sudo unshare --net bash ./probe-nft.sh
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
nft add set inet "$table" "$set4" \
    '{ type ipv4_addr; flags timeout; timeout 30s; }'
nft add element inet "$table" "$set4" '{ 198.51.100.10 timeout 30s }'

echo '--- before'
nft list set inet "$table" "$set4"

# Give the first timeout some room to run down, so a replacement is visible in the output.
sleep 2

# One transaction: delete the element, add it back with the longer timeout.
# The batch runs as an `if` condition, so `set -e` does not abort before the state is inspected.
if nft -f - <<'EOF'
delete element inet domovoi_probe blocked4 { 198.51.100.10 }
add element inet domovoi_probe blocked4 { 198.51.100.10 timeout 60s }
EOF
then
    echo 'Replacement transaction succeeded.'
    echo '--- after'
    nft list set inet "$table" "$set4"
else
    batch_status=$?

    echo "Replacement transaction failed (exit $batch_status)." >&2
    echo '--- state after failed transaction' >&2
    nft list set inet "$table" "$set4" ||
        echo 'Could not inspect the set after the failed transaction.' >&2

    exit "$batch_status"
fi
```

**Expected shape on success.** The final listing shows one element for `198.51.100.10` with `expires`
just under `60s` (the owner saw about `59.98s`). If instead it reports about `28s`, the delete-and-add
batch did not replace the element and R-2 must treat `replace_ttl` as broken on this environment.

**Expected shape on failure.** A failed batch is aborted, not partially applied: nftables submits the
file's commands as one kernel transaction, and if any command fails, none of the batch's changes are
committed. The script exits non-zero with nftables' own status, does not print the `after` listing, and
prints the set state before the namespace disappears.

- The `delete element` in that batch does not survive. The original element should still be present
  with its remaining TTL — about 28 seconds here, because it was added with 30 seconds and the script
  slept for two before the batch. That figure is an estimate: the reading decreases while the commands
  run, and Bash startup and `nft` invocation add time.
- A missing element is not evidence of partial application. Either the element expired independently
  before the batch (the 30-second TTL limits how long it can exist) or something else changed the set.
  Compare the `before` listing with the failure time: with a 30-second TTL, an element that was alive
  two seconds earlier cannot plausibly have expired. §3.4 repeats the check with a 300-second TTL, where
  expiry and a failed transaction cannot be confused.

The failure branch has been checked for Bash syntax and its control flow exercised with a stand-in `nft`
command. Transaction rollback itself is demonstrated in §3.4 with a batch the kernel rejects after
processing earlier commands; this replacement probe's own failure path was not triggered in the owner's
run, which succeeded.

Requirements: `sudo` rights, nftables 1.1.6 or later, and a kernel that permits `unshare --net` under
root, all of which this environment has. The probe must stay inside a network namespace: on a real host
it would modify the live ruleset.

### 3.4 A batch rejected by the kernel is rolled back in full

Run by a coding agent on 2026-11-13 in this WSL2 environment (`Ubuntu 26.04.1`, nftables 1.1.6), inside
`sudo unshare --net`, so the instance's own ruleset was never touched.

The documented behaviour: nftables handles the commands in one `nft -f` file in a single kernel
transaction, and the nftables wiki's scripting and atomic-replacement pages describe `nft -f` as applying
a file atomically rather than command by command. A batch should therefore commit either in full or not
at all. This subsection tests that with a failure the kernel produces *after* earlier commands in the
batch were accepted.

**Failure mechanism: set capacity.** The set is created with `size 2` and holds two committed elements.
The batch then deletes one element and adds two new ones. Every command is syntactically valid and every
identifier and type resolves, so nftables userspace evaluation accepts all of them; the kernel is what
refuses the batch, at the second `add element`, because committing it would put more elements in the set
than its declared size allows. nft userspace does not track the set's current element count, so this
diagnostic cannot come from it. A control run with the identical three commands against a set of
`size 4` succeeds and leaves `count 3`, which confirms the rejection is capacity, not syntax or
semantics.

**How the submission was established.** nft's own netlink debugging (`nft --debug=netlink`) dumps the
messages it serializes for the batch. For the failing batch the dump contains the `delete element` for
`198.51.100.10` and one message for each of the two added elements, so all three commands were serialized
and sent, not filtered out earlier. The rejection is reported as
`Error: Could not process rule: Too many open files in system` — nft's rendering of the batch's netlink
error (`ENFILE`, the kernel's set-full error here), not a parse or evaluation diagnostic, and it names
the second `add element` as the failing command.

**Userspace rejection is not rollback evidence.** A batch that names a set that does not exist fails
earlier, during userspace evaluation: it prints `Error: No such file or directory`, has no
`Could not process rule:` prefix, and its debug dump contains only the set lookup, with no element
messages — the element commands were never serialized. Such a batch says nothing about kernel rollback.
The capacity test above is the one that reaches the kernel.

```bash
#!/usr/bin/env bash
# Kernel-side batch failure: a valid delete and a valid add must both be rolled back.
set -euo pipefail

table=domovoi_probe_kernel
set4=blocked4
failed=0

cleanup() { nft delete table inet "$table" 2>/dev/null || true; }
trap cleanup EXIT

nft add table inet "$table"
# size 2 holds the two committed elements but not the extra inserts in the failing batch.
nft add set inet "$table" "$set4" \
    '{ type ipv4_addr; flags timeout; timeout 300s; size 2; }'
nft add element inet "$table" "$set4" \
    '{ 198.51.100.10 timeout 300s, 198.51.100.11 timeout 300s }'

echo '--- before'
nft list set inet "$table" "$set4"

# One batch: delete a committed element, add two more. The third command overflows the set,
# and the kernel rejects it after the delete and the first add were accepted.
if nft -f - <<'EOF'
delete element inet domovoi_probe_kernel blocked4 { 198.51.100.10 }
add element inet domovoi_probe_kernel blocked4 { 198.51.100.20 timeout 300s }
add element inet domovoi_probe_kernel blocked4 { 198.51.100.21 timeout 300s }
EOF
then
    echo 'UNEXPECTED: the batch succeeded, so this probe proves nothing.' >&2
    exit 1
else
    batch_status=$?
    echo "Batch rejected by the kernel (exit $batch_status)." >&2
fi

echo '--- after failed batch'
if ! after=$(nft list set inet "$table" "$set4"); then
    echo 'FAILED: could not inspect the set after the failed batch.' >&2
    exit 2
fi
printf '%s\n' "$after"

for kept in 198.51.100.10 198.51.100.11; do
    if ! printf '%s' "$after" | grep -q "$kept"; then
        echo "FAILED: $kept is missing, so the batch was not rolled back." >&2
        failed=1
    fi
done
for unexpected in 198.51.100.20 198.51.100.21; do
    if printf '%s' "$after" | grep -q "$unexpected"; then
        echo "FAILED: $unexpected was committed despite the failed batch." >&2
        failed=1
    fi
done

if [ "$failed" -eq 0 ]; then
    echo 'Rollback verified: the delete did not stick and neither new element was committed.' >&2
fi

exit "$failed"
```

Run it like the other probes, with the file saved as `probe-nft-kernel-failure.sh`:

```bash
sudo unshare --net bash ./probe-nft-kernel-failure.sh
```

OBSERVED (agent-run):

```text
--- before
size 2 # count 2
elements = { 198.51.100.10 expires 4m59s996ms,
             198.51.100.11 expires 4m59s996ms }
/dev/stdin:3:50-62: Error: Could not process rule: Too many open files in system
add element inet domovoi_probe_kernel blocked4 { 198.51.100.21 timeout 300s }
Batch rejected by the kernel (exit 1).
--- after failed batch
size 2 # count 2
elements = { 198.51.100.10 expires 4m59s964ms,
             198.51.100.11 expires 4m59s964ms }
Rollback verified: the delete did not stick and neither new element was committed.
```

ESTABLISHES:

- The batch reached the kernel with all three commands, as the netlink debug dump shows, and the kernel
  rejected it at the second `add element` (`ENFILE`); the error names that command, not the earlier ones.
- The delete did not survive: `198.51.100.10` is still in the set after the batch, 32 milliseconds later
  in its own TTL. Had the delete been committed, the element would be gone.
- Neither added element was committed: `198.51.100.20` and `198.51.100.21` are absent. The set still
  reports `count 2`, matching the original state.
- Expiry cannot explain this. The elements carry a 300-second timeout, the whole test takes milliseconds,
  and the TTL shows the expected small decrease (4m59s996ms to 4m59s964ms), so they were alive before
  and after the batch.
- The probe exits 0 only when the batch failed, the resulting set was read successfully, and both
  expectations hold. A failed read exits 2, an unexpected success exits 1, and a state mismatch exits 1.

NOT TESTED: the same behaviour on other nftables versions; failure modes where the kernel has already
begun committing (no such path is expected, but none was observed); and whether a packet can observe an
intermediate state during a successful batch (`04 §9.1`), because this probe inspects the ruleset after
the batch rather than traffic during it.

The earlier `no_such_set` batch that this subsection replaces was rejected during userspace evaluation —
it is kept only as the contrast case above, and it is not evidence of kernel rollback.

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
