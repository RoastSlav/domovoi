# 53 — R-2 nftables containment research

| | |
|---|---|
| Branch | `spike-nft-containment` |
| Milestone / research item | R-2 (nftables containment on supported Linux platforms); validation gate for M9 and prompt 46 |
| Dependencies | none. Read `03 RW-13`, `04 §9.1`, `03 RW-37` (exclusions and the safety identity), and `99` R-2. Attestation encoding is out of scope here |
| Can run alongside other research | yes: 54 and 56 are independent; 55 is separate and security-sensitive |
| Produces | R-2 evidence: `docs/rework/investigations/I-r2-nftables-containment.md` (take the next free letter if `I` is taken at run time) |
| Launch gate | Ubuntu WSL2 available (`investigations/H-wsl2-research-environment.md`); no other gate |
| Suggested model | medium |

## 1. Objective

Answer one question with measured evidence: **can the containment mechanism described in `04 §9.1` be
applied safely on supported Linux hosts, with automatic kernel expiry, atomic replacement, reliable
recovery and enforced management-path exclusions?**

This is research. The deliverable is a report and the probe scripts that produced it, not production
code. Nothing in this brief authorises implementing containment, enabling it, or accepting R-2.

## 2. What this prompt is not

- It is not prompt 46 (`containment`). No product code, no protocol change, no helper, no capability.
- It does not decide the supported distribution or nftables version set. It records what was tested.
- It does not accept R-2. Acceptance is recorded by the owner in the ADRs and the decision register.
- It does not modify the owner's real firewall. Every experiment runs in a throwaway network namespace or
  a disposable persistent namespace that this prompt creates and deletes.

## 3. Architecture in plain language

The v1 containment capability is `firewall.block_source` (`04 §9.1`, D-17, D-22, D-28, RW-13):

- **Target**: an `ipnet:{canonical}` derived from a detector signal's structured field, with a prefix at
  least as specific as the ceiling minimum and equal to the pinned subject.
- **Privilege**: a root-owned helper's `nft` group, limited to set elements in the table `inet domovoi`.
  No other table, chain or rule is touched. Mandatory exclusions are re-checked by the helper.
- **Expiry**: an nftables set element `timeout` equal to the TTL, at most `max_ttl`. The kernel expires
  it, so it does not depend on the agent, hub or helper process surviving.
- **Two-step apply**: step 1 adds the element with a 30-second kernel timeout; step 2, after hub
  confirmation, replaces the timeout in one `nft -f` transaction (`delete element` + `add element …
  timeout <TTL>`), because a plain `add element` is not relied on to refresh a timeout. If the element
  already expired, the batch fails and the helper reports `expired` rather than silently re-adding it.
- **Rollback**: `delete element`, idempotent; an absent element is success.
- **Outcome unknown**: read membership from the helper; the journal decides `NotRun` versus
  `OutcomeUnknown`.
- **Exclusions**: management CIDRs, the hub address, link peer, loopback and link-local addresses come
  from the root-owned helper `[safety]` configuration. A confirmed management path must be covered by a
  verified, fresh helper attestation (`RW-37`), and the helper recomputes its actual set and refuses
  overlap immediately before applying.
- **Reconciliation**: on agent restart the agent lists elements and reconciles with its journal; the
  hub reconciles through `action_status` and the active list.

R-2 exists because the design's security properties depend on nftables behaviour that has not been
measured: where the base chains sit, whether packets actually traverse them, how they interact with
firewalld, ufw and Docker rules, what the set limits and failure modes are, whether the replacement
transaction is atomic under load, and whether the helper can enforce exclusions at all.

## 4. Repository and environment context

Current state (verify before relying on it):

- **No containment code exists.** There is no nftables, firewall, `block_source`, helper or containment
  module in `crates/`. The design is documentation only.
- Relevant planned types: the `ipnet:` target form is part of the stable target identity (`03 RW-34`);
  the firewall capability ids (`firewall.block_source`, `firewall.confirm_block`,
  `firewall.list_blocks`) travel over the wire 1.2 `run_capability` / `action_status` contract defined by
  prompt 20, which is planned and may not be merged when you run this.
- Existing network facts and checks live under `crates/domovoi-checks/src/net/` and
  `crates/domovoi-core/src/` (host/network types); they do not manage firewall rules.
