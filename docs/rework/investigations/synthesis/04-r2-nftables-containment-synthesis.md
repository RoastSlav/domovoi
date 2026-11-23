# R-2 synthesis: nftables containment (four hosts, five environments)

Scope: research item R-2, the validation gate for the M9 containment capability `firewall.block_source` and for the
helper operations behind it. This document consolidates five research reports, analyses them against the design text,
draws conclusions and turns them into requirements, ideas and tests for the implementer of containment (implementation
index item 46) and of the helper. It adds no new measurements; the fifth report (Ty) was re-checked against its raw
logs where this document relies on it (section 1.1). It does not resolve or accept R-2: **R-2 stays open**,
containment stays gated, and no supported-platform claim is made. The fifth report found that the recommended replace
is not packet-atomic on interval sets on Ubuntu 24.04's stock kernel and nft; sections 2.4, 2.16, 3.1, 4, 5 and 9 are
revised for it and section 11 lists every change.

Nothing here edits a design document. Section 8 lists the edits for the owner to make. Everything marked
"recommendation" or "idea" is a proposal, not an accepted design.

## 1. Scope, sources and environments

### 1.1 Citation scheme

Every claim cites a report and a section or experiment of it. `I` is `I-r2-nftables-containment.md`, `P` is
`P-pi-r2-nft113.md`, `Q` is `Q-pi-r2-round2-nftables.md`, `R` is `R-r2-nft116-kernel7-host-c.md`, `Ty` is
`W-host-t-r2-nft109-kernel68.md` (the file's letter W collides with environment W below, so it is cited as `Ty`).
`I E9` means report I, experiment E9; `P 3.14` means report P section 3.14; `Q 3.4`, `R 3.15`, `Ty 3.10` likewise.
Raw outputs are under `docs/rework/investigations/probes/` in `r2` (I), `pi2` (P), `pi2b` (Q), `host-c` (R) and
`host-t2` (Ty), each with an `out/` directory.
Where a claim was re-checked against a raw log the log is named. For Ty the sums of leaked packets and
replacements in 3.9 and 3.10 were recomputed from `host-t2/out/p09.log`, `p21.log` to `p28.log`; section 11 lists what
could not be checked. Labels follow the reports: **demonstrated**,
**not demonstrated**, **requiring conventional Linux validation**. "Synthesis calculation" marks a number or
inference computed here, not reported by a report.

Design text read: RW-13, RW-23 and RW-37 in `03-architecture-decisions.md`; `04-target-architecture.md` section 9.1
(and 8.3 for the helper limits); `99-open-decisions.md` R-2, D-22, D-28; `04b-attestation-encoding.md` only where
address and prefix canonical forms matter; the implementation index (the `07-*` file) item 46; the R-2 research brief.
`04` section 9.1 does not name a hook. The `input` hook used in the reports' reference sequences comes from the research
brief, not from the design.

### 1.2 The five environments

| Env | OS | nft | Kernel | Arch | Firewall managers present | Docker backend | Scope |
|---|---|---|---|---|---|---|---|
| W (report I) | Ubuntu 26.04.1 on WSL2, systemd 259 | 1.1.6 (extracted `.deb`, not installed) | 6.18.58.1-microsoft-standard-WSL2 | x86_64, 4 KiB pages | none; empty default ruleset | none | throwaway namespaces only (I section 1, 2) |
| P (report P) | Debian 13, systemd 257 | 1.1.3 (distribution package) | 6.18.60+rpt-rpi-2712 | aarch64 (Pi 5), 4 CPUs, 16 KiB pages | none; `nftables.service` disabled | Docker 29.10.0, iptables-nft | namespaces; default namespace with probe-gated rules; real NIC `eth0` (P section 1, 2) |
| Q (report Q) | same Pi as P | 1.1.3 on host; 0.9.8, 1.0.2, 1.0.6, 1.0.9, 1.1.6 as userspace in containers against the same kernel | same as P | same as P | ufw 0.36.2 (iptables-nft), firewalld 2.3.1 (nftables backend), temporary; `nftables.service` started once | iptables-nft and native nftables backend, IPv6 modes | namespaces; default namespace gated; `eth0` and `wlan0` through ipvlan children (Q section 1, 2) |
| C (report R) | Ubuntu 26.04.1, systemd 259 | 1.1.6 (distribution package) | 7.0.0-43-generic | x86_64, 8 CPUs, 4 KiB pages | ufw active (iptables-nft, `INPUT` and `FORWARD` policy drop); `nftables.service` disabled; firewalld inactive | Docker 29.9.3, iptables-nft, live containers | namespaces only; the default namespace was read, never written (R section 1, 2) |
| T (report Ty) | Ubuntu 24.04.5, systemd 255 | 1.0.9 (distribution package) | 6.8.0-149-generic (Ubuntu's stock kernel for 24.04) | x86_64, 16 CPUs, 4 KiB pages | ufw (iptables-nft, `INPUT` and `FORWARD` policy drop); a ban service's `inet` table with an `input` chain at priority -1; `nftables.service` not examined | 27 running containers, iptables-nft | namespaces only; the default namespace was read, never written; load average 17 to 25 on 16 CPUs during the run (Ty 2) |

Independence caveat, applied to every "N of 5" below: **P and Q are one machine, one kernel and one Debian nft 1.1.3.**
There are five environments but four distinct hosts and four kernel/userspace pairs (6.18.58.1 with 1.1.6, 6.18.60
with 1.1.3, 7.0.0-43 with 1.1.6, **6.8.0-149 with 1.0.9**); three of the four kernels are 6.18 or newer, and T is the
only kernel older than 6.18 and the only pairing of nft 1.0.9 with a kernel other than 6.18.60. Q's older nft versions
(0.9.8 to 1.0.9) ran against the 6.18.60 kernel only, so they vary userspace and say nothing about older kernels
(Q 3.3). Every default-namespace, real-NIC, Docker, ufw, firewalld, flowtable, bridge, WireGuard and IPv6 finding
comes from the one Pi (P, Q): treat those as **single-host only** however many sections repeat them. Hosts C and T
could not exercise any of them because they are live servers (R 2, 3.16; Ty 2); host W has no Docker, no manager and
no physical NIC. **Single-host findings of T** (nothing else reproduces them): the interval-set replace leak and all of
its neighbour analysis (2.16), the no-op of `add` on an existing element (2.2; the Q log points at the kernel), the
bootstrap re-run side effects, the `create` success paths, the `size` read-back for `interval,timeout`, the `-310`
`ct` ordering rows (2.17), the `nft` 1.0.9 memory and time figures, and the `nft_ct` side effect.

Result notation in the tables: `y` reproduced, `n` contradicted, `-` not tested in that environment, `~` partly.
"N of 5" counts environments, then "(hosts k)" gives distinct hosts (P and Q count as one). Class: **U** universal in
this evidence (same result wherever tested, and the mechanism is kernel or nft grammar), **H** host-dependent
(differs or depends on the configuration), **K** pairing-dependent (the result differs between kernel/userspace pairs
that were run; which side causes it is stated where known), **S** single-host only (one host produced it). "Universal"
is a statement about the pairs run, not about Linux: after T, only rows with T = `y` or a stated reason T was not
tested may be read as U across the pairs; the tables below were reclassified accordingly.

### 1.3 Contradictions and corrections between the reports

| # | Topic | Earlier statement | Later statement | Which corrects which |
|---|---|---|---|---|
| 1 | Rollback with `destroy` | I section 4: roll back with `destroy element` "where available" | `destroy` is idempotent on plain sets only; on interval sets it exits 1 `element does not exist` on 1.0.9, 1.1.3, 1.1.6 and kernels 6.8, 6.18 and 7.0, also through JSON and libnftables; it does not exist before 1.0.9 (P 3.14, Q 3.3, R 3.14, Ty 3.12) | P, Q, R, Ty correct I |
| 2 | `nftables.service` and published ports | P 3.21: published ports stop until dockerd re-adds its rules | after a real `systemctl start nftables` (flush) the published port still worked through `docker-proxy` (input path) with the default `userland-proxy`; recovery needed `systemctl restart docker` (Q 3.1) | Q corrects P (on one host, default Docker configuration) |
| 3 | Docker `FORWARD policy drop` | P 3.15, 5.3: "FORWARD has policy drop" | it was set at boot and is not re-established by a daemon restart (Q 3.2) | Q narrows P |
| 4 | Docker and equal priority | P 3.15: Docker may re-register its chains on restart (not demonstrated) | restarts keep the order in both backends; only creating our chain before Docker's flips it (Q 3.2) | Q corrects P's worry, confirms the boot-order risk |
| 5 | `size` at 2^32 | P 3.19, R 3.13: `size 4294967295` and `4294967296` created with rc 0; what is stored not verified | Q 3.3: `size 4294967295` fails `Cannot allocate memory`; `4294967296` stores 0, `4294967297` stores 1 | **Resolved for one pairing only.** The methods differed: P (`mem.py`) and R create sets with `flags timeout; size N`, Q creates `{ type ipv4_addr; size N; }` with no flags. Ty `p23` D read all three flag combinations back on kernel 6.8 / nft 1.0.9: `4294967297` stores 1 for flagless, `flags timeout` and `flags interval,timeout` (a second element is refused with `Too many open files in system` for the last two); `4294967296` lists no size; flagless `4294967295` fails `Cannot allocate memory` as in Q; `flags timeout` with `4294967295` is accepted and listed as 4294967295; `flags interval,timeout` with `4294967295` is listed as **2147483647**. So the wrap is not limited to flagless sets (Ty 3.11, `host-t2/out/p23.log` section D). It is not shown for `interval,timeout` on 6.18 or 7.0. The recommendation to refuse a size at or above 2^32 stands, and 2^31 - 1 is the largest value stored unchanged for an interval set on T |
| 6 | Input-only hook | I 3.3 / E3 and the brief's reference use `input`; I section 4 calls `input` alone insufficient for a router or Docker host | demonstrated against Docker's real chains and from another machine over `eth0` (P 3.15, 3.17), in both Docker backends and for IPv6 (Q 3.2, 3.6) | P, Q turn I's argument into a demonstration (single host) |
| 7 | host-c as a host | Q section 8: no root, no namespace matrix | R ran the matrix as root on the same host after the sudoers drop-in existed | R supersedes Q section 8 |
| 8 | Add on an existing element | I E9: plain `add` refreshes (plain set only); Q 3.3: refresh on plain and interval sets on six userspace versions (all against kernel 6.18.60) | Ty 3.5: on kernel 6.8 with nft 1.0.9 `add` on an existing element changes nothing (not longer, not shorter, not to the set default), plain and interval; `p04`, `p23` C, `p26` | **Q's refresh is not universal.** Q's Ubuntu 24.04 userspace container ran the same nft 1.0.9 and refreshed against kernel 6.18.60 (`pi2b/out/q03_core_ubuntu-24.04.log`), so the difference points at the kernel (6.8 versus 6.18 and 7.0). The container's build identity against T's distribution package was not compared. Shortening by `add` is now tested on 6.8 only (no-op) |
| 9 | Interval `destroy` on 1.1.6 | P 3.14 / I: plain sets only on 1.1.6, so 1.1.6 unknown | R 3.14: same failure on 1.1.6 and kernel 7.0 | R closes it |
| 10 | Display of listings | `# count N` (W, C) vs only `size N` (P) | follows nft version, not kernel (R section 5); T (1.0.9) prints no `# count`, several elements per line and `raw - 10` for priority -310 (Ty 5) | no conflict; helper must not parse listings |
| 11 | "One batch of delete + add is never absent" | W, P, Q, C: 0 leaked packets in about 450 000 atomic batches on kernels 6.18 and 7.0 (section 2.4) | Ty 3.9, 3.10: 4 711 leaked in 101 159 batches on an interval set, kernel 6.8, nft 1.0.9; plain hash sets 0 of about 131 000 | **Ty narrows the earlier statement.** The method is the same (the `p09` load scripts of P, R and Ty differ only in namespace names and in options Ty added; the sender script is byte-identical; the replacer goes through libnftables in all three; all create the same `interval,timeout` set; checked by `diff` of the scripts). Kernel versus userspace is not separated (2.16) |
| 12 | `[validate]` items of the previous revision | `create` success path, `add set` / `add chain` re-adds, `flush chain` plus `add rule`, `size` for interval sets, add shortening, the rollback batch on nft 1.0.x and a pre-6.18 kernel, `-310` and `ct` | Ty 3.5, 3.11, 3.13, 3.14 ran each of them once on 6.8 / 1.0.9 | Closed for that one pairing; each stays open for the others (section 10) |

## 2. Consolidated findings

### 2.1 Set types and interval sets

| Finding | W | P | Q | C | T | Reproduced | Class | Evidence |
|---|---|---|---|---|---|---|---|---|
| An interval set holds prefixes and ranges; overlap inside one set is refused in both insertion orders with `interval overlaps with an existing one` (before 1.0.6 the kernel adds `File exists`) | y | y | y | y | y | 5 of 5 (hosts 4) | U | I E11; P 3.11; Q 3.3; R 3.10; Ty 3.7, 3.13 |
| Nothing refuses loopback, link-local, `0.0.0.0`, `0.0.0.0/0`, `::/0`; a plain set accepts them as hosts | y | y | y | y | y | 5 of 5 (hosts 4) | U | I E11; P 3.11; Q 3.3, 3.6; R 3.10; Ty 3.7 |
| A prefix into a set without `flags interval` is refused: `You must add 'flags interval' to your set declaration if you want to add prefix elements` (ranges likewise) | - | - | - | y | - | 1 of 5 (hosts 1) | S (grammar) | R 3.15, `host-c/out/p15.log` |
| An `ipv6_addr` interval set accepts `::1`, `fe80::/10`, `fd00::/8`, `::ffff:0:0/96`, `/127`, `/128`; `/129` is `Prefix length 129 is invalid for type of 128 bits width`; `::/0` and `fd00::/8` overlap | - | - | y | - | - | 1 of 5 (hosts 1) | S | Q 3.6 |
| An `ipv4_addr` set accepts `::ffff:10.0.0.1` and stores `10.0.0.1`; a dotted IPv4 literal in an IPv6 set is a hostname-resolution error | - | - | y | - | - | 1 of 5 (hosts 1) | S | Q 3.6 |
| Kernel slab per element: about 64 B plain hash, about 128 B interval (start and end), about 95 to 127 B IPv6 plain; `nft` peak RSS for a 100 000-element batch 38 to 39 MB plain, 78 MB interval | - | y | - | y | y (slab 64 B and 128 B; `nft` RSS 44 MB plain, **151 MB** interval) | 3 of 5 (hosts 3) for slab sizes; the `nft` RSS figure is **not** reproduced on T | U for slab size across arch, page size and kernel 6.8; **H** (nft version) for `nft` RSS and load time | P 3.19; R 3.13; Ty 3.11, section 5 rows 6 to 8 |
| A range counts as one element against `size` although it uses two kernel objects | y | y | - | y | y | 4 of 5 (hosts 4) | U | I E6; P 3.6; R 3.5; Ty 3.3 |

### 2.2 Timeout semantics

| Finding | W | P | Q | C | T | Reproduced | Class | Evidence |
|---|---|---|---|---|---|---|---|---|
| `timeout 0s` takes the set's default timeout (29s996ms of 30 s); in a set with the `timeout` flag and no default the element **never expires** | y | y | y | y | y | 5 of 5 (hosts 4) | U | I E7; P 3.7; Q 3.3; R 3.6; Ty 3.3 |
| Accepted: `1h30m`, `1d`, `500ms`, `31536000s`, `4294968s` (no 2^32 ms wrap). Refused: `99999999999s` (`value too large`), `-1s`, `1.5s`, `30`, `30S`, `1h30` (syntax errors), `abc` (`wrong time format`) | y | y | y | y | y | 5 of 5 (hosts 4) | U | I E7; P 3.7; Q 3.3; R 3.6; Ty 3.3 |
| An element with `timeout` into a set without the `timeout` flag: kernel `Could not process rule: Invalid argument` | y | y | y | y | y | 5 of 5 (hosts 4) | U | I E7; P 3.7; Q 3.3; R 3.6; Ty 3.3 |
| `add element X timeout 300s` on an existing element **refreshes** the timeout (30 s to 300 s), on plain and interval sets; an `add` on an expired element silently re-creates it | y (plain) | y | y (plain and interval, six userspace versions) | y (plain) | **n**: no change, longer, shorter, absent timeout or `timeout 0s`, plain and interval (an `add` on an expired element re-creates it, as elsewhere) | refresh on 4 of 5 (hosts 3); **not on T (hosts 4)** | **K** pairing-dependent; the Q log points at the kernel (see 2.16) | I E9; P 3.9; Q 3.3 (`q03_core_ubuntu-24.04.log`); R 3.8; Ty 3.5 (`p04`, `p26`, `p23` C). Same nft 1.0.9 refreshed against kernel 6.18.60 in Q (`pi2b/out/q03_core_ubuntu-24.04.log`) |
| `create element X` on an existing element fails `Could not process rule: File exists` (plain and interval) | y | y | y | y | y | 5 of 5 (hosts 4) | U | I E9; P 3.9; Q 3.3; R 3.8; Ty 3.5, 3.14 |
| Shortening an element's timeout with `add`; `create` of an absent element succeeding (success path never printed as its own probe line) | - | - | - | - | **y** (T only): `add` with 20 s, 5 s, 90 s, 300 s, no timeout or `0s` on an existing element changes nothing, on plain and interval sets; `create` succeeds on an absent plain host, interval host, prefix and range and on an expired element | 1 of 5 (hosts 1) | S (single host, kernel 6.8); not run on 6.18 or 7.0 | `grep` of all logs: only the `File exists` failure is recorded; Ty 3.5, `p23` C, `p26` |
| Listing text varies: `365d` vs `364d23h59m59s988ms`, `# count N` only in 1.1.6, elements on one line before 1.1, no `timeout` printed for interval prefixes before 1.1 | y | y | y | y | y (no `# count`, several elements per line, `priority raw - 10` for -310) | 5 of 5 | H (display) | P section 4; Q 3.3; R section 5; Ty 5 |

### 2.3 Expiry independence from processes

| Finding | W | P | Q | C | T | Reproduced | Class | Evidence |
|---|---|---|---|---|---|---|---|---|
| A 3 s element added by a shell that exited is present 0.3 s later (about 2.7 s left) and gone by 4.3 s, plain and interval | y | y | - | y | y | 4 of 5 (hosts 4) | U | I E8; P 3.8; R 3.7; Ty 3.4 |
| A 30 s element whose applier was `SIGKILL`ed disappears 29.2 to 29.3 s after the kill with no process running | y | y | - | y | y | 4 of 5 (hosts 4) | U | I E8; P 3.8; R 3.7; Ty 3.4 |
| Elements keep counting down through `ufw` reload/disable/enable/restart/reset and `firewalld` reload/restart/stop/start (4m57s to 4m53s of 300 s) | - | - | y | - | - | 1 of 5 (hosts 1) | S | Q 3.1 |
| 100 000 plain elements with `timeout 15s`: all present at 5 s, none at the next sample at 15 s; the instant of the last expiry was not sampled | - | y | - | y | y | 3 of 5 (hosts 3) | U | P 3.19; R 3.13; Ty 3.11 |
| Survival or expiry across a reboot, a suspend or a clock jump | - | - | - | - | - | 0 of 5 | not demonstrated | I E13; P 3.20; Q 3.10; R 3.16 |

### 2.4 Replace atomicity and replace under load

Replace means one `nft -f` batch of `delete element X` plus `add element X timeout T`. All replace probes used a
single namespaced set of senders; the control splits the batch into two commands and leaks about 40% of packets, so
the probe does see the absent window when it exists.

| Host and probe | Packets | Atomic replacements | Atomic leaked | Control (split) leaked | Evidence |
|---|---|---|---|---|---|
| W, ICMP, 2500 echoes at about 100 pps | 2 500 | 1 408 | 0 | 2 156 of 2 500 (345 replacements) | I E14 |
| P, ICMP, same method | 2 500 | 1 265 | 0 | 2 129 of 2 500 | P 3.13 |
| P, UDP over veth, two senders, 5 runs x 8 s per method, 1 element | 27.4 M | 245 391 | 0 | 9 087 800 (about 39%) | P 3.16 |
| P, same with 10 000 other elements | 25.7 M | 1 064 (about 25 per s) | 0 | 8 769 829 (about 40%) | P 3.16 |
| P, plain `add` (refresh) | 26.9 M | 55 544 | 0 | n/a | P 3.16 |
| P, no replace | 30.1 M | 0 | 0 | n/a | P 3.16 |
| Q, real NIC `eth0`, flood from another host, 100 000 plain / 100 000 interval / 1 000 interval | about 26 M in all | 704 / 19 / 570 | 0 | n/a | Q 3.8 |
| C, ICMP, same method | 2 500 | 1 629 | 0 | 2 005 of 2 500 | R 3.12 |
| C, UDP over veth, 3 runs x 6 s per method, 1 element | 16.8 M | 197 182 | 0 | 5 520 968 (about 40.9%) | R 3.12 |
| C, same with 10 000 other elements | 14.5 M | 986 (about 51 per s) | 0 | 5 104 691 (about 40.8%) | R 3.12 |
| **T**, ICMP, 2 500 echoes at about 100 pps | 2 500 | 1 213 | 0 (blind: expected leak about 0.01 packet) | 2 247 of 2 500 (247 replacements) | Ty 3.9 (`p05.log`) |
| **T**, UDP over veth, 3 runs x 5 s per method, 1 **interval** element, kernel 6.8, nft 1.0.9 | 6.76 M | 101 159 (about 6 200 per s) | **4 711** (1 577, 1 557, 1 577; about 0.047 per replacement; recomputed from `p09.log`) | 2 310 246 of 5.9 M (38.9%) | Ty 3.9 |
| **T**, same with 10 000 other elements, all below the target | 5.7 M | 338 (about 21 per s) | 0 | 1 993 531 (about 40.6%) of 4.9 M | Ty 3.9 |
| **T**, plain `add` (a no-op on this kernel, 2.2) | 6.8 M | 28 107 | 0 (nothing changes) | n/a | Ty 3.9 |
| **T**, no replace | 7.0 M | 0 | 0 | n/a | Ty 3.9 |
| **T**, the same method on a plain **hash** set, one element | not summarised | 64 623 (`p21`) | 0 | 1 224 537 of 3.1 M (39%) | Ty 3.10 |

**Result after T: the replace property is pairing-dependent (class K), not universal.** Zero leaks on the three
earlier hosts (W, P/Q, C: kernels 6.18.58, 6.18.60 and 7.0.0-43, nft 1.1.3 and 1.1.6; three of the four hosts) and on plain hash
sets on T; leaks on **interval** sets on T (kernel 6.8.0-149, nft 1.0.9) at about 450 000 packets per second, including
the exact layout of 5.1 (2.16). The controls leak about 39 to 41% on every host, so the probes do see an absent window
when there is one. Class **K** for the veth path (the replacer and sender scripts are the same on all hosts); **S for
the real NIC** (Q only, 6.18).
Synthesis calculation: on W, P, Q and C the atomic and refresh batches listed (about 450 000 atomic batches across
rows, excluding the refresh row) leaked no packet. All load probes of P, C and T used an `interval,timeout` set; the
only plain-set atomic load results are Q's real-NIC row (100 000 plain elements, 704 batches, 6.18) and T's `p21`
plain rows (64 623 and 66 976 batches, 6.8). This is a count, not a bound for production: it applies to UDP/ICMP
through veth or one real NIC, one receive queue, no conntrack rule, no flowtable, no offload (P section 7, R section 7,
Q section 9, Ty 7). The window sampled is about 10^4 times larger in P and C than in W, whose own report calls it thin
(I E14); the low-rate ICMP probes (W, P, C and T's `p05`) are blind to an effect of about 0.05 packet per replacement
(Ty 3.9), so their zero results carry no weight on this question.

Other replace facts:

| Finding | W | P | Q | C | T | Reproduced | Class | Evidence |
|---|---|---|---|---|---|---|---|---|
| Replace of an expired element fails (plain: kernel `No such file or directory`; interval: userspace `element does not exist`), exit 1, element not re-created | y | y | y | y | y | 5 of 5 (hosts 4) | U | I E9; P 3.9; Q 3.3; R 3.8; Ty 3.5 |
| `delete` of an absent element inside a batch aborts the whole batch (the `add` is not applied) | y | y | y | y | y | 5 of 5 | U | I E9; P 3.9; R 3.8; Ty 3.5 |
| Replace on a full set succeeds (the delete frees the slot) | y | y | y | y | y | 5 of 5 | U | I E10; P 3.10; R 3.9; Ty 3.6 |
| 20 concurrent replaces of one element: 20 of 20 exit 0, final membership 1, kernel events strictly paired `DADA...` (overlap measured: median concurrency 20, 187 to 190 of 190 worker pairs) | ~ (20 parallel, overlap not measured) | y (25 rounds) | - | y (15 rounds) | y (10 rounds) | 4 of 5 (hosts 4) | U | I E12; P 3.18; R 3.11; Ty 3.8 |
| 20 different adjacent `/32` adds in parallel: all 20 present, no lost update; long batch (1 delete + 250 adds + 1 add) succeeds | - | y | - | y | y | 3 of 5 (hosts 3) | U | P 3.18; R 3.11; Ty 3.8 |

### 2.5 Rollback primitives

Set type matters. "Absent" and "expired" behave alike. Versions are nft userspace; all ran against kernel 6.18.60
except 1.1.6 on kernel 7.0 (C) and kernel 6.18.58 (W), and 1.0.9 on kernel 6.8.0-149 (T, the only older kernel).

| Primitive | Plain set | Interval set | Evidence |
|---|---|---|---|
| `delete element X`, X present | exit 0 | exit 0 | all hosts |
| `delete element X`, X absent or expired | exit 1, `Could not process rule: No such file or directory`; inside a batch it aborts the batch; same on 0.9.8 to 1.1.6 | exit 1; `element does not exist` on 1.0.6, 1.0.9, 1.1.3, 1.1.6 (also 1.0.9 on kernel 6.8); `interval not found in set` plus kernel ENOENT on 0.9.8 and 1.0.2 | I E9; Q 3.3 (all six versions); R 3.15; Ty 3.5 |
| `destroy element X` on nft 0.9.8, 1.0.2, 1.0.6 | syntax error (`unexpected element`), keyword absent | same | Q 3.3 |
| `destroy element X`, X absent, nft 1.0.9, 1.1.3, 1.1.6 | exit 0 (netlink `NFT_MSG_DESTROYSETELEM`, kernel 6.18 and 7.0) | **exit 1** `element does not exist`; same for `/24`, `/32` spelling, range, sub-range; same through `nft -j` and libnftables (the check is in userspace); in a batch the whole batch is refused | P 3.14; Q 3.3, 3.4; R 3.14; Ty 3.12 (kernel 6.8 accepts `NFT_MSG_DESTROYSETELEM`; plain absent exit 0, interval absent exit 1) |
| `destroy` through a raw netlink client (bypassing the userspace check) | - | not demonstrated | R 3.14 |
| One batch `add element X timeout 5s` + `delete element X` (candidate "C" in R, "m3" in Q) | exit 0 for X present, absent, expired | exit 0 for present, absent, expired and for prefixes; inside a composite batch only X is removed | Q 3.4 (nft 1.1.3, kernel 6.18.60), R 3.15 (nft 1.1.6, kernel 7.0), **Ty 3.13 (nft 1.0.9, kernel 6.8: present, absent, expired, exact `/24`, both set types)** |
| Failure modes of the batch form | full set, X absent: the `add` overflows (`Too many open files in system`, exit 1) | same, plus a second `No such file or directory` for the delete; a covering element present (target `/32` inside a present `/24`): `interval overlaps with an existing one`, exit 1, the `/24` untouched | Q 3.4; R 3.15; Ty 3.13 (on the full interval set the second error cites the `add` line on nft 1.0.9, the `delete` line on 1.1.6) |
| Read membership, then `delete` (two commands, "m4"/"G") | outcome safe but not atomic: 220 of 250 raced rollbacks failed (present at read, gone at delete: `No such file or directory` 198, `element does not exist` 22) | same | Q 3.4; R 3.15 |
| The transient add inside the batch form is invisible to packets | 1.52 M packets with source = the element, 329 batch rollbacks of an absent element: 0 dropped | same set type tested | Q 3.4 |
| `flush set` | exit 0, not selective | same | R 3.15 |

Reproduced: "`delete` of an absent element fails on both set types" 5 of 5 (hosts 4); "interval `destroy` of an absent
element fails" 4 of 4 tested (P, Q, R, T; W did not test interval) (hosts 3), on three kernels (6.8, 6.18, 7.0) and
three userspace versions (1.0.9, 1.1.3, 1.1.6); "batch form idempotent" 3 of 3 tested (Q, R, T) (hosts 3) on 3 of the 6
nft versions (1.0.9, 1.1.3, 1.1.6) and 3 kernels (6.8, 6.18.60, 7.0). Older userspace than 1.0.9 for the batch form
was not run (Q 3.4). Class: U for the `delete` and `destroy` findings, "three-pair" (not universal) for the batch form.

### 2.6 Expired versus rolled back

| Finding | W | P | Q | C | T | Reproduced | Class | Evidence |
|---|---|---|---|---|---|---|---|---|
| A replace that loses a race to a rollback fails with the same text as a replace on an expired element; the final state was absent in every race (25 of 25 rounds P; 15 of 15 C); a replace never resurrected a rolled-back element (event sequences `D` or `DAD`) | - | y | y (m4 race, 25 rounds) | y | y (10 of 10 rounds) | 4 of 5 (hosts 3: P and Q are one host) | U | P 3.18; Q 3.4; R 3.11; Ty 3.8 |
| On interval sets both userspace and kernel ENOENT texts appear in the same round | - | y | - | y | y | 3 of 5 (hosts 3) | U | P 3.18; R 3.11; Ty 3.8 |

The error text therefore carries no information about why the element is absent. Membership and the journal must
decide (section 5.7).

### 2.7 Capacity, `ENFILE`, `size` and memory

| Finding | W | P | Q | C | T | Reproduced | Class | Evidence |
|---|---|---|---|---|---|---|---|---|
| Full set: `Error: Could not process rule: Too many open files in system` (ENFILE), exit 1, for plain and interval sets, single add and whole batch; re-adding an existing element succeeds; expiry frees slots | y | y | y | y | y | 5 of 5 (hosts 4) | U | I E6; P 3.6; Q 3.3; R 3.5; Ty 3.3 |
| A kernel-rejected batch is rolled back in full (original elements remain, delete did not stick) | y | y | y | y | y | 5 of 5 | U | I E10; P 3.10; R 3.9; Ty 3.6 |
| 1 000 and 10 000 elements per batch: 0.01 to 0.06 s | y | y | - | y | y | 4 of 5 | U | I E6; P 3.6; R 3.5; Ty 3.3 |
| 2 000 000 plain elements, 0.33 s per batch constant, +136 MB slab, no kernel ceiling; interval to 500 000: batch time 0.64 to 7.13 s, `nft` 78 to 498 MB | - | y | - | n/t (capped at 500 000 plain and 200 000 interval) | n/t (plain to 300 000, interval to 100 000) | 1 of 5 full; C and T partial | S (ramp) | P 3.19; R 3.13; Ty 3.11 |
| `size` wraps modulo 2^32 for a flagless hash set (`4294967296` stores 0 = unlimited; `4294967297` stores 1); `4294967295` fails `Cannot allocate memory` in that test; with `flags timeout` the same sizes were accepted but not read back | - | ~ | y | ~ | y, read back for all three flag sets: `4294967297` stores 1 for flagless, `flags timeout` and `flags interval,timeout`; `4294967296` lists no size; `4294967295` flagless fails `Cannot allocate memory`, with `flags timeout` is accepted and listed, with `interval,timeout` is listed as 2147483647 | wrap shown for `interval,timeout` on T only (1 of 5, hosts 1); on the other four environments the method gap of 1.3 #5 stays | H (method) on 6.18 and 7.0; **S** for the interval read-back | P 3.19; Q 3.3; R 3.13; Ty 3.11 (`p23` D) |
| `size -1` and `size abc` are syntax errors | - | y | y | y | y | 4 of 5 | U | P 3.19; Q 3.3; R 3.13; Ty 3.11 |
| Memory returns when the table is deleted (135 of 136 MB) | - | y | - | y | y | 3 of 5 | U | P 3.19; R 3.13; Ty 3.11 |

### 2.8 Cost of replace versus set size, and the `nft` re-read

| Set | W | P | Q | C | T |
|---|---|---|---|---|---|
| 1 element, atomic replacements per second | n/m | about 5 800 | n/m | about 10 300 | about 6 200 (interval; plain hash about 7 300) |
| 10 000 other elements (plain, 5 replace batches W; atomic P, C) | 0.119 s per 5 batches (about 24 ms each) | about 25 per s (about 40 ms each) | n/m | about 51 per s (about 20 ms each) | about 21 per s (about 48 ms each; interval, host at load 17 to 25) |
| 100 000 plain | n/m | n/m | 19 to 27 ms per batch; 50 per s under TCP load | n/m | n/m per replace; load of the 100 000-element batch 0.46 to 0.50 s, `nft` 44 MB |
| 100 000 interval | n/m | n/m | about 610 ms per batch (609 to 617 ms in five runs); 13 replaces in 10 s | n/m | n/m per replace; load of the 100 000-element batch 1.3 to 1.5 s, `nft` **151 MB**, against about 0.4 s and 78 MB for nft 1.1.x |
| 500 000 interval | n/m | 7.13 s and 498 MB per batch | n/m | n/m | n/m |
| Throughput with a replace loop on 100 000 interval elements (veth TCP) | n/m | n/m | 36.5 vs 47.8 Gbit/s without the loop, CPU 73% | n/m | n/m |

Evidence: I E6; P 3.16, 3.19; Q 3.8; R 3.12, 3.13. The growth with set size is consistent with `nft` re-reading the
whole set before every batch (inference in P 3.16; per-batch `VmHWM` growth 78, 151, 266, 382, 498 MB in P 3.19
supports it). The kernel side was not measured separately. Reproduced as a shape on 4 hosts (W, P, C, T), class U; the
absolute numbers are host-bound. On nft 1.0.9 the userspace cost is higher than on 1.1.x for the same set (interval
batch of 100 000: 151 MB against 78 MB, 1.3 to 1.5 s against about 0.4 to 0.6 s; plain 44 MB against 38 to 39 MB, 0.46 to
0.50 s against 0.22 to 0.36 s; Ty 3.11, `p11.log`); at the v1 ceiling of 10 000 elements the cost is small
(interval load 0.10 to 0.16 s, 19 MB, `p11.log`).

Per-packet cost of a non-matching set lookup: veth 64 B UDP receive rate about 8% lower with 100 000 interval
elements than with an empty table (Q 3.8). On the real NIC, flood drops at prerouting: about 937 000 packets per
second with no set rule, 830 130 with 100 000 plain (about 11% fewer), 699 598 with 100 000 interval (about 25%
fewer), one receive core saturated in every variant (Q 3.8). Single host (S).

### 2.9 Hook placement

All "published port" rows are against Docker's real iptables-nft rules unless a backend is named.

| Finding | W | P | Q | C | T | Reproduced | Class | Evidence |
|---|---|---|---|---|---|---|---|---|
| `forward` sees routed traffic in both directions; `input` sees only traffic for the host; the sender's `output` sees only its own | y | y | - | y | y | 4 of 5 (hosts 4) | U | I E3; P 3.3; R 3.2; Ty 3.2 |
| A drop in `input`, `forward` or `prerouting` (-300 and -310) stops the flow in veth topologies; a TCP flow is cut mid-stream when the element is added (server received 5 messages) | y | y | - | y | y | 4 of 5 | U | I E5; P 3.5; R 3.4; Ty 3.2, 3.14 |
| An `input` drop does **not** stop a Docker published port: the packet is DNATed in prerouting and goes through `forward`, `input` counter 0 | - | y (replica, real default namespace, real NIC from another machine) | y (both Docker backends) | - | - | 2 of 5 (hosts 1) | S | P 3.15, 3.17; Q 3.2 |
| `forward` (-10, 0, +10) and `prerouting` (-310 to -50) drops stop the published port; after removal it connects again | - | y | y | - | - | 2 of 5 (hosts 1) | S | P 3.15, 3.17; Q 3.2 |
| `prerouting` below -100 sees the host address and published port (before DNAT); a chain after -100 sees the container address | - | y | - | - | - | 1 of 5 | S | P 3.15 |
| Local closed port: both `input` 0 and `prerouting` -310 stop it; `prerouting` counters rise for local and forwarded traffic alike | - | y | - | - | - | 1 of 5 | S | P 3.15 |
| IPv6 published port, Docker defaults (`ip6tables` effective): `input` does not block, `forward` and `prerouting` do. With `"ip6tables": false` only `docker-proxy` serves it: `input` and `prerouting` block, `forward` does **not** | - | - | y | - | - | 1 of 5 | S | Q 3.6 |
| After `nftables.service` flushed Docker's tables the published port still worked through `docker-proxy` (input path) | - | - | y | - | - | 1 of 5 | S (default `userland-proxy`) | Q 3.1 |
| A bridged flow (Linux bridge, `br_netfilter` not loaded) is seen by `bridge` family hooks and **not** by `inet` hooks; the bridge's own address is seen by `inet input` | - | - | y | - | - | 1 of 5 | S | Q 3.5 |
| `eth0` in and `wlan0` out (ipvlan children, DNAT and SNAT): prerouting and forward drops each stop the flow; delivery at the far end was not verified | - | - | y | - | - | 1 of 5 | S | Q 3.5 |
| Physical NIC: a drop at `input` (UDP, 50 pps) and at `prerouting -310` stopped the flow and it resumed on removal | - | y | - | - | - | 1 of 5 | S | P 3.17 |

### 2.10 Priorities, ordering and chain creation order

| Finding | W | P | Q | C | T | Reproduced | Class | Evidence |
|---|---|---|---|---|---|---|---|---|
| A drop in our table wins against an `accept` in any other base chain on the same hook, at any priority (-10 and +10 tested); the inverse also holds: another table's drop overrides our accept | y | y | y (ufw, firewalld, Docker, both backends) | y | y (also another table's drop at -320 beats our exclusion accept) | 5 of 5 (hosts 4) | U | I E4; P 3.4, 3.15; Q 3.1, 3.2; R 3.3; Ty 3.2, 3.14 |
| Equal priority: the chain **registered later runs first** (A prio 0 created first, B prio 0 created second: B ran first). Against Docker: our chain at -300 counts the packet when loaded after Docker's, counts 0 when loaded before | y | y | y | y | y (p02 at priority 0; p23 G5, G6 at -310) | 5 of 5 (hosts 4) | U | I E4; P 3.15; Q 3.2; R 3.3; Ty 3.2, 3.14 |
| Docker and firewalld restarts do not change that order; creating our chain first and Docker's afterwards flips it (our counter stays 0) in both Docker backends | - | - | y | - | - | 1 of 5 | S | Q 3.2 |
| Priorities in use: Docker (both backends) raw -300, dstnat -100, filter 0, srcnat 100; firewalld filter+10 (10), mangle+10 (-140), dstnat+10 (-90), srcnat+10 (110); ufw base chains at 0 on `INPUT`, `FORWARD`, `OUTPUT` (policy drop on the first two); -310, -340 and -350 collided with none | - | y (Docker) | y (all three) | y (ufw, Docker listed) | y (a ban service's `inet input` chain at **-1**; ufw and Docker at 0 and below; none at -310 among 15 base chains) | 4 of 5 (hosts 3) | H (per manager) | P 3.1, 3.15; Q 3.1, 3.2; R 3.1; Ty 2, 3.14 |

### 2.11 Coexistence with ufw, firewalld, `nftables.service`, Docker

| Finding | W | P | Q | C | T | Reproduced | Class | Evidence |
|---|---|---|---|---|---|---|---|---|
| ufw (iptables-nft): `ufw reload`, `disable`, `enable`, `restart ufw`, `reset` left our table and element in place; ufw lives in `ip filter`/`ip6 filter`, no new table | - | - | y (Debian ufw 0.36.2, defaults only) | - | - | 1 of 5 | S | Q 3.1 |
| firewalld (nftables backend, new table `inet firewalld`): `--reload`, `--complete-reload`, `systemctl reload/restart/stop/start` left our table alone | - | - | y (2.3.1, defaults only) | - | - | 1 of 5 | S | Q 3.1 |
| Docker daemon restart and `docker restart <container>` left our table alone and kept counting | - | - | y | - | - | 1 of 5 | S | Q 3.2 |
| `nftables.service` with Debian's `/etc/nftables.conf` (`flush ruleset` first; `ExecStop=nft flush ruleset`): a real `systemctl start nftables` deleted every table: Docker's five, ufw's chains and our probe table; a namespace replay of `nft -f /etc/nftables.conf` and of `flush ruleset` did the same | - | y (replay; unit not enabled) | y (real start) | - | - | 2 of 5 (hosts 1) | S, **H**: depends on the unit being enabled and on the conf | P 3.21; Q 3.1 |
| Ubuntu's own ufw defaults, `IPV6=no`, firewalld with the iptables backend, enabling `nftables.service` at boot | - | - | - | ufw active but never exercised | - | 0 of 5 | not demonstrated | Q 3.1; R 2, 3.16 |

### 2.12 Exclusions and what nftables cannot enforce

| Finding | W | P | Q | C | T | Reproduced | Class | Evidence |
|---|---|---|---|---|---|---|---|---|
| nftables does not know management paths: a plain set takes loopback, link-local, hub or peer addresses; an interval set takes `0.0.0.0/0` and `::/0`; overlap is refused only against elements of the same set | y | y | y | y | y | 5 of 5 (hosts 4) | U | I E11; P 3.11; Q 3.3, 3.6; R 3.10; Ty 3.7 |
| Within **one** chain `ip saddr @excl accept` before `ip saddr @blocked drop` delivers (3/3); the reverse order drops (100% loss); an accept in a **separate earlier base chain** does not protect | y | y | y | y | y | 5 of 5 | U | I E11; P 3.11; R 3.10; Ty 3.7, 3.14 (F3) |
| ufw and firewalld deny/reject rules are not rescued by an accept of ours (at input -10 and -100) | - | - | y | - | - | 1 of 5 | S | Q 3.1 |

### 2.13 Established flows, conntrack, flowtables

All from `q07_flow_conntrack.sh` (TCP line stream at 20 lines per second through a router namespace, nft 1.1.3,
kernel 6.18.60, software flowtable on veth). Single host (S) except the first row.

| Finding | W | P | Q | C | T | Reproduced | Evidence |
|---|---|---|---|---|---|---|---|
| A drop with no earlier `ct established accept` in the same chain cuts an established TCP flow at once | y | y | y (S2) | y | y | 5 of 5 | I E5; P 3.5; Q 3.7; R 3.4; Ty 3.14 (H1) |
| With `ct state established accept` **before** the drop in the same chain the established flow is not interrupted; `conntrack -D -s <source>` makes the next packet new and the drop applies; after unblocking, a retransmission burst (79 lines) | - | - | y (S1) | - | **y at input 0 and at prerouting -150; n at prerouting -310** (H2 to H4) | 1 of 5 for Q's conntrack-delete result; T adds the ordering rows of 2.17 | Q 3.7; Ty 3.14 |
| A drop in another table never exempts or rescues (see 2.10) | y | y | y | y | y | 5 of 5 | 2.10; Ty 3.14 (G7) |
| An offloaded flow (`flow add @ft`, `conntrack -L` shows `[OFFLOAD]`) keeps passing after a drop at `forward` **and** at `prerouting -300`; flow offload runs at netdev ingress before prerouting; `conntrack -D` ends it and removes it from the flowtable; a second `conntrack -D` removes 0 entries | - | - | y (S4 to S6) | - | - | 1 of 5 | Q 3.7 |
| Hardware offload (`flags offload`), flowtables loaded by a distribution firewall | - | - | - | - | - | 0 of 5 | not demonstrated |

### 2.14 Topology, tunnels, IPv6 families

| Finding | W | P | Q | C | T | Reproduced | Class | Evidence |
|---|---|---|---|---|---|---|---|---|
| In an `inet` table an IPv6 element blocks only IPv6, an IPv4 element only IPv4; an `ip6` table sees only IPv6, `ip` only IPv4 | - | - | y | - | - | 1 of 5 | S (namespace) | Q 3.6 |
| WireGuard (namespaces): an element holding the **inner** (tunnel) address, rule ungated or gated `iifname wg0`, drops tunnel traffic and the session stays up (handshakes keep refreshing); an element holding the **outer** address on the underlay drops the encrypted packets and the tunnel goes silent; an inner-address rule fed the outer address does nothing | - | - | y | - | - | 1 of 5 | S | Q 3.5 |
| `fe80::/10` at `input` drops a ping to a link-local address and leaves global traffic and neighbour discovery working in that topology; `::1` in the set drops the host's own ping to `::1` | - | - | y | - | - | 1 of 5 | S | Q 3.6 |
| IPv4-mapped sources arriving on the wire | - | - | - | - | - | 0 of 5 | not demonstrated (not constructible with the tools used) | Q 3.6 |

### 2.15 Real-NIC flood cost and SSH impact

| Finding | Value | Class | Evidence |
|---|---|---|---|
| Arrivals dropped at prerouting, no set rule | about 937 790 and 928 866 pps (two runs), whole-system CPU 26.7 and 25.0% of 4 CPUs (one core saturated) | S | Q 3.8 |
| With 100 000 plain / 100 000 interval elements | 830 130 / 699 598 pps | S | Q 3.8 |
| New SSH connections from another machine during the flood | timed out for about a minute; existing sessions not measured; recovered by itself; no firewall change was involved | S | Q 2, 3.8 |
| Real-NIC probe traffic at 50 pps (UDP) and 2 connections per second (TCP) | no impact observed | S | P 3.17 |
| iperf3 over a physical NIC | not run (no peer had it) | not demonstrated | Q 3.8 |

Implication, not measured further: dropping early is cheap per packet, but a source that floods can starve the host's
other work (including a management path) even while it is blocked.

### 2.16 Replace leak analysis on kernel 6.8 (T only)

Source: Ty 3.9, 3.10; raw logs `host-t2/out/p09.log`, `p21.log` to `p28.log`. Method: two UDP senders over a veth into a
namespace whose chain counts arrivals, drops (set match) and packets that no element matched (a counter after the drop
rules, "passed"); a libnftables replacer loops one `delete` + `add` batch of the target element; 2 or 3 runs of 4 to 5 s
per row, `nice 19`, host at load 17 to 25 on 16 CPUs. "Ratio" is leaked packets per replacement, summed over runs
(synthesis calculation; the sums were recomputed from the logs and match the report). Rows with ratio 0 are weak
evidence (4 s runs).

| Case (all atomic batches unless stated; interval = `flags interval,timeout`) | Leaked / replacements | Ratio |
|---|---|---|
| Plain hash set, one element (`p21`) | 0 / 64 623 | 0 |
| Plain hash set + 10 000 elements (`p21`) | 0 / 66 976 | 0 |
| Plain hash, an unrelated element added and deleted in the same set (`p25`) | 0 / 192 041 (with the two interval cases below) | 0 |
| Interval, lone /32, no pause (`p09`, `p21`, `p22`) | 4 711 / 101 159; 2 540 / 54 718; 4 023 / 82 157 | 0.046 to 0.049 |
| Interval, lone /24 target (`p22`) | 4 238 / 80 378 | 0.053 |
| Interval, lone /32 with a pause between replaces of 1 / 10 / 100 ms (`p21`) | 1 033 / 5 703; 241 / 815; 20 / 88 | 0.18; 0.30; 0.23 |
| Interval, 1 neighbour **below** the target (`p22`) | 0 / 84 026 | 0 |
| Interval, 1 neighbour **above** the target (`p22`) | 2 879 / 81 960 | 0.035 |
| Interval, one neighbour on each side (`p22`) | 1 587 / 79 071 | 0.020 |
| Interval, 5, 10, 20, 50 or 200 neighbours on both sides (`p22`) | 0 / 75 409, 70 719, 63 338, 45 905, 20 404 | 0 |
| Interval, 1 000 or 10 000 elements all below (`p21`, `p09`) | 0 / 2 990; 0 / 184; 0 / 338 | 0 |
| Interval, static element and an unrelated element above or below touched (`p25`) | 0 (as above) | 0 |
| **Two-set move** (add to the other set + delete here, one batch): two interval sets / two **plain hash** sets / two interval sets + 1 neighbour above (`p24`) | 2 119 / 51 745; 2 841 / 69 600; 1 826 / 49 384 | 0.041; 0.041; 0.037 |
| **The layout of 5.1** (prerouting -310, `size 1000`, default timeout 30s, `excl4` loaded, counter first): lone element / 1 above / 1 below / 10 each side (`p27`) | 2 881 / 66 393; 2 434 / 66 964; 0 / 63 878; 0 / 57 224 | 0.043; 0.036; 0; 0 |
| **Sentinel element** in the layout: 0.0.0.1 only (below) / 255.255.255.254 only (above) / both ends / 0.0.0.1 with a /24 target (`p28`) | 0 / 64 578; 1 975 / 62 283; 1 109 / 62 564; 0 / 62 164 | 0; 0.032; 0.018; 0 |

What the table supports (fact, then label):

1. The leak needs delete + add of the **same element** in an **interval** set on this pairing; a plain hash set did not leak
   in about 131 000 batches, and touching an unrelated element did not leak in about 192 000. **Demonstrated on T.**
2. Whether an interval replace leaks depends on the **other elements of the set**, which a helper cannot control: none, or
   only elements above the target, leaked; elements only below did not; 5 or more on both sides did not. The report notes
   an earlier two-run execution of `p22` with 175 leaked packets in one run at 5 neighbours (log overwritten), so the
   boundary is soft. **Demonstrated on T; mechanism not known.**
3. The tight-loop exposure is about 0.1 microsecond per replacement (0.047 packets at about 450 000 pps; synthesis
   calculation). **With a pause between replaces the ratio is 4 to 6 times higher** (0.18 to 0.30 per replacement at 1 to
   10 ms; ratios from `p21.log`), which is closer to what a helper that replaces occasionally would see. So 0.05 is a
   lower figure, not the expected per-confirm exposure. The report's own sentence about 0.1 microsecond describes the
   tight loop only.
4. The layout recommended in 5.1 leaks with a lone element or an element with neighbours only above it. A sentinel below
   every target stayed clean, a sentinel above or at both ends leaked: a position-dependent observation, **not a
   mechanism and not a rule** (hypothesis, unproven).
5. The two-set move in **one batch** is not a fix: it leaked as much on plain sets. **Demonstrated on T, one batch form
   only.** The two-set idea in two separate transactions is untested (idea 6.16).
6. **Kernel versus userspace is not separated.** The batch is one transaction with one `DELSETELEM` and one `NEWSETELEM`
   message (`p26.log`: types 2574 and 2572 inside batch begin and end). W, P, Q and C leaked 0 with nft 1.1.x on kernels
   6.18 and 7.0; Q's nft 1.0.9 ran only against 6.18.60 and only for the semantics matrix, never under load. For the
   related `add` no-op (2.2) the same nft 1.0.9 refreshed on 6.18.60, which suggests the kernel, but that is a different
   operation. Cheapest separation: section 10.
7. The leak is a violation of RW-13's "never absent" for the pairing, and it exists in the recommended layout. It is
   small per replacement and it is not covered by any low-rate probe (the 100 pps ICMP probe expects about 0.01 packet).

### 2.17 Bootstrap, add and conntrack-ordering facts (T only)

Source: Ty 3.5, 3.11, 3.14; `p23.log`, `p26.log`. All **S**: one host, kernel 6.8, nft 1.0.9.

| Finding | Evidence |
|---|---|
| The bootstrap batch of 5.2 (including `priority -310`, five rules) loads on a fresh namespace, exit 0, and `nft -c` of it exit 0 | `p23` A |
| **Re-running the whole bootstrap over the live table exits 0 and adds the five rules a second time** (the chain's rule count goes to 10 plus the `policy accept` line, 11 matches in the log); identical `add table`, `add set`, `add chain` alone are no-ops | `p23` B |
| `add set` over an existing set of the same type and flags with a different `size` or default `timeout` exits 0 and **silently changes them** (JSON read back: `size 77`, `timeout 60`); a different type or flags gives `Could not process rule: File exists` | `p23` B3, `p26` |
| `add chain` over an existing chain with a different priority or hook fails with `Chain of type "filter" is not supported, perhaps kernel support is missing?` (misleading); a different **policy** is accepted and changes the chain (`policy drop` took effect) | `p23` B4, `p26` |
| `flush chain` plus five `add rule` in one batch exits 0 under no traffic; behaviour under traffic not tested | `p23` B5 |
| `create element` succeeds on an absent plain host, an absent interval host, an absent prefix, an absent range and an expired element; on an existing element it fails `File exists` and leaves the remaining time unchanged | `p23` C, `p26` |
| `add element` on an existing element, with 300 s, 20 s, 5 s, 90 s, no timeout or `timeout 0s`, changes nothing, on both set types; on an expired element it re-creates it. (The 5 s and 300 s cases are in `p26`'s JSON read-back, 20 s, 90 s, none and 0 s in `p23`'s listing text, where the remaining time keeps falling from 4m59s) | `p04`, `p23` C, `p26` |
| A `ct state established,related accept` before the drop: **input priority 0 keeps the established flow (24 of 24 bytes), prerouting -310 does not (6 of 24), prerouting -150 does (24 of 24)**; with no ct rule the drop cuts it (6 of 24). Conntrack runs at -200, so the state is unknown at -310 | `p23` H1 to H4 |
| Another table's `accept` at -320, -311, -310 (created before or after ours), -309 or -300 never stops our drop; at equal priority the later-registered chain runs first; another table's **drop** at -320 beats our exclusion accept | `p23` G |
| A forward flow, a local-delivery flow, an exclusion, a 3 s expiry and `flush ruleset` in the namespace behave as designed with the 5.1 layout over a veth topology | `p23` F |
| A ban service's chain at priority -1 on the live host; -310 collided with none of 15 base chains | `default_base_chains_summary.txt` |
| The kernel loaded `nft_ct` (host-wide, refcount 0) the first time a `ct` rule was created in a namespace; not removed | Ty 2 |

## 3. Analysis against the design text

Verdicts: **holds** (evidence supports it as written), **partly**, **fails**, **not tested**. Evidence ids refer to
sections 1.3 and 2 and to the reports. A verdict that holds on the earlier hosts and fails on T is written as such,
not averaged.

### 3.1 `04` section 9.1 and RW-13, statement by statement

| Statement | Verdict | Evidence and reasoning |
|---|---|---|
| Hook: not specified in `04` 9.1; the research brief's reference chain used `input` | `input` alone **fails** for routed traffic and Docker published ports (IPv4 and, depending on Docker's IPv6 mode, IPv6); `prerouting` at a negative priority covered every case tested | 2.9; P 3.15, 3.17; Q 3.2, 3.6. Single host. A bridge-switched flow is covered by neither (Q 3.5) |
| "accept before drop" (exclusions as an accept ahead of the drop) | **holds inside one chain only**; it never protects against another table's drop, and our accept never rescues what ufw, firewalld or Docker drop | 2.12; I E11; P 3.11; Q 3.1 |
| Privileges: "limited to set elements in table `inet domovoi`; no other table touched" | **partly.** No other table was touched in any run (stateless hashes equal). But creating the table, sets, chain and its hook, re-creating them after a flush, and `conntrack -D` are not "set elements". `nft` cannot scope privilege to one table: the limit is enforced by the helper building scripts only from fixed templates | I section 2; P section 2; 2.11, 2.13 |
| Management protection: nftables cannot enforce it; the helper recomputes and refuses overlap, and refuses everything when the set cannot be established | **holds** as a division of labour: nftables accepted every excluded or covering target on all hosts | 2.12; I E11 |
| Expiry: kernel set-element `timeout`, independent of the agent, hub and helper process | **holds** for process death on 4 hosts. **Partly** overall: it depends on the table surviving. `nftables.service` start, reload or stop deletes the table and its elements (kernel timers are then moot), a reboot was never run, and the saved-ruleset restore case is untested | 2.3, 2.11; I E13; P 3.21; Q 3.1; Ty 3.4 |
| Watchdog: the 30 s first element makes the block vanish if the hub never confirms | **holds mechanically** (4 hosts, kernel timers). Two weaknesses: (a) under a flood the confirmation can itself fail, so the block expires during the attack it targets; Q saw new SSH connections time out for about a minute at 0.93 Mpps on a 4-core Pi (2.15). It fails safe for management but is ineffective for containment; (b) no measurement exists of the typical confirmation latency against the 30 s | 2.3, 2.15 |
| Apply step 1: element added with a 30 s timeout | **holds with `create`.** `create` on an absent plain host, interval host, prefix, range or expired element succeeds, and on an existing element fails `File exists` and keeps the remaining time (T, `p23` C). The earlier reason for `create` ("a bare `add` on an existing element refreshes the timeout, so a duplicate dispatch could reset a confirmed 1 h element to 30 s") is **kernel-dependent**: it holds on 6.18 and 7.0 and `add` is a no-op on 6.8 (2.2, class K). `create` is correct on every pair seen; the stated reason applies only where `add` refreshes | 2.2; Ty 3.5 |
| Apply step 2: `replace_ttl` = `delete` + `add` in one `nft -f` batch, never absent | **fails on T for interval sets; holds on W, P, Q and C and on plain hash sets on T.** About 450 000 atomic batches on kernels 6.18 and 7.0 leaked nothing; 101 159 batches on an interval set on kernel 6.8 / nft 1.0.9 leaked 4 711 packets (about 0.05 per replacement at about 450 000 pps, 0.18 to 0.30 with a pause), including the layout of 5.1 (2.4, 2.16). Plain hash sets: 0 in about 131 000 batches on T (and 0 in 704 batches on a real NIC, Q, 6.18). Not shown: conntrack rule, flowtable, offload, multi-queue, more cores, IPv6 under load | 2.4, 2.16 |
| Expired element: the transaction fails, the helper reports `expired`, no silent re-add | **holds** (2.4: failed, element not re-created, 5 of 5). But a replace that lost to a rollback produces the same text (2.6), so `expired` needs a membership read and the journal | 2.4, 2.6 |
| "`add element` on an existing element is not relied on to refresh" | **holds as a design choice, and is now necessary.** What `add` does on an existing element differs by kernel: it refreshes on 6.18 and 7.0 (4 environments) and does nothing on 6.8 (T). A design that relied on either would be wrong on the other. Replace must stay `delete` + `add`, step 1 must stay `create` (2.2) | 2.2 |
| Exact behaviour per supported nft version "is verified in R-2" | **partly, and the verified set is not green.** Core primitives were run on six userspace versions against one kernel (Q 3.3) and on 1.0.9 against kernel 6.8 (T); the batch rollback on three pairs; the atomic replace under load passed on three pairs and **failed for interval sets on the one pre-6.18 pair** (Ubuntu 24.04's stock kernel and nft). No supported pair can be named for the replace property (5.13) | 2.4, 2.16, 5.13 |
| Rollback: "`del element` (idempotent; absent = success)" | **fails as written.** `delete` of an absent element exits 1 on both set types and aborts a batch; `destroy` is idempotent on plain sets only; the idempotent forms are the add+delete batch (with two failure modes) or a membership read with error mapping (2.5) |
| Postcondition: element present with remaining timeout > 0; hub round trip works | **partly.** It proves the set content, not that a rule references the set at a hook that packets reach. Nothing in it detects a deleted or altered chain, a table at another priority, or a flowtable bypass |
| Outcome unknown: read membership through helper `list`; present = applied | **holds** as a mechanism; the read must handle covering prefixes (the target may be covered by another element), and must not parse listings that differ by nft version (2.2) |
| Agent restart / hub crash: kernel timeouts continue | **holds** (2.3) |
| Partial application: expires within 30 s | **holds** (2.3, 2.5). An element created by step 1 inside a batch that failed is not committed (E10) |
| RW-13 "so the block is never absent" | **holds only for plain hash sets and for interval sets on kernels 6.18 and 7.0 with nft 1.1.x, within the 2.4 scope; fails for interval sets on kernel 6.8 with nft 1.0.9** (about 0.05 to 0.3 leaked packets per replacement at about 450 000 pps, 2.16). The design sentence needs a scope or a mitigation (5.1.5, D2, D14, D15) |
| Drills (expiry with agent stopped, excluded address refused, hub unreachable, restart mid-TTL, duplicate dispatch idempotent, `max_active`) | all are testable in namespaces; "duplicate dispatch idempotent" is satisfied by `create` on every pair seen and not by `add` (2.2). A new drill is needed: the replace leak self-test of 7.1 |

### 3.2 RW-37 and `04` section 8.3 limits

| Statement | Verdict | Evidence |
|---|---|---|
| Containment hard maxima (`max_ttl`, `max_active`) enforced by the helper | **holds**, with additions: TTL 0 is silently "default or never" and must be rejected; `max_ttl` must be refused, not clamped, so the caller learns the effective TTL | 2.2 |
| Active-count accounting by counting elements in the nft set | **feasible**, but the count must come from a structured read (JSON) of the set, not from `# count N` (only 1.1.6 prints it) and must include foreign elements | 2.2, 2.7 |
| `[nft] max_elements = 1000` | needs a floor/ceiling check: `size` at or above 2^32 wraps (flagless sets and, on T, `flags timeout` and `flags interval,timeout` too; an interval set reads back 2147483647 for `4294967295`) or fails; and each replace costs about 20 to 50 ms at 10 000 other elements, about 610 ms at 100 000 interval; `nft` 1.0.9 needs about twice the memory and three times the time of 1.1.x for large interval batches | 2.7, 2.8 |
| Exclusions: `[safety]` CIDRs, hub, link peer, loopback, link-local | **holds in kind**, but the computation must cover both families and the mapped form (an IPv4 set stores `::ffff:a.b.c.d` as IPv4), and compare overlap in both directions (an element covering an exclusion is accepted in a separate set or when the exclusion is not in the same set) | 2.1, 2.12; Q 3.6 |
| Attestation canonical forms (`04b`): `ip` is dotted IPv4 or RFC 5952 lowercase IPv6, `cidr` has host bits zeroed, `family` is `inet`/`inet6` | consistent with the recommendation; `04b` has no mapped-form rule, so the helper must canonicalise (and refuse mapped targets) **before** producing or comparing lines |

### 3.3 Threat and failure table

| Scenario | What the evidence shows | Residual risk | Requirement (section 5) |
|---|---|---|---|
| Helper crashes mid-apply | a kernel-rejected batch leaves the old state (E10); a killed applier leaves the element to expire (E8) | outcome unknown unless the journal was written first | write-ahead journal; membership read on restart (5.8) |
| Helper dies after step 1, before confirm | element expires in at most 30 s (2.3) | containment lost with no signal | reconcile reports `NotRun`/`OutcomeUnknown`; hub retries through a new permit |
| Racing replace and rollback | final state absent in 50 of 50 race rounds (P 25, C 15, T 10); replace never resurrects (2.6) | misclassification of the failure | classify by journal, not text (5.7) |
| Set full | `ENFILE`, batch rolled back; replace on a full set works; the add+delete rollback fails when full (2.5, 2.7) | rollback error on a full set | fallback to membership read (5.4) |
| Docker restart | table survives; order unchanged with distinct priorities (Q 3.2) | creation-order flip for any chain at an equal priority | priority outside every manager's (5.1) |
| `nftables.service` start/reload/stop (with `flush ruleset`) | deletes our table and elements (P 3.21, Q 3.1); the kernel timeout does not fire because the elements are gone | block silently disappears while the hub believes it active | detect table loss, report `containment_lost`, re-ensure the table (5.9) |
| Host reboot | **not demonstrated** (0 of 5). nft state is volatile by default; boot-time order of `nftables.service`, Docker and the helper untested | unknown; helper's first apply must create the table | `boot_id` already invalidates permits (`04` 9.1, 9.2); validate (section 10) |
| Saved-ruleset restore | **not demonstrated.** An administrator who saves `nft list ruleset` as the nftables config would capture `inet domovoi` with elements; a reload or boot would restore them outside the permit and `boot_id` | orphan blocks past their permit | orphan sweep at helper start; docs warning (5.9) |
| Existing connections persist | a drop with no `ct established accept` in our chain cuts established flows; an offloaded flow bypasses any drop, at any hook (2.13) | flowtable hosts; hardware offload untested | conntrack deletion on flowtable hosts, report coverage (5.10) |
| Management path accidentally covered | nftables accepts any target (2.12); the in-chain accept helps only against our own drop | another manager's drop; a flood from a blocked source starves the host (2.15) | helper overlap check both ways, both families (5.6) |
| Flood against the two-step confirmation | single saturated receive core made SSH time out for about a minute (2.15) | block expires after 30 s during the attack; fail-safe, ineffective | document; retry policy; consider earlier drop (section 6) |
| IPv6 or IPv4-mapped bypass | families are separate sets in `inet`; the IPv4 set normalises mapped text (2.14) | mapped on the wire untested; NAT64 not examined | two sets, canonicalise, refuse mapped targets (5.6) |
| Wrong hook for published ports | `input` misses them; `forward` misses the IPv6 proxy path in one Docker mode (2.9) | none at `prerouting` in the tested cases | `prerouting` (5.1) |
| Bridge-switched traffic | invisible to `inet` hooks (Q 3.5) | VM and container hosts with a bridged physical NIC | report coverage gap; optional `bridge` table (section 9) |
| Tunnel layer | inner and outer addresses are different elements (Q 3.5) | blocking the inner address leaves the endpoint talking to the socket | target semantics decision (section 9) |
| Duplicate step-1 dispatch | on 6.18 and 7.0 `add` resets the timeout, on 6.8 it does nothing (2.2); `create` fails `File exists` on both | confirmed TTL cut to 30 s on kernels that refresh | `create` for step 1 (5.3) |
| Replace leak on interval sets | 4 711 packets of a blocked source passed in 101 159 replacements on kernel 6.8 / nft 1.0.9, including the layout of 5.1; depends on neighbouring elements (2.16) | a blocked source's packets pass for about 0.1 to 0.7 microsecond per confirm or TTL extension on affected pairs; a flood source could pass a few packets per confirm | plain hash layout or per-platform leak self-test (5.1.5, 5.14, D2, D14, D15) |
| Bootstrap re-run over a live table | duplicates the five rules, silently rewrites set `size` and default `timeout`, misleading error on a chain change (2.17) | duplicated rules double counters and change nothing else; a rewritten `size` or `timeout` silently changes limits | create-if-absent only, no re-add (5.2) |
| Ubuntu 24.04 stock pair (kernel 6.8, nft 1.0.9) | fails the replace property for interval sets (2.16); everything else tested passed (Ty 3) | the most common server pair in the candidate matrix fails the property of the recommended layout | platform tier: containment write group Untested/Blocked there until a mitigation is proven (4.3, 5.13) |
| Clock jump, suspend | **not demonstrated** | unknown | section 10 |

## 4. Conclusions

### 4.1 Supported by the evidence

1. Kernel element timeouts expire independently of the process that set them and of the namespace creator (4 hosts, plain and interval sets, 2.3). They are not independent of the table: deleting the table removes the elements.
2. **The replace property, with its true scope.** One `nft -f` batch of `delete` + `add` was observed **not** to expose an absent window on: interval sets on kernels 6.18.58, 6.18.60 and 7.0.0-43 with nft 1.1.3 and 1.1.6 (about 450 000 batches, 0 leaked packets; plus one real NIC, Q); and plain hash sets on kernel 6.8 with nft 1.0.9 (about 131 000 batches, 0 leaked) and on 6.18 (704 batches on a real NIC, Q). It was observed **to expose** one on interval sets on kernel 6.8 with nft 1.0.9: 4 711 leaked packets in 101 159 batches, about 0.047 per batch at about 450 000 packets per second in a tight loop and 0.18 to 0.30 per batch with a pause between batches, including with the layout of 5.1; whether it leaks depends on the neighbouring elements of the set (2.4, 2.16). The controls leaked about 40% on every host, so the probes can see a window. "The block is never absent" is therefore **not a property of one batch of delete + add in general**; it is a measured property of specific stacks, in a veth/UDP setup without a conntrack rule, flowtable or offload.
3. A kernel-rejected batch is rolled back in full on every host (2.7), so the apply is all or nothing for the failures tested.
4. `delete` of an absent element is an error on both set types, and `destroy` is not an idempotent rollback for interval sets on nft 1.0.9, 1.1.3 and 1.1.6 against kernels 6.8, 6.18 and 7.0 (2.5; the check is in userspace).
5. A one-batch `add` + `delete` is an idempotent rollback on plain and interval sets for present, absent and expired elements and for prefixes (nft 1.0.9 with kernel 6.8, 1.1.3 with 6.18.60, 1.1.6 with 7.0), failing safely when the set is full or a covering element exists (2.5).
6. `prerouting` at a negative priority outside every manager's numbers dropped every flow tested, including Docker published ports in both Docker backends and both IPv6 modes; `input` alone misses published ports and `forward` alone misses one IPv6 path (2.9). All of that is one host.
7. A drop in our table beats an accept elsewhere at any priority, and our accept never rescues another manager's drop (2.10, 5 of 5).
8. nftables enforces no exclusion: the helper must (2.12, 5 of 5).
9. An expired replace fails and is not re-created, but its error text equals the rollback-race text; membership must decide (2.4, 2.6).
10. Existing flows are cut unless an earlier accept in the same chain **at a priority after conntrack** or a flowtable offload applies. A `ct state established,related accept` at prerouting -310 does **not** exempt them: conntrack runs at -200 and the state is unknown at -310 (2.17, T; accept at -150 and at input 0 kept the flow). Flowtable behaviour is one host (2.13).
11. `nftables.service` with a `flush ruleset` configuration deletes our table; ufw and firewalld reloads do not (2.11, one host).
12. What `add element` does on an existing element is **kernel-dependent**: it refreshes on 6.18 and 7.0 and does nothing on 6.8 (2.2). `create` fails `File exists` on both and succeeds on absent plain, interval, prefix, range and expired elements (T), so `create` is the only form whose behaviour did not vary.
13. Re-running the bootstrap over a live table duplicates rules and silently rewrites a set's `size` and default `timeout` (2.17, T).
14. The `size` wrap applies to `flags timeout` and `flags interval,timeout` sets as well, on 6.8 (2.17, T).

### 4.2 Not supported, or open

- Any claim about reboot, boot ordering, suspend or clock jumps.
- Any minimum nft or kernel for any primitive. Only one kernel older than 6.18 was run (6.8, one build), with only nft 1.0.9; the batch rollback on nft older than 1.0.9; intermediate kernels (6.1, 6.12) never.
- **Whether the interval replace leak comes from the kernel (6.8's set implementation), the userspace (nft 1.0.9 or its libnftables), or only the pairing** (2.16 point 6). A mechanism for the neighbour dependence is not known.
- Whether the leak occurs on a real NIC, with multi-queue receive, with conntrack in the path, with IPv6, or at much lower rates (the 100 pps ICMP probes are blind to it).
- Whether a sentinel element, a different set type or the two-set move in two transactions removes the leak; only the one-batch two-set move and a few sentinel positions were tried (2.16).
- Packet-path behaviour of ufw (Ubuntu defaults), firewalld (iptables backend or forwarding zones), Podman, rootless Docker, `br_netfilter`, VLAN, hardware offload, other distributions.
- The `size` wrap for `flags interval,timeout` on kernels other than 6.8 (2.7).
- Delivery at the far end for the two-NIC path; more than one real NIC receive queue; line rate.
- Mapped addresses on the wire; NAT64.
- The saved-ruleset restore and `nftables.service` enabled at boot.
- Raw-netlink `NFT_MSG_DESTROYSETELEM` on an interval set.
- Statistical strength: the original ICMP probes sampled about 100 packets per second; the leak results at scale come from veth UDP on four hosts and one real NIC; T's leak rows are 2 to 3 runs of 4 to 5 s on a host at load 17 to 25.
- Everything about the helper itself (not implemented).

R-2 is open. None of the above is an acceptance.

### 4.3 What the stock pair of Ubuntu 24.04 means, and the minimum nft and kernel statement

- **Fact (T, demonstrated):** Ubuntu 24.04.5 with kernel 6.8.0-149 and nft 1.0.9, the distribution's stock pair, passed every semantic test run (grammar, expiry, rollback batch, `create`, errors, exclusions, ordering) and **failed** the replace property for interval sets, including the layout of 5.1. It passed it for plain hash sets in the runs made.
- **Consequence for the platform tier (recommendation):** on that pair the containment write group is **Untested/Blocked** for the interval layout until a mitigation is proven on it (a plain-hash layout that passes the self-test of 5.14, or a different kernel or nft that passes it). It is Untested, not Blocked, for a plain-hash layout, because that result is a few runs of about 4 s on one host (2.16). A claim of support for Ubuntu 24.04 needs the matrix of 7.4 to pass.
- **Minimum nft and kernel (statement of the evidence, not a support claim):** no minimum is established for the replace property. It held for interval sets on {nft 1.1.3, kernel 6.18.60}, {1.1.6, 6.18.58} and {1.1.6, 7.0.0-43} and failed on {1.0.9, 6.8.0-149}. Because the two variables moved together, "nft at least 1.1.3" and "kernel at least 6.18" are each unproven necessary conditions. For `destroy` the keyword needs nft 1.0.9 or later and it is never used (interval absent elements fail on every pair). For the one-batch rollback and for `create` no pair failed (1.0.9 / 6.8, 1.1.3 / 6.18.60, 1.1.6 / 7.0 for the rollback). The practical rule: any pair other than the three that passed must pass the leak self-test (5.14) before the helper allows containment writes.

## 5. Recommendations as implementation requirements

These are proposals for the containment implementer and the helper. Each cites the finding it rests on. Lines that
depend on behaviour no report printed are marked **[validate]**; a test in section 7 covers each. Where T changed a
requirement the line says so; section 11 lists the changes.

### 5.1 Table, chain, set layout

- **R-5.1.1** One table `inet domovoi` (name from the helper's `[nft] table`). The helper never touches another table and never runs `flush ruleset`, `delete table` of a foreign table, or `destroy` (2.5, 2.11).
- **R-5.1.2** One base chain `contain`: `type filter hook prerouting priority -310; policy accept;`. Reasoning: `prerouting` sees local, forwarded and DNATed traffic and stopped every tested flow (2.9); it sits below Docker's raw (-300) and firewalld's mangle (-140), so the match sees the host address before DNAT (P 3.15); -310 collided with no manager seen (2.10). Equal priority is decided by registration order and can flip with start order (2.10), so the number must not equal -300, -200 (conntrack), -100, 0, 100, -140, -90, 10 or 110. The priority is a constant covered by a gate test. -310 is the most tested value; -340 and -350 were also used (P 3.15, 3.17). The exact value is an owner decision (section 9).
- **R-5.1.3** No `input` or `forward` chain in v1 as a second line: no tested flow was caught by them and missed by `prerouting`. Owner decision D1.
- **R-5.1.4** Sets, one pair per address family, never a mixed set (2.14). **Layout depends on D2; the recommended layout (D2-C) is:**
  - `blocked4`: `type ipv4_addr; flags timeout; timeout 30s; size <max_elements>;` (a plain hash set, single hosts only)
  - `blocked6`: `type ipv6_addr; flags timeout; timeout 30s; size <max_elements>;`
  - `blockednet4`, `blockednet6`: `type ipv4_addr` / `ipv6_addr; flags interval,timeout; timeout 30s; size <max_elements>;` for prefix and range targets. **Created and referenced by a rule only on a platform whose leak self-test (5.14) passed for the interval layout**; otherwise absent, and a prefix target is refused with `prefix_unsupported_platform`.
  - `excl4`, `excl6`: `flags interval`, no timeout, written only by the helper (defence in depth, R-5.6.5).
  The earlier layout (one `flags interval,timeout` set per family for hosts and prefixes alike) is the D2-A option; T showed it leaks on kernel 6.8 / nft 1.0.9 (2.16).
- **R-5.1.5** Set flags per target type. **Revised after T.** A plain hash set is the only layout shown free of the replace leak on the pair that leaks (about 131 000 batches, 0 leaked, kernel 6.8 / nft 1.0.9; 704 batches on a real NIC, 6.18) and it is cheaper (64 B versus 128 B per element; `nft` 44 MB versus 151 MB and 0.5 s versus 1.3 s at 100 000 elements on nft 1.0.9, 2.8) but it holds single addresses only. An interval set holds prefixes and ranges, and its replace leaked on 6.8 / 1.0.9 for a lone element and for elements with neighbours only above it, and did not for elements with neighbours only below or on both sides (2.16); that dependence is on set contents that a helper does not control, so a self-test on an empty or fixed set does not certify the live set. Therefore: (a) single-host targets (`/32`, `/128`) go to a plain hash set always; (b) prefix and range targets go to an interval set only where the self-test of 5.14, run on that platform with the interval layout, has passed, and are refused otherwise; (c) a replace on the interval path is never presented as "never absent" in UI text unless the self-test result for that platform says so. Two set types mean two rollback and error paths (a cost, accepted in D2-C). A **sentinel element** below every target is **not** a mitigation: it is a position-dependent observation (clean below, leaking above and at both ends) with no known mechanism. A **two-set move in one batch** is **ineffective** (leaked as much on plain sets). Do not use `auto-merge` (idea 6.5).
- **R-5.1.6** The set default `timeout 30s` is deliberate: an element added without an explicit timeout, or with `timeout 0s`, takes 30 s instead of living forever (2.2). Never create a `timeout` set without a default.
- **R-5.1.7** `size` is a decimal below 2^32 and at most the ceiling in R-5.11.1; config load refuses larger values (2.7). The wrap applies to `flags timeout` and `flags interval,timeout` sets too (T), so the ceiling holds for every set type; 2^31 - 1 is the largest value an interval set stored unchanged on T.
- **R-5.1.8** Chain rules, in this order, in one chain so the in-chain accept is effective (2.12):

```
counter                                  # arrivals: shows the chain is traversed
ip  saddr @excl4 accept
ip6 saddr @excl6 accept
ip  saddr @blocked4 counter drop
ip6 saddr @blocked6 counter drop
ip  saddr @blockednet4 counter drop      # only where the interval path is enabled (D2-C, 5.14)
ip6 saddr @blockednet6 counter drop      # only where the interval path is enabled
```

Add no `ct state` rule to this chain, and do not add one believing it protects established flows: at priority -310 a
`ct state established,related accept` is a **no-op** because conntrack has not run (it runs at -200), and an
established flow was cut with it present (6 of 24 bytes, T `p23` H3). Our drop cutting established flows is the
intended containment behaviour (2.13). **If an owner ever wants established flows exempted**, the protecting accept
needs a chain at a priority **after -200**; -150 worked (24 of 24 bytes, T `p23` H4) and still precedes Docker's
dstnat (-100), but it costs the early drop (conntrack entries are then created for flood packets) and sits next to
firewalld's mangle (-140). That is not recommended and not part of v1; the gate test of 7.1 forbids a `ct` rule in a
chain below -200. Do not put `comment` before a verdict in rule text (a script error seen in Q section 9). Bridge-family
coverage is out of v1 (D8).

### 5.2 Bootstrap and ensure

`ensure_table()` runs at helper start and before every mutating or `list` request. **Revised after T:** the bootstrap is
create-if-absent and is never re-sent over a live table, because on kernel 6.8 / nft 1.0.9 a re-send duplicates the rules
and an `add set` silently rewrites `size` and the default `timeout` (2.17).

```
read  nft -j list table inet domovoi          # JSON: metainfo, table, set x N, chain, rule x N (T `p23` E)
absent (userspace "Error: No such file or directory", exit 1) ->
    send the bootstrap batch below ONCE (it starts with `create table`)
present -> compare with the template: set names, types, flags, size, default timeout,
           chain hook, priority, policy, and the rules in order
equal   -> proceed; send NOTHING (no add table, add set, add chain or add rule)
drift   -> refuse all new containment with `table_drift`; report to the hub; no automatic repair
```

Bootstrap batch, used only when the read says the table is absent. The recommended layout (D2-C, plain hash sets for
hosts; the interval sets and their two rules are appended only where 5.14 passed):

```
create table inet domovoi
add set inet domovoi blocked4 { type ipv4_addr; flags timeout; timeout 30s; size N; }
add set inet domovoi blocked6 { type ipv6_addr; flags timeout; timeout 30s; size N; }
add set inet domovoi excl4 { type ipv4_addr; flags interval; }
add set inet domovoi excl6 { type ipv6_addr; flags interval; }
add chain inet domovoi contain { type filter hook prerouting priority -310; policy accept; }
add rule inet domovoi contain counter
add rule inet domovoi contain ip saddr @excl4 accept
add rule inet domovoi contain ip6 saddr @excl6 accept
add rule inet domovoi contain ip saddr @blocked4 counter drop
add rule inet domovoi contain ip6 saddr @blocked6 counter drop
```

- **Why `create table` first (recommendation, [validate]).** If another actor (a second helper start, a hand edit) created the table between the read and the batch, `create table` fails with `File exists` and the **whole batch is refused**, so no rule is added twice. `add table` would succeed and the five `add rule` lines would then duplicate (T `p23` B: 10 rule lines, exit 0). The report ran only `add`; **`create table` on an existing table, and its refusal of the batch, were not run and need a named test** (7.1). All mutations also hold the helper-wide lock (5.7), so the race needs an outside actor.
- **Never** re-send `add set` over a live set (rewrites `size` and `timeout`, exit 0), `add chain` with another hook or priority (fails with the misleading `Chain of type "filter" is not supported, perhaps kernel support is missing?`; a changed `policy` is accepted and applied) or `add rule` (duplicates). A change of `max_elements` or the default timeout is a **maintenance operation outside `ensure_table()`** (stop containment, remove and re-create the table), not an `add set`; whether that operation exists in v1 is part of D2.
- Drift detection compares the JSON read, not the exit status of an `add`: an `add` accepts a changed set definition silently.
- `flush chain` plus five `add rule` in one batch exits 0 with no traffic (T `p23` B5); its atomicity **under traffic** was not run, so it is not used to repair a chain.
- The listing prints priority -310 as `raw - 10` (nft 1.0.9); `nft -j` prints the number. Compare numbers from JSON.

### 5.3 Step 1: apply (unconfirmed, 30 s)

```
# per family; X = canonical target; set = blocked4 | blocked6 (hosts), blockednet4 | blockednet6 (prefixes, 5.14)
create element inet domovoi <set> { X timeout 30s }
```

- `create`, not `add`: a duplicate dispatch gets `File exists`, maps to `already_present`, changes nothing, and cannot shorten or lengthen a confirmed TTL on any kernel (2.2). **Rationale corrected after T:** the earlier text said a bare `add` "resets a confirmed element to 30 s". That is true on kernels 6.18 and 7.0 and **false on 6.8**, where `add` on an existing element does nothing (class K). So `create` is chosen because its behaviour is the same on every pair seen, not because `add` always refreshes. The helper never relies on either behaviour of `add` on an existing element. `create` success on an absent plain host, interval host, prefix, range and expired element is **demonstrated on one pair** (T, kernel 6.8 / nft 1.0.9); on the other pairs only the `File exists` failure is recorded, so a named test (7.1) runs it on each platform.
- The 30 s comes from a constant, never from a request field.
- The script is fed on stdin (`nft -f -`) or built as an argv vector; never `sh -c`.
- After exit 0, run verification (a), (b), (c) of 5.9 before returning `applied`.

### 5.4 Step 2: confirm and replace (`replace_ttl`)

```
# T = requested TTL seconds, 30 < T <= max_ttl; X spelled exactly as in step 1
delete element inet domovoi <set> { X }
add    element inet domovoi <set> { X timeout <T>s }
```

One `nft -f` batch and nothing else in it (2.4, 2.5). The same template extends a confirmed block's TTL. The delete
must use the stored canonical spelling; a sub-range of a stored prefix counts as absent (R 3.14). After exit 0,
read back and check that the remaining time is in (0, T]. On exit 1 run `classify_absent()` (5.7). Never retry the
batch blindly.

**Scope of the "never absent" property of this batch (revised after T).** It is **packet-atomic for hosts in a plain
hash set** in every run made (T: 0 in about 131 000 batches; Q real NIC: 0 in 704 batches, 6.18) and for interval sets
on kernels 6.18 and 7.0 with nft 1.1.x; it is **not** packet-atomic for interval sets on kernel 6.8 with nft 1.0.9
(4 711 leaked packets in 101 159 batches; the leak depends on neighbouring elements, 2.16). On the interval path
(`blockednet*`) the batch is used only on platforms where the self-test of 5.14 passed. The delete + add form is kept
because no tested alternative is better: the two-set move in one batch leaked as much (2.16), and a make-before-break
form in two transactions is only an idea (6.16). A window, where it exists, is a few packets of an already blocked
source once per confirm or TTL extension; it does not release the block.

### 5.5 Rollback

```
# primary: idempotent for present, absent, expired and prefix targets, plain and interval sets
add    element inet domovoi <set> { X timeout 5s }
delete element inet domovoi <set> { X }
```

Both failure modes leave the set unchanged and X absent but exit 1 (R 3.15, Q 3.4):

| Outcome of the primary batch | Next step |
|---|---|
| exit 0 | `rolled_back` (observers of the table may see an `add` and a `delete` event) |
| `Too many open files in system` (set full, X absent) | read membership: absent gives `absent` (success); present: `delete element X` alone, errors mapped per 5.12 |
| `interval overlaps with an existing one` | X is not an element of its own (a covering or covered element exists): read membership; if no exact X, report `absent_covered` (success) and name the covering element; never delete the covering element |
| `Could not process rule: No such file or directory` (kernel stage) | `absent` (success) after the table check of 5.7 |
| bare `Error: No such file or directory` (userspace stage) | `table_missing`; do not treat as absent |

Never use `destroy` (absent before nft 1.0.9, not idempotent on interval sets, 2.5). Never use read-then-delete as
the primary (220 of 250 raced attempts failed, 2.5). The batch form is now demonstrated on nft 1.0.9 with kernel 6.8,
for present, absent, expired and `/24` targets on both set types (T 3.13); the earlier "[validate] on nft 1.0.x and on
kernels before 6.18" item is closed **for that one pair**. On a full interval set the second error of the failure mode
may cite the `add` line (1.0.9) or the `delete` line (1.1.6), so the mapping of 5.12 must not depend on the line. The
transient `add` inside the batch is invisible to packets on 6.18 (Q 3.4, 1.52 M packets); that was **not** repeated on
6.8.

### 5.6 Exclusion computation

Inputs, all from root-owned state at the moment of apply (RW-37, `04` 9.1): `[safety]` `management_cidrs`; every
`hub_addr`; every kernel-observed `link_peer`; `local_addr` (the host's own addresses on the endpoint interfaces);
the gateway of every default route and of every `endpoint_route`; and the fixed ranges below. If any input cannot be
established, refuse all containment (`exclusions_incomplete`).

Fixed ranges: `127.0.0.0/8`, `::1/128`, `169.254.0.0/16`, `fe80::/10`, `0.0.0.0/8`, `224.0.0.0/4`,
`255.255.255.255/32`, `::/128`, `ff00::/8`. A target in `::ffff:0:0/96` is refused.

```
fn canonical(text) -> Result<Net, Refuse>:
    parse strictly: dotted IPv4 (no leading zeros) or IPv6 text, optional /prefix; no hostnames
    # nft turns a bad literal into a hostname lookup and stores ::ffff:a.b.c.d in an IPv4 set as IPv4 (2.1, 2.14)
    if v6 and (in ::ffff:0:0/96 or in ::/96):  Refuse(mapped_form)   # a mapped TARGET is refused, not converted
    zero host bits; return (family, lo: u128, hi: u128)               # forms as 04b section 2

fn check_target(t, exclusions, policy) -> Result<(), Refuse>:
    if t.prefix < policy.min_prefix(t.family):  Refuse(prefix_too_wide)
    for e in exclusions of t.family:            # mapped exclusion members already canonicalised to IPv4
        if t.lo <= e.hi and e.lo <= t.hi:  Refuse(overlaps_exclusion)    # one test covers both directions
    Ok
```

- Both directions matter: a target inside an exclusion (`/32` in a management `/24`) and a target containing one (`/16` over a management `/24`), I E11. The interval intersection test is symmetric.
- The signed lines of `04b` carry only canonical `ip` and `cidr` forms, so a mapped form is never expressible in a report; canonicalise before producing or comparing lines.
- **R-5.6.5** Also load `excl4` and `excl6` with the same effective set, so a bug that lets an overlapping target in still loses to the in-chain accept for our own drop (2.12). Rewrite them in one batch (`flush set` plus `add element`) whenever the safety identity changes. This does not protect against other tables' drops. The leak of 2.16 was measured for `delete` + `add` of one element in a blocked set, **not** for `flush set` + `add` of an exclusion set, so a one-instant gap in `excl` on an affected pair is possible and untested; a protected address is never in a blocked set, so such a gap does not by itself drop it.
- The recomputation runs immediately before apply under the helper's lock and compares `expected_safety` (`04` 9.1).

### 5.7 Expired versus rolled back; table check; membership read

The error text cannot tell the cases apart (2.6), so `classify_absent(X)` does not parse text:

```
fn classify_absent(X) -> Token:
    if !table_exists():  return table_missing              # `nft -j list table`; containment lost, 5.9
    members = read_members(set)                            # JSON, never listing text
    if exact(members, X):   return present_unexpectedly    # lost a race the other way; re-read once
    if covers(members, X):  return covered_by_other
    j = journal.entries_for(X)
    if j.has_rollback_after(step1):  return rolled_back    # write-ahead record, 5.8
    if now >= j.step1_at + j.ttl_applied - tolerance:  return expired
    return absent_unexplained                               # external deletion; report `containment_lost`
```

`read_members` uses `nft -j list set inet domovoi <set>`; never `# count N`, `size N` or text (2.2). The
JSON element shape is now recorded for nft 1.0.9 on kernel 6.8 (T `p23` E): `elem` objects with `val` as a string, a
`{"prefix":{"addr","len"}}` or a `{"range":[a,b]}` object, `timeout` only when it differs from the set default, and
`expires` in whole seconds. **[validate]** the same shapes on the other versions (the earlier hash-set JSON from 1.0.9
is in `q03_core_ubuntu-24.04.log`). All mutations go through one helper-wide lock (a file lock in the root-owned state
directory) so the journal order equals the kernel order; racing external actors are then the only source of
`absent_unexplained`.

### 5.8 Apply, confirm, rollback, expire: pseudocode

```
apply(req):                         # target X, ttl T, expected_safety, op_id
    lock()
    if gate.state != open:  return containment_blocked(gate.reason)   # leak self-test result for this kernel + nft + layout (5.14)
    check_limits(req)               # 30 < T <= max_ttl (refuse, no clamp); T = 0 refused; prefix floor
    set = route(X)                  # host (/32, /128) -> blocked4|6 ; prefix or range -> blockednet4|6 if gate.interval_path else refuse prefix_unsupported_platform
    ensure_table() or return refuse # read first; create-if-absent; never re-add over a live table (5.2)
    excl = recompute_exclusions() or return exclusions_incomplete
    check_target(X, excl, policy)
    if count_members() + 1 > max_active:  return max_active_reached      # JSON count, foreign elements included
    journal.write(intent: apply, op_id, X, step1_at: now, ttl_step1: 30)  # before the nft call
    r = nft_batch(step1_template)
    if r.ok:                      journal.write(applied); conntrack_cut_if_needed(X); return verify_then(applied)
    if r.token == already_present: journal.write(noop); return already_present
    journal.write(failed, r.token); return r.token

confirm(req):                       # replace_ttl
    lock(); ensure_table()
    if gate.state != open or (route(X) is interval and not gate.interval_path):  return containment_blocked(gate.reason)
    journal.write(intent: confirm, X, ttl: T)
    r = nft_batch(replace_template)
    if r.ok:  journal.write(confirmed, deadline: now + T); return confirmed
    return classify_absent(X)       # expired | rolled_back | table_missing | covered_by_other | absent_unexplained

rollback(req):
    lock(); ensure_table()
    journal.write(intent: rollback, X)        # recorded BEFORE the batch; 5.7 relies on it
    r = nft_batch(rollback_template); apply the table of 5.5; journal.write(result)

expire:                              # nothing runs; the kernel removes the element
    # at helper start and on list: members = read_members()
    #   journaled active X not in members -> classify_absent(X)
    #   member not in journal             -> report `orphan` (still bounded by the set default timeout; counts toward max_active)
```

### 5.9 Table loss, reload protection, verification

- **Table loss.** `nftables.service` start, reload or stop with a `flush ruleset` configuration deletes the table; the elements vanish with it and no kernel timeout fires (2.11). Containment is then absent while the hub believes it active. Requirements: `ensure_table()` before every request; `list` returns a `table_state`; a missing table while the journal has active entries yields `containment_lost` for each, and the helper re-creates **only the empty structure**. Re-applying journaled blocks is decision D5: it needs a fresh exclusion recomputation, the remaining TTL from the journal deadline and the current safety identity; the recommended default is to report and let the hub re-request through a normal permit.
- The loss is fail-open for security and fail-safe for management. Say so in the UI text. Detection delay is bounded by the next request or reconcile tick unless the helper subscribes to `nft monitor` (idea 6.8).
- Ordering: the helper's unit has `After=nftables.service` (a start before it would be flushed). This does not protect against a later reload (idea 6.9).
- Saved-ruleset restore: at helper start read the table; any element without a journal entry is an orphan (reported, bounded by the default timeout). Document that `inet domovoi` must not be saved into a persisted ruleset. **Not demonstrated** (section 10).
- **Leak gate state.** `list` also returns the gate of 5.14 (`open`, `blocked_leak`, `blocked_untested`, with kernel release, nft version and layout), so the hub shows why containment is unavailable on a platform.
- **Verification that the rule is effective**, in order:
  1. element present with remaining time in (0, T], from JSON;
  2. chain `contain` exists with hook prerouting, priority -310 and the rules of the layout in template order (a deleted, reordered or re-hooked chain is caught; "the ruleset contains the rule" is not "a packet saw it", P 3.15 and the brief's evidence rule);
  3. the arrival counter of rule 1 is non-decreasing across reads (a counter that never moves on a busy host means the chain is not traversed); the drop counter delta is reported as evidence when attacker traffic continues, not required for success;
  4. the existing hub round trip.
  The helper cannot send a packet from the blocked source. A veth-based self-test belongs to installation and validation (idea 6.11), not to each apply.

### 5.10 Existing flows, conntrack, flowtables

- Our chain has no `ct` accept, so established non-offloaded flows are cut by the drop (2.13). A host without flowtables needs nothing more.
- Detect flowtables with `nft -j list flowtables` (all tables) and treat `[OFFLOAD]` entries in conntrack as proof. On T the command with no flowtable returns only the metainfo object, exit 0 (`p23` E); with a flowtable it was **not** run (the module was not loaded and none was created). **[validate]** the command and its JSON with a flowtable on each nft version.
- If flowtables exist: after step 1 delete the target's conntrack entries with `conntrack -D -s <X>` (the only form tested, a single source address, Q 3.7) or the netlink equivalent. It needs `CAP_NET_ADMIN` (as nft does) and the `conntrack` package (absent by default on the test hosts) or a ctnetlink call. **[validate]** a prefix argument to `-s`; the reply direction (`--reply-src`) was not tested. Deleting entries makes only the target's next packet "new", so the drop applies; other sources' flows are untouched. It is cheap for one address and unbounded for a wide prefix: cap the work and report.
- Report one of `existing_flows: cut`, `n/a` (no flowtable) or `may_persist` (flowtable present and deletion failed or unsupported). The UI must not say "contained" for `may_persist`.
- Hardware offload is not covered (section 10).

### 5.11 Limits and cost caps

- **R-5.11.1** `[nft] max_elements`: 1 to 10 000 in v1 (default 1 000). About 20 to 50 ms per replace at 10 000 other elements (T: about 48 ms on a loaded host), about 610 ms and a quarter of a core's packet rate at 100 000 interval elements (2.8). On nft 1.0.9 a 100 000-element interval batch needs 151 MB and 1.3 to 1.5 s against 78 MB and about 0.4 s on 1.1.x; at 10 000 elements it is 19 MB and 0.10 to 0.16 s, so the v1 ceiling is not affected, but a raised ceiling would be (Ty 3.11). Config load refuses values at or above 2^32 (2.7). Decision D7.
- **R-5.11.2** `max_active` (default 50) must not exceed `max_elements`. Its accounting is the element count of `blocked4` plus `blocked6` read as JSON, foreign elements included (`04` 8.3). A request over the count is refused before the nft call; `ENFILE` is the second line and maps to `set_full`.
- **R-5.11.3** A prefix floor enforced by the helper (never `/0`; v4 and v6 minimums from the helper config).
- **R-5.11.4** Every `nft` call has a timeout. One replace is a few tens of milliseconds at the cap, so seconds is a symptom and reports `nft_timeout`.

### 5.12 Error-token mapping

Match on (operation, exit status, stage prefix, text), then run the table check and the membership read. Text alone
never decides `expired`. Tokens are stable; the texts are what the logs show. nft 1.0.2 and older differ for intervals. On a full interval set the second error of a rollback may cite the `add` line (nft 1.0.9) or the `delete` line (1.1.6); the mapping must not depend on which.

| Seen text (nft stderr) | Where seen | Token | Notes |
|---|---|---|---|
| `Error: Could not process rule: Too many open files in system` | all hosts, plain and interval | `set_full` | ENFILE, not a descriptor problem; the batch is rolled back |
| `Error: Could not process rule: No such file or directory` on `delete element` | all hosts, plain; kernel stage | `element_absent` then 5.7 | same text for expired and rolled back |
| `Error: element does not exist` | interval, nft 1.0.6 and later; userspace stage; also `destroy` on intervals | `element_absent` then 5.7 | |
| `Error: interval not found in set` (plus kernel ENOENT) | interval, nft 0.9.8 and 1.0.2 | `element_absent` | |
| bare `Error: No such file or directory`, no `Could not process rule` | unknown set or table; userspace stage (I E7, P 3.14) | `table_missing` or `set_missing` | check table existence; never classify as absent |
| `Error: interval overlaps with an existing one` (plus `File exists` on nft 1.0.2 and older) | interval | `overlap` | a covering or covered element exists |
| `Error: Could not process rule: File exists` | `create` on an existing element | `already_present` | |
| `Error: Could not process rule: Invalid argument` | `timeout` into a set without the flag | `table_drift` | template and set mismatch |
| `Error: You must add 'flags interval' to your set declaration if you want to add prefix elements` (or `range_value`) | prefix into a plain set | `table_drift` | |
| `Error: value too large`, `Error: wrong time format`, `Error: syntax error, unexpected ...` | TTL or template errors | `internal_template_error` | cannot occur with fixed templates and rendered integers; a bug if seen |
| `Error: Prefix length N is invalid for type of M bits width` | `/33`, `/129` | `invalid_target` | rejected before nft by `canonical` |
| `Error: Could not resolve hostname: ...` | a bad literal parsed as a hostname | `invalid_target` | rejected before nft; a hostname must never reach nft |
| `Error: Could not process rule: Cannot allocate memory` | `size 4294967295` flagless; also real exhaustion | `resource_exhausted` | |
| `Error: Chain of type "filter" is not supported, perhaps kernel support is missing?` | an `add chain` over an existing chain with a different hook or priority (T `p23` B4, `p26`); also the real unsupported-kernel case | `table_drift` if the table exists on a re-read, otherwise `unsupported_kernel` | the text is misleading; the helper does not send `add chain` over a live table (5.2), so seeing it means a foreign or racing change |
| `Error: Could not process rule: File exists` on `create table` (bootstrap) | another actor created the table first | `table_exists_race` then re-read and compare (5.2) | **[validate]**: `create table` over an existing table was not run |
| `"destroy" command is not supported, perhaps kernel support is missing?`; syntax error on `destroy` (nft 1.0.6 and older) | `destroy` | `internal_template_error` | `destroy` is never used |
| permission or netlink errors such as `Operation not permitted` | **not observed in any log** | `permission_denied` | expected; verify |

### 5.13 Minimum versions

What the evidence supports: the core primitives (`create`, `add`, `delete`, the replace batch, ENFILE, timeout grammar)
behave identically on nft 0.9.8, 1.0.2, 1.0.6, 1.0.9, 1.1.3 and 1.1.6 against kernel 6.18.60 (Q 3.3) **except** that
`add` on an existing element refreshes on 6.18.60 and does nothing on kernel 6.8 (T, nft 1.0.9; 2.2); the rollback
batch on 1.0.9 with kernel 6.8, 1.1.3 with 6.18.60 and 1.1.6 with 7.0 (Ty 3.13, Q 3.4, R 3.15); the atomic replace
under load held for interval sets on {1.1.3, 6.18.60}, {1.1.6, 6.18.58} and {1.1.6, 7.0.0-43} and **failed for interval
sets on {1.0.9, 6.8.0-149}** (hold for plain hash sets there). Kernel and userspace moved together, so neither is a
demonstrated minimum (4.3). Kernels older than 6.8 were not run.

**Minimum statement (evidence only, no support claim).** No minimum nft or kernel is established for the replace
property. `destroy` needs nft 1.0.9 or later and is never used. Any pair that is not one of the three that passed
must pass the leak self-test (5.14) before containment writes are enabled on it.

**Ubuntu 24.04's stock pair (6.8, 1.0.9) is a candidate platform that fails the property for interval sets.** Platform
tier (recommendation): containment write group **Untested/Blocked** on that pair until a mitigation is proven there
(a plain-hash layout passing 5.14, or another kernel such as an HWE kernel, or another nft, passing 5.14 on it; an HWE
kernel and an nft 1.1.x userspace are **untested hypotheses**, not findings).

**Candidate matrix to validate before any claim**, listed from general knowledge and not from the reports: Debian 12
(kernel 6.1, nft 1.0.6), Ubuntu 22.04 (5.15, 1.0.2), Ubuntu 24.04 (6.8, 1.0.9: **measured by T, fails for interval
sets**), Debian 13 (6.12 stock, 1.1.3), RHEL 9 (5.14, nft 1.0.x). Every row runs the leak test of 5.14 and the 7.1
suite. Until the matrix of 7.4 passes, the supported set is empty.

### 5.14 Replace leak self-test (new; recommendation; every detail is [validate])

Why: the replace leak of 2.16 depends on kernel, nft, set type and set contents, and low-rate probes cannot see it.
A helper cannot know from version numbers whether its platform leaks.

- **When.** At install and at helper start when the key (kernel release, nft version string, set layout) differs from the cached result; on operator request; never per apply. The result is stored in root-owned state with its key. Failure to run is not a pass.
- **Where.** A throwaway network namespace (`ip netns` or `unshare --net`) with its own table (`inet domovoi_selftest`), never the production table and never the default namespace; no attachment to a physical NIC or bridge. This needs more privilege than "table `inet domovoi`" (namespace and veth creation) and goes through the process-executor port with argv vectors; the added privilege is part of D14.
- **How.** T's `p27` method: two senders over a veth pair inside the namespace at about 450 000 packets per second or more (loopback may be used if it is shown to reproduce T's leak; T used veth), a counter before the drop, the drop rule on the set under test and a counter after it, and a replacer looping the exact replace the helper uses through the same call path (T used libnftables; the helper's own call path must be the one tested). A split `delete` / `add` control run proves the test can see a window (T's controls leaked 39 to 41%).
- **Scenarios.** For each set type the helper would use: a lone element, one neighbour above, one neighbour below, ten neighbours on each side (T `p27`). Hosts in the plain layout, prefixes in the interval layout.
- **Size.** At least 2 runs of 4 s per scenario and at least 20 000 replacements per scenario (T reached about 5 000 per second; synthesis calculation, so about 30 s in total for 4 scenarios times 2 runs).
- **Verdict.** Pass: 0 leaked packets in every scenario, the control leaked more than 1%, the replacement minimum was reached. Fail: any leaked packet. Inconclusive: control did not leak, minimum not reached, the namespace or rate could not be created. Mapping: plain layout pass gives `open` for hosts; interval layout pass gives `interval_path`; plain fail gives `blocked_leak` for everything; interval fail gives hosts only; inconclusive gives `blocked_untested`.
- **Limits, to be stated in the result and in UI text.** A pass is weak evidence. T's own data show a soft neighbour boundary (one earlier run leaked 175 packets at 5 neighbours), runs of 4 s, and a dependence on set contents that the self-test's synthetic set does not reproduce for the live set. It does not cover a real NIC, multi-queue receive, a conntrack rule, IPv6 or lower rates, and the 0.18 to 0.30 packet figure with pauses (2.16) shows the tight loop understates the per-confirm exposure. So the UI says "validated on this platform at start", never "never absent".
- **Alternatives.** None (rely on the 7.4 matrix only) or advisory (report, do not block): both leave a known leak on a platform in use. See D14 and D15.

## 6. Improvement ideas beyond the evidence

All are **ideas**: no report tested them, except where a row says T tested one form.

| # | Idea | Benefit | Cost | How to test |
|---|---|---|---|---|
| 6.1 | A netlink nf_tables binding instead of the `nft` CLI | no whole-set re-read per batch (about 610 ms at 100 000 interval, 2.8; on nft 1.0.9 also 151 MB and 1.3 to 1.5 s to load 100 000); errno instead of text (5.12); can send `NFT_MSG_DESTROYSETELEM` raw, which may be idempotent in the kernel for intervals (R 3.14). It would **not** be expected to remove the interval replace leak if that is a kernel effect (2.16 point 6, unproven) | a new dependency and large protocol code; kernel drift; AGENTS dependency and process rules apply | raw destroy of an absent interval element on 6.8, 6.18 and 7.0; replace latency at 100 000 elements; the leak test with a raw-netlink replacer |
| 6.2 | Conntrack deletion through ctnetlink | no `conntrack` package; can express prefixes and the reply direction | as 6.1 | delete by prefix on an offloaded flow in a flowtable namespace |
| 6.3 | Per-source `limit rate` or meters as a cheap tier before containment | slows a brute force with no helper round trip | policy surface; not an expiry mechanism | rate test in a namespace |
| 6.4 | A dedicated network namespace for containment | none on the host path (the hook must see host ingress) | cannot see host traffic | test harness only (section 7) |
| 6.5 | `auto-merge` on sets to coalesce adjacent prefixes | fewer elements | merged intervals blur element identity, so exact-spelling delete and rollback break; the merged TTL is unclear | add an adjacent `/25` pair, replace one, read the set |
| 6.6 | Per-element counters (set flag `counter`) | per-block hit count, early release of useless blocks, a real effectiveness signal | extra memory per element; version support | counters readable in JSON after 100 000 elements |
| 6.7 | Two sets: `pending` (30 s) and `confirmed` (TTL); the drop covers both; confirm = add to `confirmed` and delete from `pending` in **one batch** | no delete of the same element, so no interval self-overlap and no `expired` race in the replace | **Tested by T and ineffective:** leaked 0.041 per move on two plain hash sets and on two interval sets (`p24`, 2.16). More lookups; an add to `confirmed` after the 30 s watchdog would recreate containment | do not build; see 6.16 for the two-transaction form |
| 6.8 | `nft monitor` (netlink events) in a resident helper | immediate table-loss detection | requires a resident helper; monitor processes leaked in P section 2 | delete the table, measure the latency |
| 6.9 | `ReloadPropagatedFrom=nftables.service` and `PartOf=nftables.service` on the helper unit | a reload or restart of `nftables.service` triggers re-ensure | unit semantics; untested | `systemctl reload nftables` with the unit enabled |
| 6.10 | `netdev` ingress chain per interface below the flowtable priority | covers offloaded flows, bridge-switched traffic and cheaper floods | per-interface lifecycle; VLAN and IPv6 handling | ordering with a flowtable; bridge namespace test |
| 6.11 | A helper self-test with a temporary namespace and a probe table | proves packet effectiveness at install, and (since T) is the only way to learn whether a platform leaks on replace | creates a namespace and links (inside the namespace only, nothing on a physical NIC); more privilege than the production table; weak evidence when it passes (5.14) | **promoted to R-5.14 and D14**; run on each platform of 7.4 |
| 6.12 | Element `comment` carrying the operation id | reconcile from the kernel without the journal | nft version support; JSON shape unknown | add, list, replace, read back |
| 6.13 | `nft -c` dry run in CI for every template | cheap syntax validation (`-c` matched real adds in I E7) | does not test state | CI job |
| 6.14 | Re-apply journaled blocks after table loss with the remaining TTL | restores containment automatically | acts without a fresh permit and exclusions | only with decision D5 |
| 6.15 | An earlier, cheaper drop (netdev ingress or XDP) for flood sources | keeps the management path alive during a flood (2.15) | complexity; per-interface | repeat the Q flood with the drop at ingress |
| 6.16 | Make-before-break confirm with two sets in **two separate transactions**: (1) add X to `confirmed` with TTL T; (2) delete X from `pending`; the drop covers both | an element is in at least one set at every instant, so no absent window by construction, whichever set type; avoids same-key delete + add on interval sets | two sets and two lookups per family; two commands per confirm (partial failure between them leaves X in both, bounded by the 30 s of `pending`); the flip-between-lookups explanation of T's one-batch result is the report's reasoning, not a measurement | leak test with this form on the 6.8 pair and the 6.18 pair; race test as P 3.18 |

## 7. Test plan

### 7.1 Named tests mapped to findings

`NS` needs root, `ip netns` and real `nft`; `PURE` needs no kernel.

| Test | Finding | Kind |
|---|---|---|
| `replace_never_exposes_an_absent_window` (UDP flood 6 s, assert 0 leaked; the split control must leak, proving sensitivity). Runs per set type and per scenario of 5.14; **expected to fail for the interval layout on kernel 6.8 / nft 1.0.9** (2.16) | 2.4, 2.16 | NS (load) |
| `kernel_timeout_expires_element_after_helper_sigkill` | 2.3 | NS |
| `create_existing_element_reports_already_present_and_keeps_ttl` | 2.2, 5.3 | NS |
| `add_on_existing_element_behaviour_is_recorded_per_kernel` (records whether `add` refreshes, does nothing or shortens; the helper never depends on it; the result is a fixture, not an assertion of either behaviour) | 2.2 (class K) | NS |
| `no_template_uses_add_for_step_one` (gate: step 1 is `create`; `add` on a possibly existing element appears only in the replace and rollback templates) | 2.2, 5.3 | PURE gate |
| `step_one_create_succeeds_on_absent_plain_and_interval_prefix` (also range and expired element; run on every platform of 7.4, demonstrated on 6.8 / 1.0.9 only) | 5.3 | NS |
| `rollback_of_absent_element_succeeds_on_plain_and_interval_sets` (also expired element and exact `/24`) | 2.5 | NS |
| `rollback_on_full_set_falls_back_to_membership_read` | 2.5 | NS |
| `rollback_with_covering_prefix_reports_covered` | 2.5 | NS |
| `replace_of_expired_element_fails_and_does_not_recreate` | 2.4 | NS |
| `replace_losing_to_rollback_is_classified_rolled_back_from_journal` (20-way race, 15 rounds) | 2.6 | NS |
| `bare_no_such_file_is_table_missing_not_expired` | 5.12 | NS |
| `ttl_zero_is_rejected_before_nft`, `ttl_above_max_ttl_is_refused` | 2.2 | PURE |
| `element_without_timeout_takes_set_default` | 2.2, R-5.1.6 | NS |
| `max_elements_at_or_above_two_pow_32_is_rejected_at_config_load` | 2.7 | PURE |
| `max_active_counts_foreign_elements_from_json` | 5.11 | NS |
| `block_applies_to_docker_published_port_before_dnat` (namespace replica from a Docker ruleset fixture) | 2.9 | NS |
| `input_only_chain_misses_published_port` (documents why not `input`) | 2.9 | NS |
| `chain_priority_is_not_a_known_manager_number` | 2.10 | PURE gate |
| `excluded_target_refused_for_overlap_in_both_directions_v4_and_v6` | 2.12 | PURE |
| `mapped_v6_target_refused_and_mapped_exclusion_canonicalised` | 2.14 | PURE |
| `incomplete_exclusion_inputs_refuse_all_containment` | RW-37 | PURE |
| `in_chain_accept_protects_exclusion_from_our_drop` | 2.12 | NS |
| `flush_ruleset_reports_containment_lost_and_recreates_empty_table` | 2.11 | NS |
| `established_flow_is_cut_without_ct_accept` | 2.13 | NS |
| `ct_established_accept_at_minus_310_does_not_exempt_an_established_flow` (documents why no `ct` rule is in the chain) and `ct_established_accept_after_conntrack_priority_exempts_it` (control: -150 keeps the flow) | 2.17 (T `p23` H3, H4) | NS |
| `chain_below_minus_200_has_no_ct_rule` | R-5.1.8 | PURE gate |
| `bootstrap_over_a_live_table_sends_nothing_when_the_json_matches` and `bootstrap_never_sends_add_set_add_chain_or_add_rule_over_a_live_table` | 2.17, 5.2 | PURE gate |
| `bootstrap_rerun_does_not_duplicate_rules` (a second bootstrap batch starting with `create table` is refused with `File exists` and the chain keeps its five rules; needs `create table` on an existing table, never run so far) | 2.17, 5.2 [validate] | NS |
| `ensure_reports_table_drift_when_set_size_or_default_timeout_differs` (JSON compare, because `add set` rewrites them silently) | 2.17 | NS |
| `leak_self_test_control_run_leaks_so_the_test_can_see_a_window`, `leak_self_test_fails_on_any_leaked_packet`, `leak_self_test_inconclusive_blocks_containment`, `leak_self_test_result_is_keyed_by_kernel_nft_and_layout` | 2.16, 5.14 | NS (load) / PURE |
| `interval_layout_replace_leak_matrix_lone_above_below_both_sides` (reproduces the four scenarios of T `p27`; records, does not assert 0) | 2.16 | NS (load) |
| `pairing_6_8_with_1_0_9_runs_the_full_namespace_suite_and_the_leak_test` (platform row of 7.4: expected result today: suite passes, interval leak test fails) | 2.16, 4.3 | NS (load, VM or host) |
| `gate_blocks_apply_and_confirm_when_leak_state_is_not_open` and `prefix_target_is_refused_when_interval_path_is_disabled` | 5.8, 5.14 | PURE |
| `size_wrap_applies_to_interval_timeout_sets_and_config_load_still_refuses_two_pow_32` | 2.7, 2.17 | NS / PURE |
| `offloaded_flow_is_cut_after_conntrack_delete` | 2.13 | NS (needs `nf_flow_table`) |
| `error_token_mapping_matches_recorded_stderr_per_nft_version` | 5.12 | PURE |
| `list_members_parses_json_fixture_per_nft_version` | 5.7 [validate] | PURE |

Gate tests (extend, never weaken): no code path builds a script containing `destroy`, `flush ruleset`, or a table
name other than the configured `inet domovoi`; the script templates are `insta` snapshots (5.2 to 5.5) so a change
is reviewed; `nft` and `conntrack` are spawned only through the process-executor port with an argv vector or a stdin
script, never `sh -c`; the TTL formatter emits only `<n>s` with n >= 1; step 1 uses `create`, and a bare `add` on a
possibly existing element appears only inside the replace and rollback templates; no template contains `add set`,
`add chain` or `add rule` for a table that exists (the bootstrap starts with `create table`); no `ct` rule appears in a
chain whose priority is below -200.

### 7.2 Fixtures

Recorded stderr and JSON from `probes/*/out/` per nft version (0.9.8 to 1.1.6, including `host-t2/out/p23.log` for the 1.0.9 JSON shapes, `create` results and re-add errors, and `p26.log`); Docker's stateless ruleset
(`probes/pi2/out/default_ruleset_stateless_before.txt`) as the replica fixture; the firewalld and ufw rulesets
(`probes/pi2b/out/pi2b_fwd_active_ruleset.txt`, `pi2b_ufw_active_ruleset.txt`) as coexistence fixtures.

### 7.3 CI

`NS` tests need real `nft`, root and `ip netns`. A hosted Linux runner provides root through `sudo`, `ip netns` and
an installable `nftables`; use `sudo ip netns` or `sudo unshare --net`, **not** `unshare --user --map-root-user --net`:
recent Ubuntu restricts unprivileged user namespaces with AppArmor, and the Ubuntu 26.04 host refused it with
`uid_map: Operation not permitted` (Q 8). Older nft userspace runs in containers (Q 3.3) against the runner's kernel,
which varies userspace only. Flowtable and Docker-replica tests need `nf_flow_table` and the runner's module set;
skip with a visible reason if absent. Load tests (the leak self-test scenarios of 5.14) are a separate job, and a hosted runner's kernel may be any version, so the job must print the kernel release and nft version it ran on: it learns about the runner's pair, not about the matrix. Kernel variation needs virtual machines.

### 7.4 Conventional-Linux validation matrix

| Platform | Kernel and nft | Managers | Tests | Why |
|---|---|---|---|---|
| Debian 12 | 6.1, 1.0.6 | none; `nftables.service` enabled | all `NS`, reload, reboot | older kernel and userspace; `destroy` absent |
| Ubuntu 22.04 | 5.15, 1.0.2 | ufw defaults | all `NS`, `ufw reload` | `interval not found in set`, double overlap error |
| Ubuntu 24.04 | 6.8, 1.0.9 | ufw defaults | all `NS`, all of 7.1, the leak test | **measured by T: suite passes, interval-set replace leaks (2.16)**; `destroy` first present; rerun with the helper, with Docker and with the 5.14 plain layout |
| Ubuntu 24.04 with a newer kernel (HWE) | newer kernel, 1.0.9 | ufw defaults | the leak test, 5.14 scenarios | separates kernel from userspace; whether a newer kernel removes the leak (untested hypothesis) |
| Any 6.18 or 7.0 host with nft 1.0.9 userspace | 6.18 or 7.0, 1.0.9 (private extract or container) | none | the leak test under the p27 load | separates kernel from userspace the other way (section 10) |
| Any 6.8 host with nft 1.1.x userspace | 6.8, 1.1.3 or 1.1.6 (private extract or container) | none | the leak test under the p27 load | the other half of the separation (section 10) |
| Debian 13 or Raspberry Pi OS | 6.12 or 6.18, 1.1.3 | none, ufw, firewalld | rerun, with a reboot | the tested pair plus reboot |
| Fedora or RHEL 9 | their kernels | firewalld (nftables backend), forwarding zones | `NS`, `firewall-cmd --reload`, reboot | default zones, `inet firewalld` priorities |
| Any with Docker | any | any | published-port and restart tests, both backends, IPv6 modes | Docker versions other than 29 |
| Podman or rootless Docker | any | any | published-port tests | untested |
| Bridged physical NIC with `br_netfilter` | any | any | bridge coverage | untested |
| Offload-capable NIC | any | any | flowtable `flags offload` | untested |
| Reboot with the helper installed | any | `nftables.service` enabled and disabled | table creation, saved-ruleset restore, orphan sweep | never run |

## 8. Follow-up edits for the owner (list only; nothing is edited here)

| Document | Section | Change |
|---|---|---|
| `04-target-architecture.md` | 9.1, row Privileges | replace "limited to set elements in table `inet domovoi`" with: the helper creates and maintains only table `inet domovoi` (table, sets, one base chain, rules) and deletes conntrack entries of the target; scripts come from fixed templates; `nft` itself cannot scope privilege to one table |
| `04` | 9.1, new row Placement | one base chain at hook `prerouting`, priority outside every manager's numbers (candidate -310), **no `ct` rule** (it is a no-op below -200), per-family sets with default `timeout 30s`: plain hash sets for single hosts, interval sets for prefixes only where the platform's leak self-test passed (D2, 5.14); state that `input` alone and `forward` alone are insufficient (R-5.1) |
| `04` | 9.1, row Management protection | add: both address families, both overlap directions, canonicalisation and refusal of IPv4-mapped targets, gateways and `local_addr` among the inputs; the optional in-chain `excl` sets protect only against our own drop |
| `04` | 9.1, row Expiry | add: independent of processes, **not** of the table; deleting the table (for example `nftables.service` with `flush ruleset`) removes the elements; reboot not demonstrated |
| `04` | 9.1, row Watchdog | add: the 30 s window can lapse during a flood that also delays the hub's confirmation (fails safe, ineffective); no latency measurement exists |
| `04` | 9.1, row Apply semantics | step 1 is `create element ... timeout 30s` (not `add`); fix the missing full stop before "Each request carries `expected_safety`"; replace "Exact behaviour per supported nftables version is verified in R-2" with the version matrix status; add that an `expired` result is decided by membership and journal, not by text; **replace "never absent" with the scoped statement of 4.1 item 2** (plain hash sets and the tested 6.18 and 7.0 pairs; the interval replace leaked on kernel 6.8 with nft 1.0.9) and add the leak self-test of 5.14 as an install-time check |
| `04` | 9.1, row Rollback | replace "`del element` (idempotent; absent = success)" with the one-batch `add` + `delete` form, its two failure modes and the membership fallback; state that `destroy` is not used |
| `04` | 9.1, row Postcondition | extend with chain presence, template match and counters (5.9) |
| `04` | 9.1, new rows | Existing flows (conntrack and flowtable handling, reported coverage); Table loss (`containment_lost`, ensure before every request); Limits (`max_elements` range, `size` below 2^32, replace cost); **Platform gate** (leak self-test state per kernel, nft and layout; Ubuntu 24.04's stock pair is Untested/Blocked for the interval layout); **Bootstrap** (create-if-absent, never re-send `add` over a live table) |
| `04` | 8.3 config example | `[nft] max_elements`: document 1 to 10 000 and the refusal at or above 2^32; `max_active` not above `max_elements` |
| `03-architecture-decisions.md` | RW-13, Mechanism | add the placement (prerouting, not `input`), `create` for step 1, the rollback form, and that table loss releases containment; **scope the sentence "so the block is never absent" to the stacks and set types where it was measured (4.1 item 2)**; keep "production readiness depends on R-2" |
| `03` | RW-37, ownership table | `max_active` accounting: element count read as JSON including foreign elements; containment limits include the `max_elements` range |
| `03` | RW-37, exclusions | state the families, the symmetric overlap test, the fixed ranges, gateways and `local_addr`, and the mapped-form rule (consistent with `04b` canonical forms) |
| `99-open-decisions.md` | R-2 row | add pointers to this synthesis and to reports I, P, Q, R and W (`W-host-t-r2-nft109-kernel68.md`); list the open items of section 10 (including the replace leak on kernel 6.8 and its kernel-versus-userspace separation); keep "not production-ready until verified" |
| `99` | D-22 | "Firewall uses a kernel timeout" gains: the timeout is lost with the table; the helper detects and reports the loss |
| the implementation index (`07-*`) | item 46 | note the placement decision, the new reported states (`containment_lost`, `existing_flows`, the leak gate states `blocked_leak` and `blocked_untested`) and the leak self-test as inputs to the item |

## 9. Open owner decisions

| # | Decision | Options and cost | Recommendation |
|---|---|---|---|
| D1 | Hook placement | (A) `prerouting` only, one chain: covers every tested flow, one chain traversal per packet (two set lookups per family under D2-C). (B) `prerouting` plus `input` and `forward`: redundancy no test needed, two more lookups. (C) `input` plus `forward`: misses published IPv6 in one Docker mode and DNAT-before-match semantics | **A** |
| D2 | Set layout (**changed after T**) | (A) one interval set per family for hosts and prefixes: one code path, but the replace leaked on kernel 6.8 / nft 1.0.9 including this layout (2.16), and the interval cost (about 25% fewer packets per second at 100 000 elements, double memory). (B) plain hash sets for single hosts only, prefix targets refused in v1: the only layout shown leak-free on the leaking pair, cheapest, one set type, but no prefix blocking (product decision: is a prefix target required in v1?). (C) plain hash sets for hosts plus interval sets for prefixes, the interval path enabled only on platforms whose leak self-test (5.14) passed: keeps prefixes, two set types and two error paths, a gate to maintain. The in-place change of `size` or default timeout (silent `add set`) is not part of the layout decision: it is a maintenance operation or not offered (5.2) | **C**; **B** if prefix targets are not required in v1. **A** only after the matrix of 7.4 shows no leak on every supported platform |
| D3 | Support statement | (A) declare nothing until the 7.4 matrix passes; Ubuntu 24.04's stock pair is listed as Untested/Blocked for containment writes until a mitigation passes there. (B) declare the tested pairs (6.18 with 1.1.3 and 1.1.6, 7.0 with 1.1.6) as validated for the interval layout, the 6.8 / 1.0.9 pair as failing for it | **A**; B only as "validated on" notes with the failing pair named beside them |
| D4 | Existing flows | (A) conntrack delete only when flowtables exist, plus reporting. (B) always delete. (C) report only | **A** |
| D5 | Table loss | (A) re-create the empty table and report `containment_lost`; the hub re-requests. (B) also re-apply the journal. (C) report only | **A** |
| D6 | `nft` CLI or netlink in v1 | (A) CLI with a small `max_elements`. (B) netlink binding: removes the re-read cost and text parsing, adds code and a dependency | **A** for v1, idea 6.1 later |
| D7 | `max_elements` ceiling | 1 000 default; hard ceiling 1 000, 10 000 or higher (nft 1.0.9 needs 151 MB and 1.3 s for 100 000 interval elements; 10 000 is 19 MB and 0.1 to 0.16 s) | default 1 000, hard 10 000 |
| D8 | Bridge-family coverage | (A) out of v1, detect a bridged physical NIC and report a coverage gap. (B) a `bridge` table on such hosts | **A** |
| D9 | Helper unit ordering and reload propagation | `After=nftables.service` only; add `ReloadPropagatedFrom` (6.9); do nothing | `After=` plus detection; test 6.9 |
| D10 | Reboot semantics | containment does not survive a reboot (permits already invalid by `boot_id`) | confirm, after the reboot test |
| D11 | Tunnel targets | the unfiltered `prerouting` rule drops whichever address the element holds (inner or outer, Q 3.5); decide which address a detector reports and whether the UI says which layer | document the address as observed by the detector |
| D12 | Priority value | -310, -340, -350 or another number outside every manager's; a value above -200 would let a `ct` accept exempt established flows (T: -150 worked) at the cost of the early drop and is not recommended (R-5.1.8) | -310 |
| D13 | Orphan elements | report only (they still expire) or remove after review | report only |
| D14 | Replace leak self-test (**new**) | (A) required: run at install and on a change of kernel, nft or layout (5.14); a failure or an inconclusive run blocks the affected path; adds the privilege to create a namespace and links, and about 30 s of CPU. (B) advisory: run, report, never block. (C) none: rely on the 7.4 matrix only, which cannot cover every kernel an operator runs | **A**, with the added privilege recorded in the helper's privilege statement |
| D15 | What to do on platforms where the interval replace leaks (**new**) | (A) disable the interval path there: single hosts only (plain hash), prefix targets refused with a clear token and UI text. (B) allow it with the exposure stated (about 0.05 to 0.3 leaked packets per replacement at about 450 000 pps, 2.16) and report `replace_window_possible` on each confirm. (C) avoid same-key replace on interval sets with the make-before-break form (idea 6.16), only after it is tested. (D) block containment entirely on the platform | **A**; B only by an explicit owner acceptance of the exposure; C is not available until idea 6.16 is tested; D if the plain layout also fails the self-test |

## 10. What remains unproven, and the cheapest proof

| Item | Cheapest way to prove it |
|---|---|
| Reboot, boot ordering of `nftables.service`, Docker and the helper; saved-ruleset restore | a disposable VM with the helper installed: reboot with `nftables.service` enabled and disabled, with and without a saved ruleset containing the table |
| Kernels other than 6.8 below 6.18 (5.10, 5.15, 6.1, 6.12); nft older than 1.0.9 for the batch rollback | VM images of Debian 12 and Ubuntu 22.04 and Debian 13: run the Q, R and T scripts (`host-t2/` is the newest set, `p21` to `p28` are the leak probes) and the 7.1 tests |
| **Kernel versus userspace for the interval replace leak** (2.16 point 6) | (a) nft 1.0.9 userspace against a kernel that did not leak: on an existing 6.18 or 7.0 host run `p27` with a private copy of the Ubuntu 24.04 `nftables` and `libnftables1` packages **extracted, not installed** (the replacer calls libnftables, so point the library path at the extracted 1.0.9 copy); Q already ran 1.0.9 as a container userspace on the Pi's 6.18.60 kernel (Q 3.3), so the mechanism exists there, only the load test is new. (b) nft 1.1.x userspace against kernel 6.8: on a 6.8 machine (T if the owner allows, otherwise a cloud-image VM of Ubuntu 24.04) run the same with the Ubuntu 26.04 or Debian 13 packages extracted into a private directory, as environment W did for 1.1.6. The two together separate the variables; either alone halves the question. Also an Ubuntu 24.04 HWE kernel with the stock nft (VM) |
| `size` wrap on `flags interval,timeout` sets on kernels 6.18 and 7.0 | repeat `p23` section D (nine set definitions and the fill test) on a 6.18 and a 7.0 host; **T already shows the wrap for all three flag sets on 6.8** (2.7) |
| `create` success path, add shortening a timeout, `add set` and `add chain` re-adds, `flush chain` + `add rule` atomicity under traffic, on 6.18 and 7.0 | rerun `p23` C and B and `p26` on the 6.18 and 7.0 hosts (T ran them on 6.8 only, 2.17); plus `flush chain` + `add rule` under the p27 load |
| `create table` over an existing table: refusal of the whole batch (5.2) | a ten-line namespace script: bootstrap, then the bootstrap again starting with `create table`; read the rule count |
| Leak under conntrack in the path, IPv6, lower and higher rates, multi-queue, a real NIC; the dependence on pauses between replaces | repeat `p27` with a `ct state` rule at a later hook, an IPv6 element, rate steps (50 000, 200 000, 450 000 pps) and pauses of 0, 1, 10 ms; a real NIC needs a second host |
| Sentinel elements, plain hash sets and the two-transaction two-set form (6.16) as mitigations | `p28`-style scenarios with the new forms on the 6.8 pair and a 6.18 pair, 3 runs of at least 10 s |
| Whether the transient add of the rollback batch is invisible to packets on 6.8 | repeat Q's 1.52 M packet test (Q 3.4) on the 6.8 pair |
| Raw-netlink `destroy` on an interval set | a small netlink client in a namespace |
| `br_netfilter`, bridge-switched coverage | namespace with `modprobe br_netfilter` in a VM (not on a Docker host: it changes Docker's bridges) |
| Hardware offload, flowtables loaded by a distribution firewall | a host with an offload NIC |
| `conntrack -D` with a prefix and the reply direction; `nft -j list flowtables` | extend the Q flowtable namespace |
| Podman, rootless Docker, other Docker versions | a VM per variant |
| Other distributions' defaults (Ubuntu ufw, firewalld forwarding zones, RHEL) | VMs per 7.4 |
| Two-NIC delivery at the far end | add a receiver on the second network and read its counters |
| More LAN throughput peers (iperf3 over a physical NIC) | install `iperf3` on a second peer |
| IPv4-mapped sources on the wire; NAT64 | a raw-socket sender in a namespace |
| Expiry precision at the boundary; clock jump and suspend | sample at 100 ms around the deadline; step the clock in a VM |
| Multi-queue and line rate | a packet generator on a multi-queue NIC |
| Flood against the confirmation round trip | repeat the Q flood with a real hub round trip measuring the 30 s window |
| Helper implementation, journal reconcile, exclusion recomputation | out of scope of the research; the 7.1 tests |

R-2 stays open. This document records no owner decision and makes no supported-platform claim.

## 11. Revision log

This revision adds the fifth report (Ty, `W-host-t-r2-nft109-kernel68.md`, Ubuntu 24.04.5, kernel 6.8.0-149, nft 1.0.9,
namespaces only) and revises the document as a whole. What changed and why:

| Where | Change | Why |
|---|---|---|
| Title, scope, 1.1 | five reports, four hosts, five environments; citation `Ty` (the report's letter W collides with environment W); a note that the Ty sums were recomputed from the raw logs | new report |
| 1.2 | row T added; independence caveat now "N of 5", four distinct pairs, single-host findings of T listed; notation gains class **K** (pairing-dependent) and a sentence that "universal" means universal across the pairs run | T is the first pre-6.18 kernel; P and Q remain one host |
| 1.3 | #1 and #5 updated (`destroy` on a third kernel; `size` read back for all flag sets on T only, not reconciled elsewhere), #8 rewritten (add refresh is kernel-dependent; Q's own 1.0.9 refreshed on 6.18.60), #10 extended, #11 (the replace claim) and #12 (closed `[validate]` items) added | T contradicts or closes earlier statements |
| 2.1 to 2.15 | column T added to every finding table; counts changed from "N of 4" to "N of 5 (hosts k)"; classes revised: add-refresh U to K, replace atomicity U to K, `size` wrap method note, memory (slab U, `nft` RSS H), shortening and `create` success now demonstrated on T only (S); the `ct` row split by hook priority | evidence from T |
| 2.4 | T rows added; the paragraph restated: the property is pairing-dependent; the earlier "about 450 000 batches, 0 leaked" kept but scoped to W, P, Q and C; which runs were interval and which plain; low-rate ICMP results declared blind | T leaked 4 711 of 101 159 on an interval set |
| 2.5 | 1.0.9 / 6.8 added to `delete`, `destroy` and rollback rows; counts updated | T 3.5, 3.12, 3.13 |
| 2.7, 2.8 | `size` row resolved for T only; memory and time on nft 1.0.9 added | T 3.11 |
| 2.16, 2.17 (new) | leak analysis table with recomputed sums; bootstrap, add, `ct` ordering and priority facts | T 3.10, 3.14 |
| 3.1 to 3.3 | verdicts for apply step 1, step 2, the `add` row, the version row, RW-13 and the drills rewritten; four threat rows added or changed (replace leak, bootstrap re-run, Ubuntu 24.04 pair, duplicate dispatch) | T |
| 4 | 4.1 item 2 restated with its true scope; items 10, 12 to 14 added; 4.2 lists the unseparated kernel/userspace question; 4.3 added (Ubuntu 24.04's stock pair, platform tier, minimum nft/kernel statement) | T |
| 5.1.4, 5.1.5, 5.1.7, 5.1.8 | layout revised (plain hash for hosts, interval for prefixes behind a self-test), sentinel and one-batch two-set listed as unproven or ineffective, `ct` ordering and the priority a protecting accept needs | T 3.10, 3.11, 3.14 |
| 5.2 | bootstrap is create-if-absent with an exact safe form (`create table` first, never re-send `add`), drift compared from JSON | T 3.14 |
| 5.3, 5.4, 5.5 | `create` rationale corrected; scope of the "never absent" batch stated; rollback `[validate]` closed for one pair | T 3.5, 3.13 |
| 5.6.5, 5.7, 5.8, 5.9, 5.10, 5.11, 5.12 | notes on the exclusion rewrite, JSON shapes, gate in the pseudocode, gate state in `list`, `nft -j list flowtables` with none, memory and time cost, two new error rows | T |
| 5.13, 5.14 (new) | minimum statement, Ubuntu 24.04 platform tier, replace leak self-test | T |
| 6 | 6.1 and 6.7 revised, 6.11 promoted, 6.16 added | T 3.10 |
| 7 | named tests for the leak self-test, bootstrap idempotency, `add` behaviour, `ct` ordering, the 6.8 pairing, gate and size wrap; gate additions; matrix rows for Ubuntu 24.04 and for separating kernel from userspace | T |
| 8, 9 | edit-list rows updated; D1, D2, D3, D7, D12 revised; D14 and D15 added | T |
| 10 | kernel-versus-userspace separation with cheap routes, further rows | T |

Checked against the raw logs and found consistent: the leaked-packet and replacement sums of Ty 3.9 and 3.10
(`p09`, `p21`, `p22`, `p24`, `p25`, `p27`, `p28`), the `add` no-op (`p04`, `p23` C, `p26`), the bootstrap re-run and the
`add set` / `add chain` re-adds (`p23` B, `p26`), the `ct` ordering rows (`p23` H), the `size` read-back (`p23` D) and
the memory figures (`p11`). Not verified against raw logs, or inconsistent in the report:

- The earlier two-run `p22` execution with 175 leaked packets at 5 neighbours, and the broken lone /24 scenario of that
  execution: the log was overwritten (Ty 3.10, appendix note).
- The "0 of 155 M" and "0 of 91 M" packet totals of Ty section 5 row 1 for P and R were not used; this document uses its
  own recount of 2.4.
- Ty 3.5 attributes the add/shorten matrix (20 s, 90 s, no timeout, `0s`) to `p26`'s JSON read-back; `p26` holds only the
  300 s and 5 s cases, the others are in `p23` C as listing text (remaining time keeps falling from 4m59s). The result is
  the same.
- Ty 2 says the ruleset hash changed twice (during `p09` and `p11`). The logs show it moving between three values and
  returning: `p09` 47b560e6 to bdd1370d, `p10`, `p15` and `p16` still report 47b560e6, `p11` bdd1370d to ab90ca97,
  `p21` and `p22` report bdd1370d and `p12`, `p23` to `p28` report ab90ca97. This fits churn in the ban service's own table
  as the report says, but the "changed twice" wording is not what the logs show and the per-table baseline is missing.
- `p28` "sentinel 0.0.0.1 below" counts `arrived` 5 packets above `sent` in two runs (stray packets); immaterial to a 0
  leak result.
- `p23` B prints 11 matching lines after the re-run; that is 10 rules plus the `policy accept` line, as Ty 3.14 says "10
  rule lines".
- `size 4294967296` is reported as "stores 0 (no limit)" from a listing that prints no size; 0 is inferred, not read.
- The report cannot say whether the Q 1.0.9 container build equals T's distribution build; the "points at the kernel"
  inference for `add` rests on that.
