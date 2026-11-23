# Research synthesis: overview, conclusions and implementation plan

> Note on sources: the raw research reports and probe outputs this synthesis cites by letter (J to W, `probes/...`) are kept offline and are not part of this repository. Host nicknames in this directory are anonymised (`host-c`, `host-t`). The evidence each claim rests on is stated here and in ADRs 0030 to 0032.


Numbers such as 44, 45, 46 and 47 are work items of the implementation index (`07-implementation-prompt-index.md`); this planning document lives under `docs/rework/` and is not part of the published tree.

Status: synthesis of the helper, skill-runner, unit-identity, attestation and containment research. It records what was measured, what follows from it, what to build and what to decide. It accepts nothing: R-2, R-3, R-8, R-9 and R-10 stay open until the owner accepts them. Facts and recommendations are kept apart; every recommendation is marked.

Detail lives in four documents in this directory, each with consolidated findings, analysis, requirements, ideas, a named test plan, follow-up edits and owner decisions:

| Document | Research item | Reports behind it |
|---|---|---|
| [01-r8-unit-identity-synthesis.md](01-r8-unit-identity-synthesis.md) | R-8 effective unit identity for `unit.restart` | K, O, T |
| [02-r3-r9-helper-and-skill-runner-synthesis.md](02-r3-r9-helper-and-skill-runner-synthesis.md) | R-3 helper delivery, R-9 skill-runner isolation, D-24 Docker | J, M, N, S |
| [03-r10-safety-attestation-synthesis.md](03-r10-safety-attestation-synthesis.md) | R-10 safety attestation | L, N addendum, M, N, S |
| [04-r2-nftables-containment-synthesis.md](04-r2-nftables-containment-synthesis.md) | R-2 nftables containment | I, P, Q, R |

## 1. What was measured, and where

| Environment | OS and systemd | Kernel, arch | Used for |
|---|---|---|---|
| WSL 2 (research host) | Ubuntu 26.04.1, systemd 259.5 | 6.18 Microsoft, x86_64 | all five items; first pass at user scope, then system scope with real disposable users |
| Raspberry Pi 5 | Debian 13, systemd 257.13, nft 1.1.3, Docker 29.10 | 6.18.60 rpi, aarch64, real NICs (eth0, wlan0) | all five items; two nft rounds; real Docker, ufw, firewalld, flowtables, WireGuard |
| Ubuntu 26.04 server | Ubuntu 26.04.1, systemd 259.9, nft 1.1.6, AppArmor, Yama | 7.0.0-43, x86_64 | R-8, R-3/R-9/R-10 with real users; R-2 in namespaces; also a packet sender for the real-NIC tests |
| Ubuntu 24.04 LTS server | Ubuntu 24.04.5, systemd 255.4, nft 1.0.9, AppArmor with the userns restriction, Yama 1, Landlock, `/run` mounted `noexec` | 6.8.0-149, x86_64 | R-8, R-3/R-9 with real users (no toolchain, so no R-10 harness build), R-2 in namespaces |

Counting honestly: four independent hosts, not more than a dozen runs. The Pi's two nft rounds and the WSL's three passes share a host each. Both Ubuntu servers and the Pi run live services, so every probe there used disposable names and a baseline comparison. Nothing was run on RHEL/SUSE/Arch, Debian 12 or Ubuntu 22.04 (systemd 252/249), systemd 258, SELinux, a container host or a non-systemd host. Nothing tested a reboot or a power cut. The helper and skill-runner binaries do not exist yet; every isolation result used a stand-in process.

## 2. Conclusions that hold across the research

Supported by measurement on at least two independent hosts unless stated.

**Isolation (R-3, R-9, R-10 custody).**
- Two real non-root accounts cannot read, write, rename, link, `ptrace` or read `/proc` of a root-owned 0700/0600 area. A compromised agent or skill cannot read the attestation key or forge a signature (3 hosts at OS level; the harness behind real users ran on WSL only).
- Socket mode and group, `SO_PEERCRED` and the mount mask are separate layers; the mask beats a granted group; a `0666` socket is not protected by any directive.
- Membership of the `docker` group, or a mounted Docker socket, is root. A read-only bind of the helper directory was enough to read the key (Pi, Ubuntu).