- The protocol crate is `crates/domovoi-proto/`; it has no firewall message today.
- The record of this environment is `docs/rework/investigations/H-wsl2-research-environment.md`:
  Ubuntu 26.04.1 LTS, kernel `6.18.58.1-microsoft-standard-WSL2`, systemd 259.5, nftables 1.1.6, NAT
  networking, Hyper-V Firewall enabled. It already contains one isolated timeout-replacement probe and
  one kernel-rollback probe. Those are environment records, **not** R-2 evidence.
- The nftables design constraints are in `docs/rework/04-target-architecture.md` §9.1, §7.3 and
  `docs/rework/03-architecture-decisions.md` RW-13 and RW-37.

Proposed outputs of this prompt, and nothing else:

- `docs/rework/investigations/I-r2-nftables-containment.md` — the report.
- Probe scripts under `docs/rework/investigations/probes/` only if the report embeds them; otherwise
  inline in the report. Do not add code under `crates/`.

## 5. Evidence discipline

Label every finding in the report as exactly one of:

- **demonstrated on WSL2** — observed in this environment, with the command and output included;
- **not demonstrated** — not tested, or the test was inconclusive;
- **requiring conventional Linux validation** — depends on a physical NIC, a real base ruleset, a
  distro firewall manager, Docker, another kernel or another nftables version.

Rules:

- A successful experiment is evidence, not acceptance. Never write "R-2 is resolved", "containment is
  production-ready" or "verified on supported platforms".
- Record the exact environment for every run: `uname -r`, `nft --version`, `systemctl --version`, WSL
  mode, and whether the run was inside `unshare --net`.
- Distinguish "the ruleset contains the intended rule" from "a packet was seen by it". Most failures in
  this area are the second kind.
- Keep raw output excerpts short but sufficient to reproduce the claim; do not paste whole dumps.
- Commands in §7 that are not taken from H are reference sequences: verify their syntax and identifiers
  before running them, and record the command you actually ran.

## 6. Safety and isolation

Mandatory:

1. Never run an experiment against the default network namespace's ruleset. Start with
   `sudo unshare --net bash`, or a disposable `ip netns` namespace created for this prompt.
2. Never flush, delete or modify an existing table, chain, set or rule you did not create. Every
   experimental table name starts with `domovoi_probe`: the bare `domovoi_probe` is the stand-in, and a
   second table uses a suffix such as `domovoi_probe_kernel`.
3. Never test with addresses that could affect the owner's connectivity, and never touch the WSL2 host,
   Hyper-V Firewall or Windows firewall. Use `198.51.100.0/24` (TEST-NET-2), `203.0.113.0/24`
   (TEST-NET-3) and `10.99.0.0/24` inside namespaces.
4. Clean up with a `trap` that deletes your tables and namespaces, including on failure. A persistent
   `ip netns` used for the process-death test must be deleted at the end.
5. Creating veth pairs, addresses and routes is allowed **only inside** your namespaces.
6. Do not install firewalld, ufw or Docker to "test coexistence". Their absence is a finding to record,
   not a gap to fill by changing the host.
7. Do not reboot the WSL2 instance to test persistence; the process-death and namespace tests cover part
   of that question, and reboot behaviour is left for conventional Linux validation.
8. Do not run the probe as a service, and do not leave a background process running.

## 7. Experiment matrix

Run every row. `E1`–`E13` are the minimum; add rows when an observation raises a new question, and say so.

| ID | Question | Method | What to record | What it can and cannot establish |
|---|---|---|---|---|
| E1 | What is the baseline environment and ruleset? | `uname -r`; `nft --version`; `systemctl --version`; `sudo nft list ruleset`; `sudo nft list tables`; inside `unshare --net`, repeat | Exact versions; whether the default namespace has tables; what a fresh namespace contains | Baseline only. An empty WSL2 ruleset is not a conventional host. |
| E2 | Can a disposable table be created and populated without touching other tables? | Create `table inet domovoi_probe` and an IPv4 set with `flags timeout` inside a namespace; list all tables; delete only your table. `domovoi_probe` is the test stand-in for the proposed production table `inet domovoi`; never create a production-named table | Table/set listing; whether other tables changed; the exact table names used | Table isolation for the stand-in only. It says nothing about the production table's name or coexistence. |
| E3 | Where should base chains sit (hooks and priorities), and do packets traverse them? | Three namespaces — sender, router (forwarding enabled) and receiver — connected by two veth pairs, as specified in §7.1; base chains with `counter` on sender `output`, router `forward` and receiver `input`; send ICMP/UDP from the sender to the receiver | Which counters move in each direction; whether the `forward` chain sees routed traffic; the forwarding and route configuration used | Traversal in this exact topology with `domovoi_probe` tables. Not physical NICs, bridges, NAT or Hyper-V Firewall. |
| E4 | Does a `drop` in one base chain override an `accept` in another at the same hook? | Two base chains on `input`, priorities `0` and `10`; one accepts, one drops the test source; observe delivery | Whether the packet is delivered; counter deltas; ordering effect when the priorities are swapped | Same-hook base-chain ordering in this kernel. Not firewalld's real chains. |
| E5 | Does an element in a set actually block traffic at the intended hook? | In `inet domovoi_probe`, add the sender's address to the set referenced by an `input` rule with `drop`; send traffic again | Loss and counters before/after | A rule matched a packet in this topology. Not coexistence with an existing filter policy. |
| E6 | What are the set size and resource limits? | Create sets with `size 2`, `size 1000`, `size 10000`; fill to capacity; add past capacity; measure time and memory with `/usr/bin/time -v` if it is installed, otherwise use `time` and `/proc/<pid>/status` | The exact error and exit status at capacity; throughput for 1k and 10k inserts; whether expiry frees slots | Limits on this kernel/nft version. Not the production `max_elements` value. |
| E7 | What TTL syntax is accepted, and what is refused? | `timeout 0s`, `-1s`, `1h30m`, `1000000s`, a non-numeric value, an element without the set's `timeout` flag; use `nft -c` and a real add | Accepted grammar; exact refusal messages; whether 0 means "never" | Validation behaviour, not expiry accuracy under load. |
| E8 | Is per-element timeout expiry reliable and independent of user processes? | Add an element with a 3-second timeout to a set in `inet domovoi_probe` in a persistent namespace (`ip netns add domovoi_probe_persist`); exit the shell; inspect after 4 seconds with a new `ip netns exec`; also observe a 30-second element | `expires` values and disappearance time; state after the creating process exits | Kernel expiry independence from a process. Not reboot behaviour. |
| E9 | Does delete-and-add replace the timeout atomically, and what happens if the element already expired? | Re-run H §3.3's replacement probe, adapted to your own `domovoi_probe` table; then add a 1-second element, wait 2 seconds, and run the same batch | Remaining TTL on success; exact failure message and exit status when the element is gone; whether the add silently re-creates it | Transaction result, not that no packet saw an intermediate state. |
| E10 | Is a kernel-rejected batch rolled back in full? | Re-run H §3.4's capacity-overflow probe in a table prefixed `domovoi_probe_` (H uses `domovoi_probe_kernel`); confirm the delete did not stick and neither add committed | Exit status, netlink diagnostic, final `count` and elements | Rollback of the tested failure. Other failure modes remain untested. |
| E11 | Can the helper enforce management-path exclusions through nftables alone? | In `inet domovoi_probe`, add a `10.99.0.0/24` element representing a management path; add an overlapping `10.99.0.42` element that a compromised caller might request; test overlap; test whether a higher-priority `drop` protects the management CIDR even when the element exists | Whether nftables refuses the overlap (expected: no); whether ordering can protect it; what the helper must check itself | That nftables does not encode the exclusion policy; helper-side recomputation is required. Not the helper's implementation. |
| E12 | Are apply and rollback idempotent and concurrency-safe? | In `inet domovoi_probe`, add the same element twice; delete twice; run two batches simultaneously adding the same element; run a delete-and-add while another batch adds | Error codes and resulting membership | Idempotency of the operations tested. Not the journal-based reconcile logic. |
| E13 | What changes after a reboot or a WSL2 restart? | Do **not** reboot. Record the question as untested, and state what the process-death test did and did not cover | — | Nothing beyond E8. Reboot stays for conventional Linux. |

### 7.1 E3 topology (three namespaces, two veth pairs)

Reference sequence; verify syntax and interface availability before running it. Everything below stays
inside the three disposable namespaces. The host namespace and its firewall are never touched.