**Where the design as written does not work.**
1. The RW-39 skill-runner unit fails at start with `226/NAMESPACE` (parent `TemporaryFileSystem=` plus a child `InaccessiblePaths=`) on every run, and `systemd-analyze verify` stays silent. The corrected form (parent masks, binds back in) works.
2. `SupplementaryGroups=` empty does not drop an account's own groups. The control is the account itself (no groups), plus a start-time check.
3. systemd does not cancel an `Accept=yes` instance when the client disconnects. The runner must watch for it, give the skill its own process group and exit 0; `RuntimeMaxSec` on `Type=simple` is the backstop (`Type=oneshot` ignores it).
4. With `/etc/domovoi` at `root:domovoi-agent 0750` the skill account cannot reach `skills_dir` without the mask and bind, so the mask is required for function, not only for secrecy.
5. `restart` runs the configuration systemd has loaded, not what is on disk; `NeedDaemonReload` misses edits with a restored mtime, generator input changes and `set-property --runtime`. RW-34's disk-only hash does not say what a restart will run.
6. RW-34's three fragment directories are not the whole search path: `/usr/local/lib/systemd/system` outranks `/usr/lib`, drop-ins also come from `/run/systemd/generator*`, `/run/systemd/system.control` and dash-prefix directories, a masked unit's fragment is the link not `/dev/null`, and a transient unit does have a fragment path. `sshd.service` resolves to `ssh.service`, so the default protected-unit list must match by `Id` and `Names`.
7. nftables rollback "`del element`, idempotent" is false: `delete` of an absent element exits 1 and aborts the batch; `destroy` does not exist before nft 1.0.9 and is not idempotent on interval sets in 1.0.9, 1.1.3 and 1.1.6 (kernels 6.18, 7.0). The idempotent form is one batch of `add element X timeout T` plus `delete element X`.
8. An `input`-hook drop misses Docker published ports (both Docker backends); some IPv6 modes also miss `forward`. A `prerouting` chain below every manager's priority stopped every flow tested.
9. Kernel timeouts expire independently of processes, but not of the table: `systemctl start nftables` with the stock `flush ruleset` configuration deletes our table and no timeout then fires.
10. Established connections are not cut by a new drop when `ct established accept` precedes it, and flowtable-offloaded flows bypass any later drop; `conntrack -D` ends them and nft cannot.
11. On kernel 6.8 with nft 1.0.9 (Ubuntu 24.04's stock pair) the `delete` plus `add` replace is not packet-atomic on interval sets: it leaked packets under about 450 kpps (about 0.05 per replacement in a tight loop; 0.2 to 0.3 per replacement with a few milliseconds between replaces), including in the exact recommended layout. Plain hash sets did not leak, and interval sets did not leak on kernels 6.18 and 7.0 with nft 1.1.x. Kernel versus nft cause is not separated. `add element` on an existing element is a no-op on 6.8 (it refreshes on the others).
12. Re-running the nft bootstrap over a live table duplicates rules and silently rewrites a set's size and timeout; the bootstrap must be create-if-absent. A `ct state established accept` at `prerouting` -310 exempts nothing because conntrack runs at -200.
13. New drop-in directories are missed by `NeedDaemonReload` and by `systemctl cat` on systemd 255 (and both read stale elsewhere in other cases); only a disk-side enumeration catches every case. Set-valued D-Bus properties come back in no fixed order, so a D-Bus based hash must sort them; plan time must use `LoadUnit` (`GetUnit` fails for an inactive unloaded unit).
14. The `selftest` wordings written for the runner would misclassify a good host as Blocked on systemd 255 in three places (ST-07, two points of ST-13); noexec `/run` means nothing may be executed from `/run`.

**Attestation (R-10).**
- The `04b` encoding is implementable and portable: all vectors and 95 of 95 harness tests pass on three hosts and two architectures. This says nothing about live netlink, policy routing or VPN behaviour (fixtures only).
- Several spec contradictions need decisions (section 4). The generation-repeat gap under a rolled-back state file is a detection gap, not an unsafe containment, because permits bind the generation and both hashes.
- Code reading found three defects to fix: the hub's key-mismatch flag is sticky (one forged key blocks containment until key rotation), the helper accepts a missing `expected_safety` for `Contain`, and freshness is measured from hub receipt, so observed state can be up to 360 s old.

**Race limits (R-8).** The gap between reading a unit's identity and restarting it cannot be closed from the helper. No guard reached zero in any row (3 hosts, up to 3600 trials). The one post-check that worked (reading `/proc/<MainPID>/environ`) detects a wrong start but cannot prevent it and was tested for one property.

## 3. Cross-cutting themes

1. **Detect, do not assume.** Every measured weakness has the same shape: the thing the design trusts (config on disk, a verify line, a flag, a table) can change underneath it. The answers that worked were checks at the last moment plus a way to notice afterwards, not tighter assumptions: `Id`-based unit matching, `getgroups()` at start, table-loss detection, membership reads after a failed apply, a hub high-water mark.
2. **Tools lie in quiet ways.** `systemd-analyze verify` passes broken units; `systemctl show` text varies by version; `nft` error text is the same for expired and rolled back; `NeedDaemonReload` misses real edits. Read structured interfaces (D-Bus `busctl`, `nft -j`) and classify with a second source.
3. **Fail closed must be testable.** A plain mask on an absent path fails closed, a `-` prefix fails open, `TemporaryFileSystem=` on an absent path succeeds and creates a host directory. The installer, `selftest` and the helper start checks have to assert modes, groups and paths, because the platform will not.
4. **Sizes must be capped on purpose.** Replace cost grows with set size (about 20 to 40 ms at 10 000 elements, about 610 ms at 100 000 interval elements) and a flood lowers throughput 11 to 25 percent at 100 000 elements. A small default `max_elements` is a safety property, not a tuning knob.
5. **Evidence is thin where it matters most.** Single host per architecture, no reboot, no real helper, no real installer. The next confidence step is a real-installer pass with the production binaries, not more stand-in probes.

## 4. Contradictions to resolve before the ADRs are final

| Contradiction | Sources | Resolution proposed |
|---|---|---|
| `04b` "one or more fields" versus RW-37 empty `incomplete_reason` | G-1 | exception: empty exactly when `complete=true` (03 D1) |
| `04b` invalid config yields no hash versus RW-37 64-hex hashes | G-2 | fixed minimal shape for incomplete reports (03 D2) |
| ADR 0032 C4 says escaping the selector changes vector R4; the synthesis argues inner escaping leaves R4 unchanged | 03 | run the vector before deciding |
| ADR 0032 recommends hashes at `init` (generation 1); synthesis recommends generation 0 | 03 D3 | owner decision |
| ADR 0032 describes the harness as WSL-only with a 515 µs cost | 03 | superseded: 95 of 95 on three hosts; cost is 0.8 ms compute, 1.2 ms (NVMe) to 67 ms (USB HDD) per state write |
| `04` §8.3 says the key is masked in the agent unit; a mask there removes the agent's own sockets | 02 | the agent is protected by file permissions, not a mask; edit the text |
| RW-39 unit text versus measurement | 02 | replace with the corrected form (02 section 5) |
| `domovoi-skills` spelling: four sites in ADR 0030, five in the code (a snapshot is the fifth) | 02 | keep `domovoi-skill` (D-31); edit five sites |
| round 1 said published ports stop after an `nftables.service` flush; round 2 shows they keep working through `docker-proxy` | 04 | round 2 supersedes |
| round 1 recommended `destroy` as rollback | 04 | corrected: absent before 1.0.9, not idempotent on interval sets |
| the S10b bind form: "259.5 fails, 257 does not" is a user-scope versus system-scope confound | 02 | not established as a version difference |
| the `size` wrap at 2^32 differs by method between rounds | 04 | unreconciled; do not rely on `size >= 2^32` |
| `expected_safety` "required on every Contain" versus "when present"; test name says capped, harness refuses | 03 | fix RW-37 wording; refuse and say so |

### Conflicts between the four syntheses

The four documents were written separately from different reports. They were compared on the points where they specify the same component (the root helper) and on shared names. Findings:

| Point | What the documents say | Result |
|---|---|---|
| Helper unit directives: 02 section 5 gives one shared unit with `CapabilityBoundingSet=CAP_NET_ADMIN`, `ProtectSystem=strict`, `ReadWritePaths=/var/lib/domovoi-helper`, `PrivateDevices=yes`, `Accept=yes`, `RuntimeMaxSec=60s`; 01 needs D-Bus restart and property reads; 04 needs `nft` and `conntrack` | no contradiction found | verified on the Pi (systemd 257): a root process under exactly that directive set restarted a disposable unit through `busctl` (new main PID) and read its `ActiveState`, so the capability bounding set does not block the D-Bus path. Not verified: `RestrictAddressFamilies=AF_UNIX AF_NETLINK` (02 lists it as an unmeasured candidate; it must keep `AF_UNIX` for the bus and `AF_NETLINK` for nft and conntrack), and `conntrack` itself (not installed on the test hosts) |
| The one shared helper unit gets the union of privileges needed by all groups (02: dropping `CAP_NET_ADMIN` for read-only groups is unmeasured) | tension, not a contradiction | the helper's write groups and the key custody share a root process; separating them (one unit per group, or an in-process capability drop) is an open design choice in 02 |
| Write-ahead journal of 04 | 04 does not name a path | must live under the helper state directory (the only writable path in 02's unit); 04 and 02 must agree before implementation |
| `After=nftables.service` (04 D9) | not in 02's helper unit | add when 04 D9 is decided |
| Refusal and error tokens across 01, 03, 04 | no token is defined with two meanings (`target_changed` and `target_stale` in 01; `containment_lost`, `table_missing`, `table_drift` in 04; `state_corrupt`, `key_corrupt`, `attestation_missing`, `report_incomplete` in 03) | only the sets of one component are specified together; the final token list needs one owner file, and `target_changed` (01) versus `target_stale` (01) needs a single meaning |
| `Accept=yes` helper with RuntimeMaxSec 60 s versus 04's two-step apply with a 30 s confirmation | compatible | each request is its own instance; the confirmation is a separate request |
| Priority value, hook and set layout (04) versus attestation exclusions (03) | consistent | exclusions are computed by the helper for both families in both documents |

## 5. Decisions the owner must take

The four documents carry the full tables with options, costs and a marked recommendation (R-8: Q1 to Q10; R-3/R-9: D-1 to D-11; R-10: D1 to D15; R-2: D1 to D15). Ordered by what each blocks:

**Before the helper, skill runner and attestation can be built (47):**
- Platforms and versions: decided as tiers (section 5b). Still open: the exact wording of the tier gate in the product, and the per-host opt-in for helper write groups on Untested platforms.
- `Accept=yes` versus `no`: decided, `yes` (section 5a).
- Mode of `/etc/domovoi`: decided, keep 0750 with a mandatory mask (section 5a).
- Wrong directory, key or state mode: decided, refuse, report and offer an explicit owner-confirmed repair (section 5a).
- Inherited-socket adoption (02 D-6, ADR 0030 A4): decided, std only from fd 0, no new dependency (section 5a).
- No user namespaces for the runner at launch, no AppArmor or SELinux profile shipped (02 D-5, D-9).
- R-10 decisions still open: D5 to D7 (revocation, rollback detection, apply-time limits), D9 to D15 (grammar amendments, timing, tokens, UTF-8 step, missing `expected_safety`, retired keys, non-lookup rules). Decided: D1, D2, D3, D4, D8 (section 5a).
- The five-site spelling edit (02 D-7).

**Before `unit.restart` (45):**
- Supported systemd range and non-merged `/usr` handling (01 Q1); valid targets (01 Q2, recommendation: plain `/etc` and `/usr/lib` units only for v1); helper never runs `daemon-reload` (01 Q3); residual race accepted with the post-check (01 Q4); `EnvironmentFile=` and binaries unbound and stated on the card (01 Q5); read identity inside `systemd.read` (01 Q6); wrong start is alert only (01 Q7).

**Before containment (46):**
- Hook placement and priority (04 D1, D12: recommendation `prerouting` only at -310); set layout (04 D2); support statement (04 D3: declare nothing until the matrix passes); existing flows (04 D4); table loss (04 D5); CLI versus netlink in v1 (04 D6); `max_elements` (04 D7: 1 000 default, 10 000 ceiling); bridge-family coverage (04 D8); unit ordering (04 D9); reboot semantics (04 D10, after the reboot test).
- New from the fourth host (04 D14, D15): require a leak self-test at helper start that blocks containment on a platform where the replace leaks (recommended: required), and what to do where the interval-set replace leaks (recommended: disable the interval path; host targets use plain hash sets). 04 D2 (set layout) is revised accordingly.
- R-10 D5 and D6 (revocation, rollback detection) because containment depends on them.

## 5a. Decisions taken by the owner

Recorded as given; the synthesis recommendations they refer to are in the documents named.

| Decision | Owner's answer | Synthesis reference | Consequence for the design |
|---|---|---|---|
| `Accept=` for the skill runner (helper follows) | `Accept=yes` | 02 D-2 | runner-side EOF cancel, own process group, exit 0, `RuntimeMaxSec` on `Type=simple` |
| Mode of `/etc/domovoi`, runner access to `skills_dir` | keep `root:domovoi-agent 0750`; the mask and bind stay mandatory | 02 D-1 | `selftest` must detect a missing mask as an outage |
| Helper start with a wrong directory, key or state mode | refuse and report, and offer self-healing | 02 D-11 | start fails closed with a mode token; repair is a separate, explicit, owner-confirmed action (never automatic at start); the report names each path, found mode, expected mode and owner |
| Platforms and systemd versions | not a hard floor: allow everything, publish a list of supported, untested and unaccounted-for platforms | 02 D-3, D-4 | see 5b; a hard version floor is replaced by tiers gated by a runtime isolation check |
| Inherited socket adoption without `unsafe` | evaluate a safe wrapper crate first | 02 D-6, ADR 0030 A4 | evaluation done (`05-socket-adoption-crate-evaluation.md`): it recommends no crate at all, taking the connection from fd 0 with std only (both unit shapes are `Accept=yes` with `StandardInput=socket`), with `listenfd` as the fallback for fd 3 or `Accept=no`; confirmed by the owner: std only from fd 0, no new dependency, no ADR exception to the unsafe rule; `listenfd` stays the documented fallback for fd 3 or `Accept=no` |
| Inherited-socket adoption | confirmed: std only from fd 0, no new dependency | 02 D-6, `05-*.md` | `StandardInput=socket` connection adopted with `try_clone_to_owned`; refuse a non-zero `LISTEN_FDS`; a test must pin that children get a cleared environment and non-inherited stdin; `listenfd` is the fallback if fd 3 or `Accept=no` is ever needed |
| Empty `incomplete_reason` (G-1) | exception: empty only when `complete=true` | 03 D1 | `04b` gets a one-line exception; the verifier rejects an empty reason on an incomplete report and a non-empty reason on a complete one |
| Shape of an incomplete report (G-2) | one fixed minimal shape | 03 D2 | same lines every time (`complete=false`, a reason, zeroed hashes and limits); one parser path |
| Generation at `init` (G-6) | generation 0 with zero hashes | 03 D3 | init needs no valid config or observer; the first real attestation is generation 1; ADR 0032's "compute hashes at init" is replaced; RW-37 wording changes |
| Wire-type fixes | adopt the recommended list | 03 D8, section 5.9 | required `expected_safety` for `Contain`, `deny_unknown_fields`, an enum for reasons, a fixed hash encoding; a protocol version bump depends on whether 1.2 has shipped |
| Revocation of verified state (R-10 D5, X-1) | revoke on key mismatch, signed incomplete report or generation regression; gate on the latest outcome | 03 D5 | the hub's sticky mismatch flag is replaced by a latest-outcome rule; one good attestation restores trust, any bad one revokes |
| Rollback and generation repeat (R-10 D6, G-15) | hub high-water mark, persisted in the acceptance transaction | 03 D6 | the hub stores the highest accepted generation and hashes per helper and rejects a lower generation or an equal generation with different hashes; needs a small hub table and migration |
| Apply-time limits (R-10 D7, G-16) | refuse a TTL above the maximum; `max_active=0` means none allowed; persist the limiter | 03 D7 | an over-long TTL is refused with a token, not clamped; the default configuration refuses all containment; the mutation limit survives a helper restart |
| Missing `expected_safety` on `Contain` (R-10 D13, X-2) | refuse | 03 D13 | the helper refuses; RW-37's "required on every Contain" wins over "when present" |
| Grammars the spec leaves open (R-10 D9, G-11) | adopt the harness readings with the synthesis amendments | 03 D9, section 5.2 | the amended grammars become `04b` text (interface names, ports, table/priority/metric ranges, `max_ttl`, unit and container names); the backslash and colon beliefs were checked on a host |
| Freshness and nonce timing (R-10 D10, G-12) | strict boundaries; freshness from nonce issue time; no key-id cross-check; consume the nonce late | 03 D10 | observed state is bounded by the nonce lifetime, not up to 360 s |
| Refusal tokens (R-10 D11, G-13) | keep the nine wire tokens, detail stays internal, add the two missing core variants | 03 D11 | no wire enum growth; `path_uncovered` and `confirmation_stale` are added to core |
| Retired private keys (R-10 D14) | delete | 03 D14 | rotation and recovery delete the old private key |
| Valid `unit.restart` targets (R-8 Q2) | plain units from `/etc` and `/usr/lib` only | 01 Q2, REQ-4, REQ-5 | refuse aliases, linked, generated, transient, masked, `enabled-runtime`, `/usr/local/lib` and `set-property`-only units; revisit with real owner units |
| `daemon-reload` by the helper (R-8 Q3) | never; refuse a stale unit | 01 Q3 | `NeedDaemonReload=yes` or disk different from loaded config refuses the step; the owner reloads |
| Residual race (R-8 Q4) | accept, document on the card, rely on the post-restart check | 01 Q4 | the card states the residual; no guard reached zero |
| Wrong start (R-8 Q7) | alert only, no automatic action | 01 Q7 | the step is marked failed and an alert raised; no corrective restart or stop |
| Unbound inputs (R-8 Q5) | `EnvironmentFile=` contents and executables stay unbound, stated on the approval card | 01 Q5 | no false invalidations from routine updates; the card names what is not bound |
| Where the plan-time identity read lives (R-8 Q6) | a new `systemd.read` group, enabled by default | 01 Q6 | pre-flight works on a fresh install; widens the fresh-install default slightly |
| Restart of a stopped unit (R-8 Q8) | `restart` (starts it) with an exact instance allowlist | 01 Q8 | a never-started template instance is refused by the allowlist |
| Reading unit files under the helper's capability set (R-8 Q10) | D-Bus plus the files it can read; refuse the rest with `read_failed` | 01 Q10 | no `CAP_DAC_READ_SEARCH`; units with foreign-owned unreadable drop-ins are refused; reconsider only if real owner units need it |
| Containment hook (R-2 D1, D12) | `prerouting` only, one chain at priority -310 | 04 D1, D12 | one chain below every manager's priority; no `ct established accept` ahead of conntrack; the priority is a constant covered by a gate test |
| Set layout (R-2 D2) | plain hash sets for single hosts; interval sets for prefixes only where the leak self-test passed | 04 D2 | prefix blocking exists only on platforms that pass the self-test |
| Leak self-test (R-2 D14) | required at install and on a change of kernel, nft or layout; a failure blocks that path | 04 D14 | the helper's privilege statement records the added right to create a namespace and links; about 30 s of CPU |
| Platform where the interval replace leaks (R-2 D15) | disable the interval path: single hosts only, prefix targets refused with a clear message | 04 D15 | Ubuntu 24.04's stock pair gets hosts-only containment until a mitigation is proven |
| Containment support statement (R-2 D3) | declare nothing until the validation matrix passes; measured pairs only as "validated on" notes | 04 D3 | no support claim in documentation or UI; Ubuntu 24.04's stock pair is listed Untested for containment writes |
| Established and offloaded flows (R-2 D4) | delete the target's conntrack entries only when flowtables exist; always report coverage | 04 D4 | needs `conntrack -D` (the tool or ctnetlink) on flowtable hosts; coverage reported on every confirm |
| Table loss (R-2 D5) | re-create the empty table, report `containment_lost`, the hub re-requests | 04 D5 | the helper checks for the table before every request; re-applying goes through a normal request with a fresh safety check |
| `nft` CLI or netlink (R-2 D6) | `nft` CLI with a small `max_elements` | 04 D6 | fixed script templates; a netlink binding stays a later idea |
| `max_elements` (R-2 D7) | default 1 000, hard ceiling 10 000 | 04 D7 | replace cost and memory stay bounded (about 20 to 40 ms at 10 000 elements) |
| Bridge-family coverage (R-2 D8) | out of v1: detect a bridged physical NIC and report a coverage gap | 04 D8 | no bridge-family table; the confirm reports the gap |
| Helper unit ordering and reload propagation (R-2 D9) | `After=nftables.service` plus runtime table-loss detection; test `ReloadPropagatedFrom` | 04 D9 | a start before the firewall service is not flushed; later reloads are caught by D5 |
| Orphan elements (R-2 D13) | report only; they still expire by kernel timeout | 04 D13 | no automatic removal path |
| UTF-8 validity step (R-10 D12, G-19) | keep | 03 D12 | the contract's own rule list stays complete and testable |
| Unrepresentable routing state (R-10 D15, G-8) | refuse as incomplete | 03 D15 | hosts whose routing the observation cannot represent get no containment; no silent misreading |
| Drop-in order bound (R-8 Q9) | as systemd reports it | 01 Q9 | a check that the reported order matches the disk-side computation |
| Spelling of the skill account (R-3/R-9 D-7) | edit the five code sites to `domovoi-skill` | 02 D-7, ADR 0030 A1 | one commit; one snapshot to review; D-31 stands |
| Runner sandbox mechanism (R-3/R-9 D-5) | systemd builds it from the unit; no user namespaces at launch | 02 D-5 | works with AppArmor unchanged; unavailable on hosts without systemd; Landlock stays an idea |
| Per-package narrowing (R-3/R-9 D-8) | defer | 02 D-8, ADR 0031 B4 | v1 exposes the whole skills directory read-only |
| AppArmor or SELinux profile for the runner (R-3/R-9 D-9) | none at launch | 02 D-9 | a profile is a separate measured change |
| Slow-storage reaction (R-3/R-9 D-10) | warn only above a median of about 100 ms; never block | 02 D-10 | `selftest` ST-20 reports it |
| Selector ambiguity (G-10) | escape the interface name inside the item | 03 D4 | confirmed by running the vectors (`06-g10-selector-escaping-check.md`): R4 and the other seven vectors are unchanged; only names with bytes outside the literal set change on the wire; ADR 0032 C4's claim that R4 changes is wrong |
| Measure the Ubuntu 24.04 server | yes, non-destructive | 5b | needs passwordless `sudo` on that host; it runs a live agent, so only disposable probe users and units |

### Taken on the owner's standing instruction to use the recommendations

The owner said to stop asking and use the recommended option wherever they have not disagreed. These follow:

| Decision | Taken | Reference |
|---|---|---|
| Mandatory protected set in `04b` names `sshd.service` literally and the vectors depend on it | taken from the recommendation: the encoding keeps the literal name (vectors frozen); matching in the helper is by `Id` and `Names`, so a restart addressed to `ssh.service` is refused when its names include a protected one; the installer writes the host spelling and adds `ssh.socket` | 01 REQ-16 | no vector changes; the enforcement is wider than the encoding |
| `max_active` default | 0 (no containment) as in 04 and 04b; the synthesis figure of 50 is superseded by D7 | 03 D7 | the default configuration refuses all containment |
| Aliases as `unit.restart` targets (the Q2 decision says "refuse aliases"; REQ-9 lets plan time accept an alias name) | taken from the recommendation: a unit file that is itself a link or alias is refused (linked, alias `UnitFileState`); an alias *name* that resolves to a plain allowed unit is accepted and only its `Id` and `Names` are bound | 01 REQ-9, Q2 | `restart ssh.service` and `restart sshd.service` bind the same identity; no alias is ever bound by name |
| Generation sequence on `rotate-key` and `recover` | taken from the recommendation: a new key starts a new sequence (generation 0, first real attestation 1); the hub's high-water mark is kept per helper and key and is reset only by an owner re-pin of the key | 03 D3, D6 | a rolled-back state under the same key is rejected; a legitimate key change is an explicit owner action |
| Helper confinement versus the leak self-test (ADR 0031 B5) | taken from the recommendation: the self-test is a separate subcommand run by the installer and by a one-shot unit ordered before the helper socket when the kernel or nft changes; the long-lived helper never creates namespaces, so it can keep `RestrictNamespaces=yes` | 04 D14, 02 5.3 | the helper's privilege statement does not grow; the self-test unit has its own directive set |
| Token for a wrong state directory mode (ADR 0032 E2) | `helper_state_permissions` | 02 5.5, 03 | one token across helper, selftest and hub |
| Protocol version for the wire-type changes (ADR 0032 E1) | fold into the unreleased wire 1.2: the latest release (v0.0.11) carries 1.1 and 1.2 exists only on the overhaul branch, so no further bump | 03 D8 | the frozen golden frames of 1.2 are regenerated with the final types before release; no 1.3 |
| Where the helper code lives (ADR 0030 A2) | taken from the recommendation in brief 47a: one new crate `domovoi-helper` (edges: `core`, `config`) holds the root runtime, the host inspection adapter, the self-test and the repair command; pure types, evaluators and the tier function go in `domovoi-core`; the agent gets a small client module | ADR 0030 A2, 47a | the ADR is written in 47a's step 2 and confirmed at its launch gate |
| Circular launch gate (acceptance needs a real-installer pass that only 47a/47b can produce) | taken from the recommendation: the owner accepts R-3 and R-9 for building with the unproven items recorded as residual; a real-installer run on at least one real host is an exit criterion before merge; no platform is declared Supported | 47a | R-10 is not a gate for 47a |
| Hub and a separate agent unit on one host | taken from the recommendation: such hosts stay on the legacy layout labelled "privilege boundary not in effect"; the installer says why | 47a | the single-box mode is labelled the same way |
| Helper read operations for checks that need root (`docker.read`, `smart.read`, `kernel.read`, `net.read`) | gap found in 47a: no prompt implements them; taken from the recommendation: a new brief 47d after 47a, and the migration to separate identities warns before confirmation that root-needing checks report errors until 47d ships | 47a, new 47d | `--migrate-identity` keeps its typed confirmation and now states this |
| Reboot semantics (R-2 D10) | containment does not survive a reboot (permits are already invalid by `boot_id`); confirm after the reboot test, which is not yet run | 04 D10 |
| Tunnel targets (R-2 D11) | document the address as observed by the detector and say which layer it is | 04 D11 |
| Version floor and platform list (R-8 Q1, R-3/R-9 D-3, D-4) | the tier model of 5b; the gate names the tested versions | 01 Q1, 02 D-3, D-4 |
| Per-host opt-in for helper write groups on Untested platforms | an owner-set switch per host in the helper configuration, off by default, recorded in audit | 5b |

## 5b. Platform tiers (design of the owner's decision)

The owner chose to let the helper and runner run on any platform while publishing which are supported and which are not. The evidence supports this only if the isolation does not depend on the platform being known, so the gate is a runtime check, not a version list:

| Tier | Meaning | Skills | Helper read groups | Helper write groups (`unit.restart`, containment) |
|---|---|---|---|---|
| Supported | listed as release-tested (a platform is promoted here only after a real-installer pass with the production binaries) | allowed | allowed | allowed |
| Untested | systemd host that passes every isolation check at start (`selftest` ST checks; unit starts, masks effective, groups empty, modes correct) but is not release-tested | allowed with a visible "untested platform" notice and a recorded acknowledgement | allowed | refused unless the owner opts in per host, because their safety rests on kernel and nft behaviour that is version-specific |
| Blocked | any isolation check fails, or no systemd, or a container without a measured design | refused | refused for write; read allowed if its own checks pass | refused |

Nothing is Supported yet, because no platform has had the real-installer pass. The research gives these **measured candidates** (isolation checks passed with stand-in programs, once each):

| Platform | systemd | Kernel | Skills and helper read | Helper write: `unit.restart` | Helper write: containment |
|---|---|---|---|---|---|
| Debian 13, aarch64 | 257.13 | 6.18 | candidate | candidate (identity checks reproduce) | candidate on plain hash sets; interval sets leak 0 here |
| Ubuntu 26.04, x86_64 | 259.9 | 7.0 | candidate | candidate | candidate; interval sets leak 0 here |
| Ubuntu 24.04 LTS, x86_64 | 255.4 | 6.8 | candidate, once the three `selftest` wording fixes are in | candidate with the version gate naming 255 | Untested: interval-set replace leaks; plain hash sets did not; needs the leak self-test |
| WSL 2 (research host) | 259.5 | 6.18 | research only | research only | research only |

The Ubuntu 24.04 result is the reason the 257 floor was withdrawn: this very common deployment passes the isolation checks below it. Not covered at all: Debian 12 and Ubuntu 22.04 (systemd 252/249), systemd 258, SELinux hosts, containers, Unraid and other non-systemd hosts (Blocked by definition until a design exists).

## 6. Implementation plan

Dependency order from the index is unchanged: 44 (ADRs) then 47 (helper, attestation, skill runner), then 45 (permit and autonomy) and 46 (containment). What the research changes is what each must contain.

**44, ADRs (done in draft as 0030 to 0032, status proposed).** Update them from this research before acceptance: replace the RW-39 unit text with the corrected form; add the F-2, ST-check and mode requirements; correct 0032 C4 and the harness description; record the three code defects; list the unproven items from section 8.

**47a, accounts, sockets, helper skeleton.** Installer creates `domovoi-agent` and `domovoi-skill` (no groups), `/etc/domovoi` 0750, helper state 0700 root with 0600 key and lock, sockets with mode and group; helper start checks of directory, key and state modes (fail closed with tokens); `selftest` checks ST-01 to ST-20 from document 02; adoption of an inherited socket (decision D-6); `arch-check` rules named in 02 section 5.
**47b, skill runner.** Corrected unit files from document 02 section 5; `Accept=yes` with runner-side EOF cancel, own process group, exit 0, `RuntimeMaxSec` on `Type=simple`; `ProtectHome=yes`; no `TemporaryFileSystem=` on a path that may be absent; bind-through of the skills directory; refusal of any `run_as` but `domovoi-skill`.
**47c, attestation.** The encoding and grammars of document 03 section 5 (after decisions D1 to D4); key lifecycle; state file and recovery; hub verifier with the latest-outcome rule, a persisted high-water mark and issue-time freshness; required `expected_safety` for `Contain`; proto changes (required fields, `deny_unknown_fields`, an enum for reasons, hash encoding); golden vectors C1 to C4 and R1 to R4 frozen; both excluded RW-37 tests added.
**45, `unit.restart`.** Identity from D-Bus: loaded configuration plus a disk-side enumeration of the whole search path, matched by `Id` and `Names`; refuse masked, transient, generated, linked and unlisted-location units; plan-time and dispatch-time checks in the order of document 01 section 5; `NeedDaemonReload=no` required; `daemon-reload` never run by the helper; post-restart check of the loaded properties and `/proc/<MainPID>/environ` with alert-only handling; start-limit refusal reported, not retried; exact instance allowlist; protected-unit default matched by `Id`.
**46, containment.** One `prerouting` chain at -310 (this is a recommendation of the synthesis; a protecting `ct established accept` must sit after conntrack at -200 and is not recommended); plain hash sets for single-host targets and interval sets for prefixes only where the helper's start-time leak self-test passed on that platform (Ubuntu 24.04's stock kernel and nft leak on interval sets); a create-if-absent bootstrap that never re-adds over a live table; always-explicit element timeouts (reject 0); `create` for step 1; confirm; replace as `delete` plus `add` in one batch; rollback as `add` plus `delete` in one batch with a membership-read fallback; classification of expired versus rolled back by table check, JSON membership read and a write-ahead journal; exclusions checked in both overlap directions for both families; conntrack flush when flowtables exist; table-loss detection with `containment_lost` and re-creation of the empty structure; `max_elements` cap; verification probe; unit ordering after `nftables.service`; unit tests in an unprivileged namespace in CI and a real-host matrix.

**Brief consistency pass.** The briefs for 47a, 47b, 47c, 47d, 45 and 46 were written by separate sessions and then reconciled as a set (`prompts/README.md`: "Launch order", "Cross-brief names", "Test ownership", "Cross-brief decisions", "Known gaps and deferred work"). The order is 47a, 47b, 47c, 47d, 45, 46. Two statements above are superseded by the briefs: 45 follows REQ-20 for the post-restart check (it reads `/proc/<ExecMainPID>/environ` and never re-reads the loaded properties, so the wording of the 45 paragraph above is out of date), and the helper may spawn the fixed programs the later briefs name (`ip`, `smartctl`, `dmesg`, `ss`, `systemctl`, `busctl`, `nft`, `conntrack`) through the injected process port. Section 5a's helper read operations (47d) and the persisted limiter (45) are owned.

## 7. Improvement ideas worth keeping (each is an idea, not evidence)

Ranked by expected value over cost; each has benefit, cost and a test in the four documents.
1. Hub-side high-water mark of generation and hashes, persisted in the acceptance transaction (closes the rollback gap).
2. Read unit identity over D-Bus in one call and hash the structured property set plus the disk enumeration; fail closed on drift.
3. `nft -j` membership reads and a write-ahead journal for exact apply and rollback accounting.
4. Landlock as a sandbox layer for the runner where no user namespace or AppArmor rule is needed.
5. Per-execution post-start verification by cgroup and `InvocationID` rather than by property re-read.
6. A netlink binding instead of the `nft` CLI to remove the re-read cost and text parsing (later, with a larger `max_elements`).
7. Pinning a restart to `InvocationID` or `ActiveEnterTimestamp` to detect a change between plan and dispatch.
8. A `selftest` mode that prints the eight-point isolation report for support.

## 8. Remaining validation, cheapest first

1. **Real installer, production binaries.** Real helper and runner, real accounts, the corrected units, on the four hosts. Removes the stand-in caveat (smallest cost, biggest confidence gain).
2. **Reboot test on the Pi** (owner decides; needs a way to power-cycle): boot ordering with Docker, `nftables.service` and the helper; containment not surviving a reboot; permit invalidation by `boot_id`.
3. **More distros and versions for each item:** Debian 12 (systemd 252), Ubuntu 22.04 (249) and systemd 258 to bound the tested range; a non-merged `/usr` host; a Fedora or RHEL-family host for SELinux and firewalld defaults.
4. **Live routing:** policy rules, multiple tables, a VPN, two gateways, to replace the fixture routes in the attestation harness.
5. **Separate kernel from nft version** for the interval-set replace leak and the `add`-refresh difference: nft 1.0.9 userspace on kernel 6.18 and nft 1.1.x on kernel 6.8 (an extracted distro package or a VM), then other kernels, `br_netfilter`, hardware offload, Podman, and a real NIC with a leak self-test under realistic packet rates.
6. **Race statistics:** more trials per row on one host with an in-process bus client; unit files with `ExecReload` and `Type=notify`.
7. **Power-loss durability** of the state file on a real filesystem.

## 9. Before any of this is pushed

The raw output files in the Pi and Ubuntu probe directories contain host details (LAN addresses, container names, firewall rules). Review and redact them before any research branch is pushed.