```bash
# Namespaces and links (root).
ip netns add dv-sender
ip netns add dv-router
ip netns add dv-receiver
ip link add vs type veth peer name vr1
ip link set vs netns dv-sender
ip link set vr1 netns dv-router
ip link add vr2 type veth peer name vrecv
ip link set vr2 netns dv-router
ip link set vrecv netns dv-receiver

# Addresses inside 10.99.0.0/24 only.
ip -n dv-sender   addr add 10.99.0.1/30 dev vs
ip -n dv-router   addr add 10.99.0.2/30 dev vr1
ip -n dv-router   addr add 10.99.0.5/30 dev vr2
ip -n dv-receiver addr add 10.99.0.6/30 dev vrecv

# Links and loopback; forwarding only in the router.
for ns in dv-sender dv-router dv-receiver; do
  ip -n "$ns" link set lo up
done
ip -n dv-sender   link set vs up
ip -n dv-router   link set vr1 up
ip -n dv-router   link set vr2 up
ip -n dv-receiver link set vrecv up
ip netns exec dv-router sysctl -w net.ipv4.ip_forward=1

# Routes: each endpoint reaches the other subnet through the router.
ip -n dv-sender   route add default via 10.99.0.2
ip -n dv-receiver route add default via 10.99.0.5
```

Each namespace has an independent ruleset. Create the stand-in table and one counting base chain per
namespace:

```bash
ip netns exec dv-sender nft 'add table inet domovoi_probe'
ip netns exec dv-sender nft 'add chain inet domovoi_probe out { type filter hook output priority 0; policy accept; }'
ip netns exec dv-sender nft 'add rule inet domovoi_probe out counter'

ip netns exec dv-router nft 'add table inet domovoi_probe'
ip netns exec dv-router nft 'add chain inet domovoi_probe fwd { type filter hook forward priority 0; policy accept; }'
ip netns exec dv-router nft 'add rule inet domovoi_probe fwd counter'

ip netns exec dv-receiver nft 'add table inet domovoi_probe'
ip netns exec dv-receiver nft 'add chain inet domovoi_probe in { type filter hook input priority 0; policy accept; }'
ip netns exec dv-receiver nft 'add rule inet domovoi_probe in counter'
```

Send three ICMP requests from the sender to the receiver, then read all three counters. Expected shape,
to be confirmed: the sender's `output` counter sees the three requests (and the receiver's `input` sees
three); the router's `forward` chain sees both directions, so it should count six. Record the actual
numbers, including any asymmetry or unexpected drop.

Repeat with the drop rule that E5 needs: on the receiver, add a set `blocked4` and a rule
`ip saddr @blocked4 drop` in the `in` chain, insert `10.99.0.1`, and send three more requests. Expected:
the sender's `output` and the router's `forward` counters still increment for the requests, the receiver's
`input` counter increments and drops them, and no reply returns. Record the exact counters. A second
variant that drops in the router's `fwd` chain may be run as well; label which chain dropped the packet.

Cleanup deletes the namespaces and with them both veth pairs:

```bash
ip netns del dv-sender
ip netns del dv-router
ip netns del dv-receiver
```

If `ip netns` cannot be used in this environment, stop and report; do not substitute host interfaces or a
different topology, because the chain responsibility being measured would change.

## 8. Failure and recovery scenarios

For each, record the command, the failure signal, the resulting state, and what a helper would have to
report:

1. Batch with an element that does not exist (`delete` of an absent element).
2. Batch that overflows the set's `size` (E10).
3. Batch with a malformed timeout or an unknown set.
4. Element expires between the hub's confirmation and the replacement batch (E9).
5. The applying process is killed after the first element is added and before confirmation; the element
   must expire within 30 seconds without any process.
6. The namespace's creator leaves; does the element survive in a persistent namespace, and does it
   expire?
7. Duplicate apply of an already-present element.
8. Rollback when the element is already absent.

Expected shape, to be confirmed rather than assumed: a failed batch commits nothing; a killed process
leaves the 30-second element to expire; an expired element makes the replacement batch fail and must not
be re-added.

## 9. Ordered instructions for the executor

1. **Verify the checkout and environment.** Run `git status --short --branch`; confirm you are on
   `spike-nft-containment`, cut from the latest `overhaul-investigation`. Record `uname -r`,
   `nft --version`, `systemctl --version`, `systemctl is-system-running`.
2. **Read before testing.** `03 RW-13`, `03 RW-37` (exclusions and safety identity only), `04 §9.1`,
   `04 §7.3` hard limits, `99` R-2, and `H-wsl2-research-environment.md` §3.
3. **Establish the baseline (E1).** Confirm nftables works and the default namespace will not be touched.
4. **Start an isolated namespace.** `sudo unshare --net bash`, or create `ip netns` namespaces for the
   tests that need multiple endpoints (E3 uses three, as specified in §7.1). Record which you used.
5. **Run E2–E6 first.** Table isolation, hooks, traversal, ordering, packet drop, limits.
6. **Run E7–E12.** TTL grammar, expiry independence, replacement, rollback, exclusions, idempotency.
7. **Record evidence as you go.** For every command: purpose, exact command, raw output excerpt, exit
   status. Do not reconstruct outputs from memory.
8. **Separate observations from conclusions.** Write "observed" rows first, then a short interpretation
   section. Never let a conclusion imply a test you did not run.
9. **Write the report** at the path in §10 with the required structure and labels.
10. **List the remaining unknowns** and the conventional Linux validation matrix. Do not update `99`,
    any ADR, `06`, `07` or the architecture documents; no owner decision is yours to make.

## 10. Report requirements

File: `docs/rework/investigations/I-r2-nftables-containment.md` (next free letter if taken).

Required sections:

1. **Environment and date** — exact versions, WSL2 networking mode, who ran each experiment.
2. **Safety statement** — what namespaces were used and confirmation that the host ruleset was untouched.
3. **Findings by experiment** — one subsection per `E` row: question, command, observed output, label
   (**demonstrated on WSL2** / **not demonstrated** / **requiring conventional Linux validation**), and
   what it does not establish. Name every table exactly; every experimental table name starts with
   `domovoi_probe`.
4. **Table, hook and priority recommendation** — a proposal with the evidence behind it and the evidence
   still missing. State explicitly that the measurements describe the stand-in table `inet domovoi_probe`,
   not a production table, and that no experiment created or modified a table named `inet domovoi`. This
   is a recommendation to the owner/ADR, not an accepted design.
5. **Exclusion enforcement** — what nftables can and cannot guarantee; what the helper must recompute.
6. **Failure and recovery** — the §8 scenarios, each with its observed result.
7. **Limits and resources** — measured numbers and the conditions they were measured under.
8. **Threats to validity** — NAT, Hyper-V Firewall, Microsoft kernel, empty base ruleset, no Docker, no
   firewalld/ufw, single nftables version.
9. **Conventional Linux validation matrix** — distro, kernel, nftables version, firewall manager, Docker
   presence, and the specific experiment to repeat on each. Include at least: a host with an existing
   base ruleset; firewalld active; ufw active; Docker with published ports; a bridged physical NIC; a
   VPN/tunnel; a reboot.
10. **Remaining unknowns and non-acceptance statement** — explicitly: R-2 stays open; containment is not
    production-ready; no supported-platform claim is made.

## 11. Completion criteria

The prompt is complete when:

- every `E1`–`E13` row is attempted or explicitly marked not demonstrated with a reason;
- the report exists at the required path with every finding labelled;
- every table named in the report starts with `domovoi_probe`, and the report distinguishes the
  stand-in `inet domovoi_probe` from the proposed production table `inet domovoi`;
- at least the success, rollback, expiry-independence and expired-element failure cases are reproduced
  and shown with commands and outputs;
- the exclusion question is answered as "nftables alone cannot enforce it" or the opposite, with
  evidence;
- the conventional Linux matrix is concrete enough for the owner to commission the follow-up tests;
- no production file, migration, protocol type or owner decision was changed.

## 12. Open questions this prompt must not close

- Which distributions, kernels and nftables versions are supported. R-2 names "supported Linux
  platforms"; the supported set is not defined yet.
- Whether the intended hook/priority placement coexists with real firewalld, ufw and Docker rulesets.
- Packet behaviour under live traffic during a successful replacement batch.
- Reboot recovery on a host where the ruleset is persisted or restored.
- Whether a management path can be protected by ordering alone, or only by helper-side refusal.

## Execution constraints

You are implementing **one** change in the Domovoi repository (a Rust workspace with an embedded React UI).
This document is your complete brief. The planning documents it cites are background: what you need is
written here.

1. **Inspect before relying on anything.** Before you edit, open every file, type and function this
   brief names.
   - Confirm that each one exists and behaves as described.
   - If something differs materially, write down the difference **before** changing code. Examples: a
     different signature, a missing module, behaviour already implemented, or a prerequisite prompt not
     merged.
   - If the difference changes what you would build, stop and report it instead of guessing.
2. **Follow the contracts here exactly.** Do not change public interfaces, wire formats, database
   schemas or security behaviour beyond what this brief specifies.
3. **Stay in scope.** No unrelated refactoring, renaming or reformatting. No new dependencies unless
   this brief names them. Add any new dependency to `[workspace.dependencies]` in the root
   `Cargo.toml` and reference it from crates with `{ workspace = true }`.
4. **Never weaken a security check, a gate test or an arch rule to make something pass.**
5. **Stop and report rather than invent.** Do this when the brief contradicts the code, an
   architectural decision is missing, or a prerequisite does not exist.
6. **Be truthful.**
   - Never claim a test passed unless you ran it and saw it pass.
   - Never present stubbed or partial work as complete.
   - Mark anything unfinished as unfinished.

## Repository rules

These come from `AGENTS.md`, which CI enforces.

- **Crate boundaries.** `domovoi-core`, `domovoi-proto` and `domovoi-engine` do no I/O. Only
  `domovoi-runner` spawns processes, always with argv vectors and never `sh -c`. Dependency edges follow
  `ARCHITECTURE.md` §4. `cargo xtask arch-check` enforces all of this.
- **Approvals.** `HumanApproval::new` is called only in `crates/domovoi-hub/src/auth/`, and
  `HumanApproval::restore` only in `crates/domovoi-store/src/`. The LLM tool catalogue stays closed.
  Nothing but an authenticated human approves an action.
- **Code rules.**
  - No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `dbg!` in library code.
  - No new `#[allow(...)]`.
  - `thiserror` in libraries; `anyhow` only in `domovoi` and `xtask`.
  - Every public item has a doc comment saying what it is for.
  - Non-test source files stay at or under 500 lines; web source files at or under 250 lines.
- **Data and time.** Never edit a merged migration; add a new one. Choose a migration's number by listing
  `crates/domovoi-store/migrations/` on your branch and taking the next unused number; never assume it
  from a plan or a predecessor. Renumber only your own unmerged file if the branch gained a migration
  first, and describe the migration by its contents, not its number. Time comes from the `Clock` port;
  tests advance the fake clock from `domovoi-testkit` and never sleep. Secrets are
  `secrecy::SecretString` and are never logged, put into prompts or returned from the API.
- **Web.** Read `web/DESIGN.md` before touching `web/`. Build only from the kit in
  `web/src/components/`. Use `useActionFeedback` for every mutation and `describeError` for every error
  message, and tokens only for styling.
- **Tests.** Name tests after the behaviour, not the function. Never delete or weaken `tests/gates.rs`.
  Review every `insta` snapshot you accept.
- **Branch and commits.**
  - Work on a branch named as the brief says.
  - Commit messages are one line in Conventional Commits form, with exactly one scope, for example
    `feat(store): ...`. No body and no trailers.
  - Committed files, commit messages and the PR description must not mention prompts, prompt numbers,
    the build plan, coding agents or AI assistance.
- **Checks to run.** Run them for what you changed only. CI runs the full suite.
  - `cargo fmt --all`
  - `cargo clippy -p <crate> --all-targets -- -D warnings` for each crate you edited
  - `cargo nextest run -p <crate>` for each crate you edited, or `cargo test -p <crate>` if nextest
    is not installed
  - `cargo nextest run -E 'rdeps(<crate>)'` when you changed a public item that other crates use
  - `cargo xtask arch-check` when you added or moved modules or touched a `Cargo.toml`
  - for `web/`: `npm run lint`, `npm run typecheck`, and the vitest files for the changed area
  - On Windows, the hub and agent run only under WSL 2; unit and integration tests run natively.

## Completion report

End with a report in exactly this structure:

1. **Implemented behaviour**: what now works, in observable terms.
2. **Files**: added, changed and removed, each with one line on why.
3. **Implementation decisions**: any choice the brief left to repository convention, and what you chose.
4. **Interfaces**: types, functions, endpoints and messages introduced or changed, with signatures.
5. **Database and protocol changes**: migrations (file names), schema changes, wire changes.
6. **Security**: how each security requirement in the brief is met, and where it is tested.
7. **Tests added**: the test names, and which acceptance criterion each covers.
8. **Commands run**: each exact command and its actual result (pass, fail, counts).
9. **Not run**: tests or checks you did not run, and why.
10. **Deviations**: every place you departed from the brief, and why.
11. **Known limitations and remaining work**.

For this research prompt, read that report as follows: "implemented behaviour" means the evidence and
artefacts produced (probes, observations, report); "tests added" means the probe scenarios and any checks
you added; "database and protocol changes" must be "none"; "security" means how isolation and exclusions
were respected during the experiments. Do not invent product behaviour to satisfy the wording.
