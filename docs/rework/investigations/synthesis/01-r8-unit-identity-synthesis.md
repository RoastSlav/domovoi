# R-8 synthesis: effective systemd unit identity (four hosts)

Scope: research item R-8, the identity of a systemd unit that RW-34 binds into a plan (`config_hash`) and that
the M8 `unit.restart` contract and the H1 helper `systemd.write` group will act on. This document consolidates the
four research reports, analyses them against the design text, draws conclusions and turns them into
requirements, ideas and tests for the implementer. It adds no new measurements; it re-reads some raw outputs of the
fourth report (Section 1.3). It does not resolve or accept R-8:
**R-8 stays open**, `unit.restart` stays gated, and no supported-distribution or supported-version claim is made.
The revision that added the fourth host is listed in Section 12.

Nothing here edits a design document. Section 8 lists the edits for the owner to make.

Two things were not available and are not invented: the design text for the M8 `policy-permit-autonomy` work was not
present beyond its entry in the implementation index (milestone gate "R-8 accepted; H1 complete"), so Section 5 is
built only from RW-34, RW-23, RW-08, `04` section 8.3 and the roadmap M8 entry.

## 1. Scope and sources

### 1.1 Citation scheme

Every claim cites a report and a section of it: `K` is `K-r8-systemd-unit-identity.md`, `O` is
`O-pi-r8-systemd257.md`, `T` is `T-host-c-r8-ubuntu-systemd259.md`, `V` is `V-host-t-r8-ubuntu2404-systemd255.md`.
`K U5` means K section 3, experiment U5; `O §6` means O section 6; `T §7 M13` means T section 7, matrix row M13;
`V U22` is V section 4, experiment U22; `V §8 D1` is V section 8, item D1. Raw outputs are under
`docs/rework/investigations/probes/r8` (K), `pi8` (O), `host-c8` (T) and `host-t8` (V), in `out/`. Label meanings
follow the reports: **demonstrated** (observed, with output), **not demonstrated**, **requiring conventional Linux
validation**. "Synthesis calculation" marks a number or inference computed here from the reports' counts, not
reported by them. `[raw]` after a V citation means the synthesis author re-read the cited raw output of V and it
matches the report (Section 1.3).

Design text read: RW-34, RW-23 and RW-08 in `03-architecture-decisions.md` (and the helper group table near its end),
`04-target-architecture.md` section 8.3, `06-implementation-roadmap.md` M8, `99-open-decisions.md` R-8, and the
implementation index.

### 1.2 The four hosts

| Dimension | K (WSL2) | O (Pi) | T (host-c) | V (Ubuntu 24.04) |
|---|---|---|---|---|
| systemd | 259.5 (`259.5-0ubuntu3.4`) | 257.13 (`257.13-1~deb13u1`) | 259.9 (`259.9-0ubuntu0.1`) | 255 (`255.4-1ubuntu8.18`) |
| Distribution | Ubuntu on WSL2 | Debian 13 | Ubuntu 26.04.1 LTS | Ubuntu 24.04.5 LTS |
| Architecture | x86_64 (WSL2 VM) | aarch64, real hardware | x86_64, real hardware | x86_64, ext4 on NVMe |
| Kernel | 6.18.58.1-microsoft-standard-WSL2 | 6.18.60+rpt-rpi-2712 | 7.0.0-43-generic | 6.8.0-149-generic |
| `/usr` layout | merged (`/lib -> usr/lib`) | merged | merged | merged |
| `/run` mount | executable | **noexec** | executable | **noexec** |
| Reload time (`daemon-reload`) | 130 to 150 ms | 520 to 824 ms | 282 to 303 ms (3 samples), 634 to 689 ms (5 samples, busier) | about 0.57 to 0.73 s (V §1, §4 U18 E3); journal lines in V's raw outputs span 472 to 712 ms (synthesis calculation) |
| `show` of 3 properties / `restart` | about 21 ms read, 9 ms restart | 9 to 54 / 13 to 47 ms | 11 to 27 / 14 to 18 ms | 9 to 19 / 21 to 37 ms |
| Scopes exercised | system | system, user (own user and a disposable user) | system, user (disposable user, partial) | system, user (disposable user: rank, restart, mask, visibility) |
| Security modules | not exercised | SELinux/AppArmor compiled in, none exercised | AppArmor loaded (188 profiles), probe processes unconfined | AppArmor loaded (149 profiles), probe processes unconfined; SELinux compiled in, not exercised |
| Workload / sharing | idle | shared with two other sessions; 2043 reload requests by the probes | live deployment, Docker, a second session; 277 reloads (capped) | live deployment, 27 containers, a second session issuing its own reloads; 203 reload requests by V (cap about 200, V §2) |
| Race sample | 8 rows x 120 | 12 rows x 100 x 3 runs = 3600 | 10 rows x 30 x 3 runs = 900 (830 clean) | 5 rows x 30 x 3 runs = 450 at 50 ms only (421 clean restarts, synthesis calculation from `u17_summary.txt`) |
| Incident | `/dev/null` clobbered, repaired (K §2) | none | none | none (V §2) |
| Source | K §1 | O §1 | T §1 | V §1 |

Constraints shared by all four:

- Every host is a merged-`/usr` systemd 255 to 259 host reached with root. Nothing older than 255 or newer than 259.9,
  258 untested, no non-merged `/usr`, no RHEL family, SUSE or Arch, no container or non-systemd
  host, no SELinux-confined helper (K §8, O §9, T §9, V §11).
- **Location confound.** Nearly every probe unit lived in `/run/systemd/system`, which RW-34 does not allow as a
  fragment location (K §1, O §1). Results on units in allowed locations: `/etc` against `/run` precedence (K U4),
  `/etc` masks (K U6), `/usr/lib` fragment restart-without-reload and package drop-ins (O, T and V U13 R5, R7, R9),
  drop-in directories created in `/etc` and `/usr/lib` after load (V U22 N4, N5), real packaged units read only
  (K U9, O R9, T R9, V U13). Everything else is shown on `/run` units and extended to allowed locations by the
  (documented, shared) systemd loading code, not by direct test.
- The race harness is synthetic: one editor that rewrites a drop-in and runs `daemon-reload` repeatedly (K U11, O §6,
  T §6, V §6).
- Three of the four hosts are Ubuntu, three are x86_64 (V §11).

### 1.3 What was checked against raw outputs

The synthesis author re-read these raw outputs of V (`probes/host-t8/out/`) and found the report's statements
correct: `u22.out` (cases N1 to N6, including the restart warning in every case and `NeedDaemonReload` after the
restart); `u10b_final.txt` (every matrix cell, `FOREIGN=0` on all rows); `u10b_sorted_rows.out` and `u10c.out`
(`Requires` returned in two orders around a reload; `d_dbus_s` same on M10 and M16); `u20.out` (variants A, B, C: values,
`CapEff`, the two `Permission denied` reads, 18 against 13 lines of `systemctl cat`, restart worked in B and C);
`u21.out` (D1 `GetUnit` failure and `LoadUnit` success, D2 counts, D3 timestamps unchanged although the script runs
`daemon-reload` between the reads, D4 no `--json`, D5 counts, D5b socket facts, D6, D7); `u17_summary.txt` (counts and
intervals); `u10_determinism.reloads.txt` (journal wording `Reloading requested`); `u6_u9.out` (`cat` of a linked unit
prints `# <link>` only, mode-000 drop-in value ran); `u18.out` E4 (property order); `u14_run.out` and `u23.out` (`/run`
`noexec`); `u13.out` (real masks). The wording of the other hosts' journal line (`Reload requested`) was confirmed in
the raw journal files of the earlier hosts.

Not confirmed, or found to differ in wording, and treated accordingly:

- V U13 says the real masks `alsa-utils.service` and `cryptdisks-early.service` are `/usr/lib/systemd/system/<unit> ->
  /dev/null` "checked with `ls -l`". `u13.out` shows `LoadState=masked`, `UnitFileState=masked`,
  `FragmentPath=/usr/lib/systemd/system/<unit>`, the `cat` line "is masked" and the absence of an `/etc` entry; no `ls -l`
  of the `/usr/lib` link is in the output. The `/dev/null` target is therefore not demonstrated by the raw file; the
  rest is.
- V §1 and §5 give `daemon-reload` as 574 to 733 ms; the journal lines in the raw outputs span 472 to 712 ms.
- V §2 counts 203 reloads against a cap of about 200 (disclosed by V); the total was not recomputed here.
- The output header of `u20` variant C prints `ProtectHome=yes`, the command line in `u20_helper_directives.sh` passes
  `ProtectHome=read-only` as V §8 says; only the label is wrong. The D7 header says "PartOf and Requires"; the unit
  written by the script has `PartOf=` and `After=`, as V §8 says.
- V §8 does not capture the exit status of `systemctl cat` under the restricted unit (V says so).
- V says "2 of about 14 reload rows" changed an unsorted D-Bus hash; the raw matrix shows `d_dbus` changed on M10 and M16
  where nothing real changed, and a later sorted pass shows M10 unchanged, consistent with an unstable order; the
  denominator 14 is not derivable and is not used here.
- V calls T's `u18` E6 flawed. T's raw `u18.out` E6 shows a script executed directly from `/run` (`exec-from-run-ok`),
  which is valid evidence that `/run` is executable on T; the part of E6 that started a unit that was already active
  proves nothing. T's generator in `/run` ran (E37), which does not depend on E6.
- V's statements about the other hosts' `show` order and `cat` text were taken from the earlier reports' tables, not
  compared line by line with their raw outputs. M14 `NeedDaemonReload` `no` on O and T was confirmed in the O and T
  matrix tables; K has no value for it.

## 2. Consolidated findings

Columns K, O, T, V: `Y` the result was observed as described, `N` observed differently (explained in the notes),
`-` not tested on that host. "Reproduced" is `k of n`, where n counts only the hosts that tested the row; the hosts
that did not are named, so `4 of 4` and `3 of 3 (K -)` are both full agreement among testers, while `3 of 4 (V N)`
is a difference. A row seen on one host only reads `1 of 1 (K, O, T -)` and is **not** evidence about the others.

### 2.1 Every experiment and mutation row

| Id | Finding | K | O | T | V | Reproduced | Evidence |
|---|---|---|---|---|---|---|---|
| E01 | Plain unit: `FragmentPath` is the file, `DropInPaths=` empty string (not absent), `NeedDaemonReload=no` | Y | Y | Y | Y | 4 of 4 | K U2, O U2, T U2, V U2 [raw] |
| E02 | `DropInPaths` is in application order: by file name (basename) across directories, not by path; later wins; renaming flips the effective value | Y | Y | Y | Y | 4 of 4 | K U3, O U3, T U3 and U7, V U3 |
| E03 | Empty assignment resets; same-name drop-in in `/etc` replaces the `/run` one (only the `/etc` path listed) | Y | Y | Y | Y | 4 of 4 | K U3, O U3, T U3, V U3 |
| E04 | `/etc` fragment beats `/run` fragment; `/run` drop-ins still apply on top | Y | Y | Y | Y | 4 of 4 | K U4, O U4, T U4, V U4 |
| E05 | Rank `/etc` > `/run` > `/usr/local/lib/systemd/system` > `/usr/lib`; `/usr/local/lib` is a search directory | - | Y | Y | Y | 3 of 3 (K -) | O R1 to R5, T R1 to R5, V U13 |
| E06 | Package drop-ins in `/usr/lib/.../*.d` are listed; a same-name `/etc` drop-in replaces the `/usr/lib` one; list ordered by basename across `/etc`, `/run`, `/usr/lib` | - | Y | Y | Y | 3 of 3 (K -) | O R5, R9; T R5, R9; V U13 |
| E07 | Fragment edited, no reload: `NeedDaemonReload=yes`, `restart` prints a stderr warning, exits 0, runs the **old loaded** configuration; after reload runs the new one | Y | Y | Y | Y | 4 of 4 | K U5, O U5, T U5, V U5 |
| E08 | Same on an allowed-location fragment (`/usr/lib`) | - | Y | Y | Y | 3 of 3 (K -) | O R7, T R7, V U13 R7 |
| E09 | Drop-in added without reload is ignored by `restart`; drop-in removed without reload still applies and its path stays listed | Y | Y | Y | Y | 4 of 4 | K U5, O U5, T U5, V U5 |
| E10 | `try-restart` behaves like `restart` (warning, no reload) | Y | Y | Y | Y | 4 of 4 | K U5, O U5, T U5, V U5 |
| E11 | Inactive non-failed unit is unloaded, so `start` reads the new disk content; a **failed** unit stays loaded and keeps the old config | Y | Y | Y | Y | 4 of 4 | K U5 and U5b E, O U5, T U5, V U5, U5-B E |
| E12 | `stop` of a stale failed unit also prints the "changed on disk" warning | - | Y | Y | Y | 3 of 3 (K -) | O U5, T U5b, V U5-B E |
| E13 | `NeedDaemonReload` false positive: `touch` of an unchanged file gives `yes`; edit-then-revert gives `yes` | Y | Y | Y | Y | 4 of 4 | K U5-B, M01, M15; O; T; V §7 [raw] |
| E14 | `NeedDaemonReload` false negative: in-place edit with mtime restored gives `no`, restart ran the old config | Y | Y | Y | Y | 4 of 4 | K U5-B A, O U5-B A, T U5-B A, V U5-B A |
| E15 | `NeedDaemonReload` false negative: rename-replace with new content and restored mtime gives `no`; the **restart ran the stale config** | not run | Y | Y | Y | 3 of 3 (K showed the property only) | K U5-B B; O E1; T E1; V U5-B B, U18 E1 |
| E16 | `NeedDaemonReload=yes` for a drop-in removed and for an edited target of a linked unit (4 of 4); for a drop-in **added with an old mtime**: `yes` on K, O, T, **`no` on V** because that drop-in sat in a new directory (see E47) | Y | Y | Y | N for one row | 4 of 4 (removed, linked target); 3 of 4 (V N) (added, old mtime) | K U5-B C, D, F; O; T; V U5-B C, D, F |
| E17 | Sixth `restart` in 10 s is refused (`start-limit-hit`, exit 1) and leaves the unit **failed**, down; the first line of the message differs by version (257 and 259: "... failed because start of the service was attempted too often"; 255: "Job for X failed.") | Y | Y | Y | Y | 4 of 4 | K U5-G, O U5-B G, T U5-B G, V U5-B G [raw] |
| E18 | Alias (`[Install] Alias=`): `Id` is the canonical unit, `Names` lists both, same `FragmentPath`; restart via alias restarts the canonical unit; a drop-in directory named for the alias applies to the canonical unit and is listed | Y | Y | Y | Y | 4 of 4 | K U6, O U6, T U6, V U6 |
| E19 | Alias retargeted: `Id` and `FragmentPath` change, restart via the alias name runs another unit | Y | Y | Y | Y | 4 of 4 | K U6 S5, O U6, T U6, V U6 |
| E20 | Linked unit: `FragmentPath` is the symlink path, `UnitFileState=linked-runtime`; target edits change content read through it | Y | Y | Y | Y | 4 of 4 | K U6, O U6, T U6, V U6 |
| E21 | Masked: `FragmentPath` is the `/etc` symlink (not `/dev/null`), `LoadState=masked`, `cat` prints only "is masked", restart fails "is masked" and an active unit stays up; real masks seen on Pi (`userconfig.service`), host-c (`sleep.target`, `multipathd.socket`) and V (`alsa-utils.service`, `cryptdisks-early.service`, see E54) | Y | Y | Y | Y | 4 of 4 | K U6, O R9, T R9, V U6, U13 |
| E22 | Template instance: `FragmentPath` is the template; `DropInPaths` lists instance and template drop-ins in application order; a never-started instance of an existing template resolves at once (`loaded`, `static`); missing template gives `not-found` with `show` exit 0; bare `foo@.service` makes `show` exit 1 | Y | Y | Y | Y | 4 of 4 | K U7, O U7, T U7, V U7 |
| E23 | Transient unit: has a `FragmentPath` under `/run/systemd/transient`, `UnitFileState=transient`, `Transient=yes`, empty `DropInPaths`; `not-found` after stop | Y | Y | Y | Y | 4 of 4 | K U8, O U8, T U8, V U8 |
| E24 | `/lib/systemd/system` never appears in reported paths (also not for an alias link written with the `/lib` spelling, V); `/lib` and `/usr/lib` spellings are one inode; hashing both spellings gives a different digest; enable links written by packages still spell `/lib` | Y | Y | Y | Y | 4 of 4 (all merged) | K U9, O U9, T U9, V U9 |
| E25 | `systemctl show` of an unknown unit exits 0 with `LoadState=not-found`; unreachable manager exits 1; `timeout` kill 124 | Y | Y | Y | Y | 4 of 4 | K failure 7, O §5, T §5, V §5 |
| E26 | Unreadable drop-in: root reads it; an unprivileged user gets `Permission denied` while `show` still lists it; systemd ran the unit with a mode-000 drop-in | partly (read result only) | Y | Y | Y | 4 of 4 for the read result, 3 of 3 for "systemd ran it" (K partly) | K failure 6, O §5, T §5, V §5 (value `secret` ran) [raw] |
| E27 | Candidate (a) deterministic over repeated reads, reload, `touch`; path is an input; NUL framing separates `AB` from `A`+`B`; path-sorted digest is not precedence order | Y | Y | Y | Y | 4 of 4 | K U10, O U10, T U10, V U10 |
| E28 | Matrix M00 to M18: every candidate cell identical on the four hosts, except M16 candidate (c) (K and T changed, O and V did not, see 2.3); V adds the `NeedDaemonReload` difference on M14 (E48) | Y | Y | Y | Y | 4 of 4 (cells), 1 host-dependent cell, 1 host-dependent reload state | K §7, O §7, T §7, V §7 [raw] |
| E29 | Matrix M19 to M22 (`/usr/lib` drop-in added, dash-prefix drop-in with and without reload, `set-property --runtime`) | - | Y | Y | Y | 3 of 3 (K -), except M20 (E30) | O §7, T §7, V §7 [raw] |
| E30 | M07 and M20 (drop-in added, no reload): (a) over reported paths **misses** them; `a_disk` catches both on O, T and V. M07: `b` and `c` catch it and `NeedDaemonReload` flips to `yes` on all four. M20 (new dash-prefix directory): `b`, `c` and `NeedDaemonReload=yes` on O and T, but **on V `b`, `c` are `same` and `NeedDaemonReload` stays `no`** | Y (M07) | Y | Y | Y (M07), N for b, c and reload state on M20 | M07 4 of 4; M20 a/a_disk 3 of 3 (K -), M20 b, c, reload state 2 of 3 (V N) | K §7, O §7, T §7, V §7 M20 [raw] |
| E31 | M13 and M18: no candidate other than disk-side ones sees them; `NeedDaemonReload` stays `no` | Y | Y | Y | Y | 4 of 4 | K §7, O §7, T §7, V §7 [raw] |
| E32 | M22 `set-property --runtime`: writes `/run/systemd/system.control/<unit>.d/50-*.conf`, applied live, `NeedDaemonReload=no`, listed in `DropInPaths`, changes every candidate | - | Y | Y | Y | 3 of 3 (K -) | O §5 #11, T §5, V U13 R8, §7 |
| E33 | M11 `EnvironmentFile` content and M12 `ExecStart` binary change no candidate | Y | Y | Y | Y | 4 of 4 | K §7, O §7, T §7, V §7 [raw] |
| E34 | Generated unit: `FragmentPath=/run/systemd/generator/<unit>`, `UnitFileState=generated`, `SourcePath` empty (K read only a masked generated unit; T and V read `-.mount` from the fstab generator with `SourcePath=/etc/fstab`) | partly | Y | Y | Y | 3 of 3 full (K partly) | K U8, O U14, T U14 and u19, V U14 |
| E35 | Generator drop-ins (`/run/systemd/generator.early/<unit>.d`, `generator/<unit>.d`) attach to an ordinary on-disk unit, appear in `DropInPaths`, change `Environment`; a path check on `FragmentPath` alone passes it | - | Y | Y | Y | 3 of 3 (K -) | O U14, T U14, V U14 (generator placed in `/etc`) |
| E36 | Changed generator input without reload: `NeedDaemonReload=no`, restart ran the stale value | - | Y | Y | Y | 3 of 3 (K -) | O U14, T U14, V U14 |
| E37 | Generator placed in `/run/systemd/system-generators`: ran on T (executable `/run`); silently skipped on O and V (`noexec` `/run`; the `/etc` generator ran on both); on V a unit with `ExecStart=/run/x.sh` fails "Permission denied" while the same script in `/var/tmp` runs | - | N (skipped) | Y (ran) | N (skipped) | run 1 of 1 (T), skip 2 of 2 (O, V) | O U14, T U14, V U14, U23 [raw] |
| E38 | Dash-prefix drop-in directory (`name-.service.d`) applies to every unit with that prefix and is invisible to a `<unit>.d` enumeration | - | Y | Y | Y | 3 of 3 (K -) | O §5 #12, T R6, V U13 R6 |
| E39 | `systemctl show -p DropInPaths` and `--value` quote and C-escape awkward names (space, tab, newline, quote, backslash, non-ASCII) inside one line; plain names unquoted; whitespace split breaks them | not demonstrated | Y | Y | Y | 3 of 3 (K -) | O U15, T U15, V U15; raw `probes/pi8/out/u15.out` |
| E40 | `busctl get-property ... DropInPaths` and `busctl --json=short` return an unambiguous string array; `FragmentPath` exact | - | Y | Y | Y | 3 of 3 (K -) | O U15, T U15, V U15 |
| E41 | `systemctl show --json` does not exist (`unrecognized option`); on V `systemctl` has no `--json` at all, `list-units` included | - | Y | Y | Y | 3 of 3 (K not tested) | O U15, T U15 and E4, V U15, §8 D4 [raw] |
| E42 | User scope: same rank, restart-uses-loaded-config, warning naming `--user daemon-reload`, mask in home; system and user managers do not see each other's units; on V a unit in `/etc/systemd/user` is reported as `/etc/xdg/systemd/user/...` | - | Y (full) | partial (rank, restart, mask, visibility) | partial (same rows) | 3 of 3 on repeated rows | O U16, T U16, V U16 |
| E43 | Race at 50 ms gap, unguarded: 22/120, 53/300, 1/60, 1/80 | Y | Y | Y | Y | rates differ by host and cadence | K U11, O §6, T §6, V §6 |
| E44 | Post-restart check reading `/proc/<MainPID>/environ`: flagged all wrong starts, 0 false alarms (18 of 18, 2 of 2, 1 of 1 = 21 of 21); loaded-property re-read after restart: 120, 9 and 2 false alarms | - | Y | Y | Y | 3 of 3 (K -) | O §6, T §6, V §6 [raw] |
| E45 | Failure scenarios 1 to 9 (fragment edit, drop-in added or removed, mask, alias retarget, unreadable, show failure, template instance, transient); scenario 2 differs on V for the `NeedDaemonReload` signal (E47) | Y | Y | Y | Y | 4 of 4 | K §3, O §5, T §5, V §5 |
| E46 | Failure scenarios 10 to 14 (generator drop-in, `set-property`, dash-prefix, `/usr/local/lib`, awkward names) | - | Y | Y | Y | 3 of 3 (K -) | O §5, T §5, V §5 |
| E47 | **New drop-in directory not noticed on V**: a drop-in in a directory that did not exist at load leaves `NeedDaemonReload=no` and `systemctl cat` without warning, in `/run` (N2), in `/etc/systemd/system` (N4) and in `/usr/lib/systemd/system` (N5); unchanged after 5 s and with the unit stopped (N6); a file added to an existing or empty existing directory is noticed (N1, N3); `restart` printed the warning in all five cases, ran the old configuration, and `NeedDaemonReload` read `yes` only after that restart. Cause not isolated | - | - | - | Y | 1 of 1 (K, O, T not run for these cases; O and T read `yes` on M20, a new directory, so 255 differs from 257 and 259.9) | V U22, §7 M20 [raw] |
| E48 | `NeedDaemonReload` after `mask` of an active unit (M14): `yes` on V, `no` on O and T (K shows no value) | - | N | N | Y | 3 of 3 differ (V yes; O, T no) | V §7, O §7, T §7 [raw] |
| E49 | Set-valued D-Bus properties come back in no fixed order across reloads: `Requires` was `["system.slice","sysinit.target"]` before and `["sysinit.target","system.slice"]` after a reload; an unsorted D-Bus hash (`d_dbus`) changed on M10 and M16 with no real change; with `Requires`, `BindsTo`, `PartOf`, `Wants` sorted (`d_dbus_s`) both rows were `same`. `d_dbus` is stable across restart and reload otherwise and ignores M11, M12 | - | - | - | Y | 1 of 1; `d_dbus_s` run for 2 rows only | V §7, u10b_sorted_rows.out, u10c.out [raw] |
| E50 | `GetUnit` of an inactive unit fails ("not loaded") although `systemctl show` had just printed `LoadState=loaded`; `LoadUnit` works and returns the properties. Consistent with E11 (inactive non-failed units are unloaded); mechanism not isolated | - | - | - | Y | 1 of 1 | V §8 D1 [raw] |
| E51 | `GetAll` works on the Unit interface (99 properties) and the Service interface (334); every property of Section 5.2 is present | - | - | - | Y | 1 of 1 | V §8 D2 [raw] |
| E52 | `UnitsLoadStartTimestampMonotonic` and `UnitsLoadFinishTimestampMonotonic` are identical before and after a `daemon-reload` (the script reloads between the two reads) | - | - | - | Y | 1 of 1 | V §8 D3 [raw] |
| E53 | Under `CapabilityBoundingSet=CAP_NET_ADMIN ProtectSystem=strict PrivateDevices=yes PrivateTmp=yes` (variant B, and C with `NoNewPrivileges`, empty ambient set, `ProtectHome=read-only`): D-Bus reads return identical values and `systemctl restart -- <name>` works (polkit active); root cannot read a nobody-owned mode-600 file or a mode-000 file; `systemctl cat` prints 13 lines instead of 18, silently omitting the two files; `/etc` is read-only. Attempt 1 failed earlier because root without `CAP_DAC_OVERRIDE` and `CAP_DAC_READ_SEARCH` could not read a script in a 0750 home directory | - | - | - | Y | 1 of 1 | V §8, `u20.out`, `u20.attempt1.out` [raw] |
| E54 | A mask can ship in `/usr/lib/systemd/system`: `FragmentPath` is a `/usr/lib` path (allowed by the location rule), `LoadState=masked`, `UnitFileState=masked`, no `/etc` entry. That the file is a link to `/dev/null` is stated by V but not shown in the raw output (Section 1.3) | - | - | - | Y | 1 of 1 | V U13, `u13.out` [raw] |
| E55 | Ubuntu 24.04 socket-activates sshd: `ssh.socket` active with `Triggers=ssh.service`; `ssh.service` has `TriggeredBy=ssh.socket` and `Requires` including `ssh.socket`; both enabled, in `/usr/lib` | - | - | - | Y | 1 of 1 (O, T did not read `ssh.socket`) | V U7, §8 D5b [raw] |
| E56 | The reload journal line is `Reloading requested from client PID n ...` on 255 and `Reload requested from client PID n ...` on 257 and 259 | - | Y | Y | Y | 3 of 3 differ by version (O, T `Reload`; V `Reloading`); K not checked | V §2 [raw], O and T raw journal files |
| E57 | `UnitFileState` values over all unit files on V: alias 36, disabled 53, enabled 126, enabled-runtime 3, generated 9, indirect 7, masked 10, static 248, transient 32; `enabled` (`ssh.service`) and `indirect` (`lightdm.service`) read with `show`; `disabled` and `static` taken from `list-unit-files` counts and example names | - | - | - | Y | 1 of 1 | V §8 D5 [raw] |
| E58 | Persistent `set-property` (no `--runtime`) writes `/etc/systemd/system.control/<unit>.d/50-CPUWeight.conf`, applied live, `NeedDaemonReload=no`, listed in `DropInPaths` before the `/run/systemd/system.control` entry | - | - | - | Y | 1 of 1 | V §8 D6 [raw] |
| E59 | Restarting a unit by exact name also restarted a dependent unit that has `PartOf=` and `After=` on it (both `InvocationID` values changed) | - | - | - | Y | 1 of 1 (`PartOf=` only) | V §8 D7 [raw] |
| E60 | Real aliases on V: `sshd.service` is `Id=ssh.service`; `syslog.service` is `rsyslog.service`; `display-manager.service` (an `/etc` link spelled `/lib/systemd/system/lightdm.service`) is `Id=lightdm.service`, `UnitFileState=indirect` | - | Y (`sshd`) | Y (`sshd`) | Y | `sshd` alias 3 of 3 (K -) | V U9, §8 D5b [raw] |

Note on E30 and the matrix: on V the `a_disk` cell of M14 is `CHG` in the final pass and `MASKED` (could not
compute) in the first pass (`u10b_pass1.out`); M14 is not evidence for `a_disk`.

### 2.2 Universal differences from RW-34's expectations

Reproduced wherever tested (every host that ran the row agrees), so the design text should change (Section 8):

1. A restart runs the configuration systemd **has loaded**, not the disk (E07 to E11; 4 of 4). RW-34 hashes disk
   content only.
2. A masked unit's `FragmentPath` is the symlink path, not `/dev/null` (E21; 4 of 4).
3. A transient unit has a `FragmentPath` (E23; 4 of 4).
4. `show` of a missing unit exits 0 (E25; 4 of 4).
5. `DropInPaths` order is application order by basename, not path order (E02; 4 of 4).
6. A loaded unit's reported `FragmentPath` and `DropInPaths` are the loaded view and cannot show a drop-in added after
   the last load (E30; M07 on 4 of 4; M20 on 3 of 3 that ran it for the reported-path candidate).
7. `NeedDaemonReload` has false positives and false negatives (E13 to E15, E36 on every host that ran them; E47 on V
   only). It is not a sufficient guard on any host.
8. `/lib/systemd/system` never appears on a merged-`/usr` host (E24; 4 of 4, all merged).
9. A refused `restart` (start limit) leaves the unit failed (E17; 4 of 4).
10. Reported paths can lie in `/usr/local/lib/systemd/system`, `/run/systemd/generator*`,
    `/run/systemd/system.control`, `/etc/systemd/system.control` and `/run/systemd/system`, none of which RW-34 lists
    (E05, E32, E35, E58; 3 of 3 that ran them).
11. `sshd.service` resolves to `Id=ssh.service` with both in `Names` on the Pi, on host-c and on V (raw
    `probes/pi8/out/u6_u9.out` lines 274 to 278, `probes/host-c8/out/u6_u9.out` lines 272 to 276, V U9 and
    `probes/host-t8/out/u21.out` D5). The Pi and host-c reports do not discuss it; it is read from their raw output.
    See Section 5.8. K was a WSL2 host and did not read it.
12. A mask can sit in an allowed location: on V real masks live in `/usr/lib/systemd/system` (E54, one host; the
    mechanism is a packaging choice, so other distributions may differ).

### 2.3 Host-dependent differences (and the host dimension)

| Difference | Hosts | Dimension that separates them | Isolated? |
|---|---|---|---|
| `systemctl cat` of a linked unit prints `# <link> -> <target>` (K, T) or `# <link>` only (O, V); candidate (c) therefore changes on M16 on K and T, not on O and V | K, T against O, V | Four builds: 255 (Ubuntu, x86_64) behaves like 257 (Debian, aarch64); 259.5 and 259.9 (Ubuntu, x86_64) behave alike. Distribution and architecture cannot explain it (V and T are both Ubuntu x86_64 and differ); kernel does not either (K and O run 6.18 and differ). What remains is the systemd version (257.13 < x <= 259.5; 258 untested) or an Ubuntu package change between 255 and 259 | **Partly**: isolated from distribution, architecture and kernel for these four builds (V §5, §9); version against package change not separated; no other distribution at 259 tested |
| Line order of `systemctl show -p A,B,...`: K and T the same order as listed; O and V a different order (V: `ExecStart Environment Description ActiveState ...`, as O) | K, T against O, V | same as the row above | **Partly**, same boundary |
| `/run` is `noexec`: generators and `ExecStart=/run/...` fail on O and V | O, V | mount options of the image or installer. Debian 13 and Ubuntu 24.04 Server both have it; K (WSL2) and T (Ubuntu 26.04) do not. Not Debian-specific | Yes by mechanism on both hosts (O E2, V U23 `Permission denied`); why T differs from V is not isolated |
| `NeedDaemonReload` after a drop-in in a new drop-in directory: `no` on V (E47); `yes` on O and T for M20 (a new dash-prefix directory) | V against O, T | version (255 against 257 and 259.9), Ubuntu package, kernel (6.8 against 6.18 and 7.0) | **No**: seen on one host; K did not run the case; the cause is not isolated (V U22 hypothesis only) |
| `NeedDaemonReload` after `mask` of an active unit: `yes` on V, `no` on O and T | V against O, T | same dimensions | **No**, one host |
| `daemon-reload` duration 0.13 s, 0.5 to 0.8 s, 0.3 to 0.7 s, 0.5 to 0.7 s | all | hardware, unit count, load | No |
| Race rate at 50 ms unguarded: 18.3%, 17.7%, 1.7%, 1.2% | T and V lower | editor cadence (about 1 per s on O, 0.4 per s on T and V), reload duration, load | **No**; designed not comparable (T §6, V §6) |
| `show --json` absent | O, T, V (K not tested) | cannot say whether 259.5 has it | Not tested on K |
| Wording of messages: reload journal line (E56), start-limit first line (E17), `Created symlink`, bus failure, mask refusal, `Removed`, transient `Description` (`[systemd-run] ...` on T only) | V against T, partly O | version | Partly (V §5: differs from T in each); none may be parsed |
| User unit in `/etc/systemd/user` reported as `/etc/xdg/systemd/user/...` | V | `/etc/xdg/systemd/user` is a symlink to `../../systemd/user` on that host (V U16) | Not tested elsewhere |

### 2.4 Not isolated or not tested

- Whether the `cat` annotation and `show` order differences come from a systemd version between 257.13 and 259.5 or from
  an Ubuntu package change (V §5). Distribution, architecture and kernel are excluded for four builds only.
- Whether the new-directory `NeedDaemonReload` miss and the M14 reload state are 255-specific, Ubuntu-24.04-package-specific
  or present on other builds (single host; K and 258 untested).
- Whether the race rate difference between O and T and V is cadence, hardware or kernel (T §6, V §6).
- Behaviour on non-merged `/usr`, systemd older than 255, 258, RHEL family, SUSE, Arch, containers, non-systemd hosts,
  SELinux and AppArmor-confined helper or agent, real services with `Type=notify`, sockets, `ExecReload=`, dependency
  graphs beyond one `PartOf=` pair (K §8, O §9, T §9, V §11).
- A new drop-in directory next to an existing directory of another name, or a new directory for a template instance:
  not run (V §11).
- Whether sorting `Requires`, `BindsTo`, `PartOf`, `Wants` is enough for a stable D-Bus hash, or `Names`, `Before`,
  `After` and others need it too (V §7; `d_dbus_s` ran for two rows).
- `systemctl cat` exit status when files are unreadable; helper behaviour with other polkit rules or no polkit, with
  SELinux, as a non-root user; `daemon-reload` from the restricted unit (V §8, not run).
- Restart of a socket-activated service or of `ssh.socket` itself: only read (V U7).
- `LoadUnit` for a name that is not a known unit, and what loading an unloaded unit costs or changes: not examined.

## 3. Analysis against RW-34

RW-34's unit row, as the design text states it: `unit:{host}/{canonical_id}` plus `config_hash` = SHA-256 over the
`FragmentPath` contents and every `DropInPaths` file as (path, content) pairs sorted by path; valid when
`FragmentPath` is under `/etc/systemd/system`, `/usr/lib/systemd/system` or `/lib/systemd/system`; generated and
transient units are not valid targets; `EnvironmentFile=` and `ExecStart` binaries are not bound; an identity that
no longer resolves is refused as `target_changed`. Verdicts: **holds**, **fails**, **partly**.

| # | Assumption | Verdict | Evidence and reading |
|---|---|---|---|
| A1 | Valid fragment locations are the three listed directories | **partly** | Holds as a deny rule for `/run/systemd/generator*` and `/run/systemd/transient` (E23, E34). Incomplete: the reported set also includes `/run/systemd/system`, `/usr/local/lib/systemd/system` (outranks `/usr/lib`), `/etc/systemd/system.control` and `/run/systemd/system.control` (E05, E32). More important: the rule names only `FragmentPath`, but the **drop-ins** of an allowed fragment can come from generator directories and `system.control` (E35, E32), so a `FragmentPath`-only check passes a unit whose behaviour is changed from a forbidden location (O §5 #10 to #13, T §5). All of this is shown on `/run` fragments with the lookup code shared by all locations; only `/usr/lib` and `/etc` fragments were tested in the allowed locations (E04, E08). V adds that a **mask** can ship in `/usr/lib` with an allowed `FragmentPath` (E54), so the location rule never replaces the `LoadState` and `UnitFileState` refusals. |
| A2 | Canonical id resolves aliases; alias retarget invalidates | **holds** | `Id` is the canonical unit, `Names` lists aliases, retarget changes `Id` and `FragmentPath` (E18, E19). The protection that the evidence supports is to compare `Id` and issue the restart by `Id` (K U6). Aliased drop-ins (`<alias>.service.d`) apply to the canonical unit and appear in `DropInPaths` (E18), so a drop-in enumeration keyed only on the canonical name misses them. Real alias: `sshd.service` is `Id=ssh.service` on three hosts (2.2 item 11, E60). |
| A3 | Template instances are covered by the instance id plus template path | **partly** | The instance reports the template fragment and lists instance and template drop-ins in application order (E22). But any instance name of an existing template resolves at once, including never-started ones, so a plan can bind a unit that does not exist yet and `restart` would start it (K failure 8, O §5, T §5). Binding is per instance string; allowlisting by template would admit unlimited instances. Dash-prefix drop-ins were shown for a plain unit only (E38). |
| A4 | A masked unit is recognisable (RW-34's design implies `/dev/null`) | **fails as assumed, holds as an outcome** | `FragmentPath` is the symlink, not `/dev/null` (E21). Masking is visible in `LoadState`/`UnitFileState`, and systemd itself refuses to restart a masked unit while leaving an active one up (K failure 4). A rule that reads the `FragmentPath` file sees empty content and a changed hash, so a hash-only design would still notice, but it cannot tell mask from an empty fragment. A mask in `/usr/lib` (V, E54) has an allowed `FragmentPath`; only `LoadState` or `UnitFileState` shows it, and REQ-6 (regular file by `lstat`) would also refuse it once the link target is a non-regular file (not shown in V's raw output). |
| A5 | Transient units are excluded | **holds, reason changes** | Transient units have a `FragmentPath` under `/run/systemd/transient` and `Transient=yes` (E23); excluded by the location rule and by the property, not by the lack of a path. |
| A6 | Generated units are excluded | **partly** | Generated units are excluded by the `FragmentPath` prefix (E34). Generator drop-ins on ordinary units are not excluded (E35), and a changed generator input is invisible to `NeedDaemonReload` (E36). On a host with `noexec` `/run` (O and V) a generator in `/run/systemd/system-generators` is silently not run (E37), so absence of a generated artefact is not evidence of absence of a generator. |
| A7 | `/lib/systemd/system` vs `/usr/lib/systemd/system` | **holds trivially, untested where it matters** | All four hosts are merged; systemd always reports `/usr/lib`, also for an alias link written with the `/lib` spelling (E24). Hashing both spellings counts one file twice and changes the digest (K U9, O U9, T U9, V U9). The non-merged case, the only one where `/lib` could appear, is not demonstrated. `dpkg -S` knows only the `/usr/lib` spelling on Ubuntu (T U9). |
| A8 | `config_hash` over (path, content) of reported paths sorted by path | **partly** | Deterministic (E27). Catches fragment and drop-in content and path changes, masks, alias-name drop-ins and `set-property` (E28, E29, E32). **Misses** a drop-in added since the last load (M07, M20), because the reported list is the loaded view (E30); the `NeedDaemonReload` flag that would warn of it is also silent on V for a new directory (E47). Says nothing about what is loaded when disk differs (M13, M18, E15). Sorting by path discards the application order, which is a function of basenames across directories and so already determined by the path set (K U10), so the loss is not demonstrated to matter. False invalidations: moved drop-in (M09), comment edit (M10) (safe side). `DropInPaths` text cannot be split on whitespace (E39). If the loaded part is hashed from D-Bus, set-valued properties must be sorted first (E49). |
| A9 | `EnvironmentFile=` and `ExecStart` binaries are not bound | **holds as stated** | No candidate changes on M11 and M12 (E33). Not shown: that a restart then picks up the new file contents (documented systemd behaviour, no probe). A restart of the target also restarted a `PartOf=` dependent on V (E59, one host, `PartOf=` only); `Requires=`, `BindsTo=` and sockets are not tested. |
| A10 | (Implicit) the bound config is what a restart runs | **fails** | `restart` and `try-restart` run the loaded config and exit 0 with only a stderr warning (E07 to E11). A hash of disk content answers "what would the next load read" (K §4). This is the largest gap in RW-34. |
| A11 | `NeedDaemonReload` as a guard | **fails as sole guard** | False negatives: mtime-preserved edit, rename-replace (restart ran stale), generator input, `set-property --runtime` (E14, E15, E32, E36; every host that ran them) and, on V only, a drop-in in a new drop-in directory in any location including `/etc` and `/usr/lib` (E47). False positives: `touch`, edit-and-revert (E13). It remains a useful cheap extra refusal because it errs on the safe side except for those five classes. |
| A12 | Re-resolve "immediately before execution" closes the race | **partly** | Reduces, does not close (below). |

### 3.1 The race, honestly

- Two windows exist. **W1**: approval to dispatch, arbitrary length (minutes or days). A dispatch-time recompute and
  compare closes W1 deterministically for everything the identity covers. **W2**: the last dispatch-time read to the
  manager acting on the restart job, tens of milliseconds (guard `systemctl` calls 9 to 54 ms, restart 9 to 47 ms,
  E-timings in 1.2). Nothing in the evidence closes W2 (K U11, O §6, T §6). Which window each experiment row sampled
  is not separated by the reports; the "delay" in the rows is a gap between approval-read and dispatch of 50 ms or 0,
  not W1 at realistic length (synthesis reading).
- K, 120 restarts per row, one run, guards defined differently from O and T: at 50 ms unguarded 22/120; guarded 5/93,
  4/104, 2/102. At zero delay unguarded 1/120, guarded 6/120, 5/116, 5/116. Synthesis calculation, Wilson 95%:
  unguarded 50 ms 18.3% (12.4 to 26.2); guarded 2.0% to 5.4%; zero delay unguarded 0.8% (0.1 to 4.6) against guarded
  4.3% to 5.0% (1.9 to 10.5).
- O, 3600 trials, three runs: unguarded 50 ms 53/300 = 17.7% (13.8 to 22.4); guarded 1.5% to 4.2% (intervals
  overlap each other, so the guards cannot be ranked: G4 and G5 were not measurably better than G1 and G2). At zero
  delay unguarded 7/300 = 2.3% (1.1 to 4.7) and guarded 1.3% to 5.5%. **No guard reached zero in any row.**
- T, 900 trials (830 clean), editor cadence about 0.4 reloads per second: unguarded 50 ms 1/60 (0.3 to 8.9); guarded
  rows 0 to 2 mismatches each; zero delay 0/90 unguarded, 2 mismatches over 349 guarded restarts. T says itself that
  the experiment could not rank guards or show a reduction (T §6).
- V, 450 trials at 50 ms only (the zero-delay rows were dropped to stay inside the reload cap), 14 to 16 editor reloads
  per run, about 0.4 per second: unguarded 1/80 (1.2%, 0.2 to 6.7); G1 0/88 (0.0 to 4.2); G2 1/88 (0.2 to 6.2); G4 0/83
  (0.0 to 4.4); G5 1/82 (0.2 to 6.6); 5 trials tainted by a foreign reload and 10 after the editor stopped were
  excluded (raw `u17_summary.txt` matches). Three mismatches in 421 clean restarts. V says itself that this cannot show
  that a guard helps, rank guards or show zero (V §6). A residual mismatch in guarded rows (G2, G5) is consistent with
  O's "no guard reached zero".
- What can be said: at 50 ms under heavy injected reload load, re-reading before restart reduced mismatches on two
  hosts (K, O), with a stronger effect than any difference between guards; T and V (lower reload density, small
  samples) could neither confirm nor refute it. At zero delay the evidence supports
  "the guards did not help"; it does not support "the guards made it worse": the intervals overlap (O G0 1.1 to 4.7
  against G5 3.3 to 9.1 is the only separated pair and it is one row). The inference that a guard's own `systemctl`
  calls widen the window is plausible from the timings and was never isolated (O §6, T §6).
- Rates are not comparable across hosts (cadence differed), not poolable across K's and O/T's guard definitions, and
  are upper-bound stress numbers (editor reloading about every second for minutes), not production rates.
- One host per architecture (aarch64: O; x86_64: K, T and V; K and T share the 259 line, V is 255). A kernel effect
  was never separated from a systemd effect (T §9).
- Post-restart verification: reading the started process environment flagged 18 of 18, 2 of 2 and 1 of 1 wrong
  starts (21 of 21, three hosts) with no false alarm; the property-reread variant had 120, 9 and 2 false alarms (E44).
  V's single wrong start adds almost no independent weight (V §6). Only the one changed value
  (`Environment`) was tested; `ExecStart`, `User` and other properties were not (O §6). Recovery after a detected
  wrong start was not tested.

## 4. Conclusions

### 4.1 Supported by the evidence

C1. A restart executes the loaded configuration, and exit status and stderr warning are the only signals that it
differs from disk. Reproduced on four hosts, units in `/run` and `/usr/lib` (E07 to E11, E08).

C2. Identity must therefore include the loaded configuration, or the restart must be preceded by a reload. A disk
hash alone is insufficient (K §4; O, T and V §8 to §10 reach the same conclusion).

C3. Reported `DropInPaths` and `FragmentPath` are the loaded view, so a disk-side enumeration is needed to see a
drop-in that was added since the last load. The replica of the lookup (`a_disk`) saw every such case on the three
hosts that ran it (O, T, V; E30). `systemctl cat` saw M07 on all four hosts but **did not see a dash-prefix or any
new drop-in directory on V** (M20, E47), so `cat` is not a disk-side oracle (idea I1 failed on one host, which is
enough to rule it out as the only cross-check). `cat` is also version-annotated and not a hash input (E28 M16).

C4. `NeedDaemonReload` cannot be the guard: four false-negative classes seen on every host that ran them (mtime-preserved
edit, rename-replace, generator input, `set-property --runtime`), a fifth seen on V only (drop-in in a new directory,
E47), and two false-positive classes (A11). On V it also read `yes` after `mask` where O and T read `no` (E48), so its
state after a given operation is version- or build-dependent.

C5. `systemctl show` text is not a stable interface: awkward path names are quoted and C-escaped (three hosts), property
order differs between builds (255 and 257 against 259), `--json` does not exist on three hosts (V: no `--json` in
`systemctl` at all; K not tested). A D-Bus read through `busctl` returned an unambiguous array on those three hosts
(E39 to E41). Set-valued D-Bus properties are returned in no fixed order across reloads (V, one host, E49), so a hash
over D-Bus values must sort them.

C6. Binding the canonical `Id`, not the name used in the plan, is what makes alias retargets harmless; and `show`
exit status never says a unit is missing (E19, E25).

C7. A location rule on `FragmentPath` alone is insufficient: drop-ins can arrive from `/run/systemd/generator*`,
`/run/systemd/system.control`, `/etc/systemd/system.control` and dash-prefix directories, and a unit can sit in
`/usr/local/lib` (E05, E32, E35, E38, E58). A mask can also have an allowed `FragmentPath` (E54, V only).

C8. Masked, transient, generated and linked units are distinguishable by `LoadState`, `Transient` and
`UnitFileState` (E20, E21, E23, E34, E54).

C9. The race is reduced but not closed by a dispatch-time re-read; the residual cannot be removed with `systemctl`
(K §6, O §6). T and V could not rank guards (T §6, V §6).

C10. A post-restart check of the started process's own environment detected every wrong start observed (21 of 21, three
hosts, one property) without false alarms; a loaded-property re-read after restart is unusable under concurrent
reloads (E44).

C11. A refused restart under the start limit leaves the unit down (E17), so `unit.restart` has a failure mode that
is an outage, on all four hosts. The first line of the refusal differs by version, so the refusal is detected by
`Result=start-limit-hit`, not by text.

C12. The `sshd.service` / `ssh.service` alias exists on Debian and Ubuntu hosts (three hosts read it), so name-based
protection can miss the canonical name (E60). On Ubuntu 24.04 `ssh.socket` holds the listener and `ssh.service` is
`TriggeredBy` it (E55, one host), so a protected set that names only the service spellings leaves the socket out.

C13. Plan-time D-Bus reads must use `LoadUnit`: `GetUnit` fails for an inactive unit that the manager has unloaded (E50,
one host; consistent with E11). `GetAll` returns every property of Section 5.2 (E51).

C14. Under a helper directive set with `CapabilityBoundingSet=CAP_NET_ADMIN` and `ProtectSystem=strict`, D-Bus reads and
`systemctl restart` still work on V (polkit active, uid 0), but root cannot read files it does not own unless they are
world-readable, and `systemctl cat` silently drops them (E53, one host, one polkit configuration). Whether the helper
reads files with a DAC capability, reads D-Bus only, or fails closed is therefore forced by the directive set; REQ-1
and the directive set of `02-r3-r9-helper-and-skill-runner-synthesis.md` must be decided together.

C15. `UnitsLoadFinishTimestampMonotonic` did not move on reload on V (E52), so idea I5 does not work with it on 255
(failed, one host).

### 4.2 Not supported, or still open

- That any dispatch-time guard reaches zero residual mismatch; that guards G1 to G5 can be ranked (O §6, T §6, V §6).
- That the algorithm works on supported distributions, versions older than 255 or at 258, non-merged `/usr`, RHEL
  family, SUSE, Arch, containers, non-systemd hosts, or with a confined helper.
- That a general loaded-equals-disk check exists: the probes compared one property (`Environment`); a property-level
  comparison for all properties was not built (O §7, T §7, V §7). The loaded-property D-Bus hash `d_dbus` is stable
  across restart and reload on V except for unsorted set-valued properties, over 23 rows on one host; the sorted form
  `d_dbus_s` ran for two rows. Its stability on other builds, and whether four sorted properties suffice, are untested.
- That `a_disk` (a bash replica, about 40 lines, checked against the 23 matrix rows on O, T and V) matches systemd's
  lookup in general.
- That post-restart verification works for properties other than `Environment`, for services that fork or drop
  privileges, or for `Type=oneshot`; that a recovery restart avoids the same race.
- That restart is safe for units with dependents other than a `PartOf=` pair (E59), sockets, `ExecReload=`,
  `Type=notify`.
- Whether aliases, linked units, generated units and `set-property` drop-ins are valid targets (all reports reserve it
  as an owner decision).
- That the race exposure in production is acceptable; no production rate exists.
- That the new-directory miss of `NeedDaemonReload` (E47) and the `mask` reload state (E48) are properties of systemd 255
  in general; they were seen on one build.
- That a helper restricted to `CAP_NET_ADMIN` works with other polkit rules, without polkit, under SELinux, or as a
  non-root user (V §8).

## 5. Recommendations as implementation requirements

These are recommendations for the implementer and the owner, not accepted decisions. "Evidence" gives the
supporting finding; a requirement with no evidence id is a design choice made here and is marked **(design)**.
Requirements that need an owner decision are cross-referenced to Section 9.

### 5.1 Where the identity is computed

- **REQ-1.** The identity is computed by the helper (root), through a new read operation, not by the agent: drop-in
  files may be unreadable to an unprivileged reader while `show` still lists them (E26). The default helper groups in
  `04` section 8.3 have no unit read group, so one is needed (Section 8). A read failure is a refusal (`read_failed`),
  never an empty hash.
  **Directive set (E53).** Reading files as root is not guaranteed: under `CapabilityBoundingSet=CAP_NET_ADMIN` root could
  not read a nobody-owned mode-600 file or a mode-000 file, and `systemctl cat` silently omitted them (13 lines instead of
  18). The helper therefore must not use `systemctl cat` as a reader, and must decide together with the helper directive
  set (`02-r3-r9-helper-and-skill-runner-synthesis.md`, Section 5) whether it holds `CAP_DAC_READ_SEARCH` (the directive
  set then widens), reads only what it can read and refuses the rest with `read_failed`, or is not restricted this way
  **(owner decision, Q10)**. D-Bus reads and the restart itself worked under `CAP_NET_ADMIN` on V.
- **REQ-2.** v1 scope is the system manager and `.service` units only **(design)**. User scope behaves the same on
  the rows tested (E42) but is outside RW-34's directories; sockets, timers and mounts were never probed.
- **REQ-3.** Process execution goes through the `ProcessExecutor` port (R3, R4). The evidenced read path is `busctl`
  (`LoadUnit`, `get-property`, `GetAll`, `--json=short`) (E40, E50, E51). An in-process D-Bus client would need a new workspace dependency and an ADR;
  it is untested here (Section 6, I10). Never pass `--json` to `systemctl` (absent on three hosts, E41) and never parse
  `systemctl show` or `cat` text.

### 5.2 Inputs read for the identity

All read through D-Bus (`org.freedesktop.systemd1`), as the reports recommend (O §8, T §8). Scalar properties from
`systemctl show` are acceptable only if the output is parsed as `Key=Value` lines and the value is not a path list;
the `busctl` form is preferred for everything. `GetAll` (99 Unit and 334 Service properties) and `LoadUnit` for an inactive unit worked on V (E50, E51); neither was
run on K, O or T. Use `LoadUnit` at plan time, not `GetUnit` (E50).

| Input | Source (Unit interface unless noted) | Use |
|---|---|---|
| `Id` | `Id` | canonical name; bound |
| `Names` | `Names` | protection and allowlist matching only; not bound |
| `LoadState` | `LoadState` | must be `loaded` |
| `UnitFileState` | `UnitFileState` | must be in the accepted set (below) |
| `Transient` | `Transient` | must be false |
| `FragmentPath` | `FragmentPath` | location check; bound; content hashed |
| `SourcePath` | `SourcePath` | informative; generated units set it (`/etc/fstab`) or empty (E34) |
| `DropInPaths` | `DropInPaths` (array of strings) | location check; bound in reported order; contents hashed |
| `NeedDaemonReload` | `NeedDaemonReload` | must be false at bind and dispatch |
| loaded properties | `Service` interface: `ExecStart` (path, argv, ignore flag only), `ExecStartPre`, `ExecStartPost`, `ExecStop`, `ExecReload` argv, `Environment`, `EnvironmentFiles` (paths), `User`, `Group`, `WorkingDirectory`, `Restart`, `Type`, `StartLimitIntervalUSec`, `StartLimitBurst`, plus `Requires`, `BindsTo`, `PartOf`, `Wants` (Unit) | hashed as the **loaded hash**; the list is a design choice **(design)**. The four set-valued properties are **sorted** before hashing (REQ-22, E49) |
| `ActiveState`, `SubState`, `InvocationID`, `ExecMainPID`, `ActiveEnterTimestampMonotonic`, `Result` | dispatch-time only | pre- and post-restart observation, never in the hash |

- Strip runtime fields from `ExecStart` (`start_time`, `stop_time`, `pid`, `code`, `status`): the raw property changes
  on every restart and reload (K §7 d_raw, O §7, T §7). Keep only path, argv, ignore flag.
- The accepted set for `UnitFileState` is `enabled`, `disabled`, `static`, `indirect` **(design)**. All four were
  observed on real units on V (E57: `enabled` `ssh.service`, `indirect` `lightdm.service` read with `show`; `disabled` and
  `static` from `list-unit-files`); confirm on a second host (Section 10). Refuse `masked*`, `linked*`, `transient`,
  `generated`, `bad`, `alias` and anything unknown (fail closed). **`enabled-runtime` is outside the set**: V had three
  such units and its probe units (enabled with `--runtime`) read that state; a unit enabled only for this boot is refused.
  Whether it should be accepted is an owner point (Q2).
- Refuse a path that contains a control character (newline, tab, NUL), a non-absolute path, `..` or `//`. The reports
  show such names are legal and are quoted by `show` (E39); refusing them avoids a parser or log-injection class
  rather than supporting them **(design)**.
- A unit that is `LoadState=masked` is refused even when its `FragmentPath` is under an allowed directory (E54);
  the refusal comes from `LoadState` and `UnitFileState`, not from the location rule.

### 5.3 Location rule

- **REQ-4.** `FragmentPath` must start with `/etc/systemd/system/` or `/usr/lib/systemd/system/` **(design,
  narrower than RW-34 only by the `/lib` entry that never appears, E24)**. If a path starts with
  `/lib/systemd/system/` (never observed) rewrite the prefix to `/usr/lib/...` only when `realpath("/lib")` is
  `/usr/lib`, otherwise keep and treat `/lib` and `/usr/lib` as separate directories; deduplicate by canonical path
  (E24; non-merged case not demonstrated).
- **REQ-5.** **Every** entry of `DropInPaths` must be under `/etc/systemd/system/` or `/usr/lib/systemd/system/` (and
  end with `.conf`). Any entry under `/run/systemd/generator*`, `/run/systemd/system.control`,
  `/etc/systemd/system.control`, `/run/systemd/system`, `/usr/local/lib/systemd/system` or elsewhere refuses the
  target with `target_unsupported` and the offending path. Evidence: E05, E32, E35, E38 (C7). Persistent
  `system.control` is documented behaviour, not probed.
- **REQ-6.** `FragmentPath` and each drop-in must be a regular file by `lstat` (no symlink, no special file; this
  also refuses a mask link in `/usr/lib`, E54, once `LoadState` has been checked). A
  symlinked fragment is the "linked" state (E20); a symlinked drop-in is reported by its link path while `cat` and
  the loader follow it (K U9, M16), so the hash would depend on a target outside the checked directory **(design)**.
- **REQ-7.** Units whose fragment is in `/run/systemd/system`, `/usr/local/lib/systemd/system` or that have a
  generator drop-in are refused in v1; this is an owner decision (Section 9, Q2).

### 5.4 Resolving names

- **REQ-8.** Validate the requested name against the unit-name grammar (characters `[A-Za-z0-9:_.\\@-]`, a
  `.service` suffix, no leading `-`, no glob characters, not a bare template `x@.service`) before any manager call. A
  leading dash needs `--` on the command line (T U14). Evidence: E22 (bare template exits 1), E25.
- **REQ-9.** Resolve the name to `Id` from the manager. If `LoadState` is `not-found`, refuse `target_not_found`
  (never infer from exit status, E25). If the requested name is an alias (`Id` differs), plan time may accept it but
  the bound identity and the restart use `Id` only (E18, E19).
- **REQ-10.** For an instance (`x@y.service`): `y` must be explicitly allowlisted by exact instance string
  (Section 5.8). Any instance of an existing template resolves, including never-started ones (E22).

### 5.5 The identity record and hash

```
UnitIdentityV1 {
  tag            = "unit-identity:v1"
  scope          = "system"
  id             : String            // canonical Id
  unit_file_state: String            // from the accepted set
  fragment       : { path, sha256 }  // path as reported, normalised per REQ-4
  drop_ins       : [ { path, sha256 } ]   // reported (application) order
  disk_drop_ins  : [ path ]          // disk-side enumeration, see REQ-12; must equal drop_ins paths
  loaded_hash    : [u8;32]           // over the loaded property set in 5.2
  needs_reload   = false             // asserted, not hashed
}
identity_hash = SHA-256( canonical_encoding(UnitIdentityV1) )
```

- **REQ-11.** Canonical encoding: domain tag first, then each field as length-prefixed bytes (a u32 big-endian length
  and the bytes); lists as a count then elements; no concatenation without framing (E27: the NUL-less form
  collides). Content hashes are SHA-256 over the bytes read from one open file descriptor (a single read per file).
  Use the encoding conventions already used for the plan hash (RW-08 "canonical encoding", `04b`); the exact bytes
  are an implementation choice.
- **REQ-12.** Drop-ins are bound in the order reported by the manager (application order), not sorted by path. This
  differs from RW-34's "sorted by path" and costs nothing, since order is a function of the path set (K U10) and
  adds a check against ordering-rule changes **(design)**. Sorted-by-path is acceptable if the owner prefers to keep
  RW-34's wording.
- **REQ-13.** Disk-side enumeration: independently enumerate the files systemd would load from disk and require the
  path set to equal the reported (loaded) set. The research replica (`probes/pi8/u10b_matrix.sh`, function `a_disk`)
  walks ten directories in this order: `/etc/systemd/system.control`, `/run/systemd/system.control`,
  `/run/systemd/transient`, `/run/systemd/generator.early`, `/etc/systemd/system`, `/run/systemd/system`,
  `/run/systemd/generator`, `/usr/local/lib/systemd/system`, `/usr/lib/systemd/system`,
  `/run/systemd/generator.late`; it considers the unit name, its template name, alias names (symlinks pointing at
  the unit), dash-prefix truncations (`a-b-.service.d`, `a-.service.d`) and `service.d`; the first directory wins per
  file name; order is by basename. Any file in a non-allowed directory refuses (REQ-5). Mismatch between the
  enumerations refuses as `target_stale`. This replica was checked only against the 23 matrix rows on three hosts (O §7, T §7, V §7). It is the **only**
  disk-side oracle with evidence: it caught M07 and M20 on O, T and V, including a drop-in in a new directory (E30, E47),
  while `systemctl cat` and `NeedDaemonReload` did not on V. `systemctl cat` is **not** a cross-check (idea I1 failed,
  Section 6). The enumeration must run at dispatch time over all search directories, so that a directory created after
  the last load is walked (E47 N4, N5).
- **REQ-22.** Before hashing D-Bus values, sort the set-valued properties `Requires`, `BindsTo`, `PartOf` and `Wants`
  (E49: `Requires` came back in two orders around a reload and changed an unsorted hash with no real change). Treat every
  other list-valued unit property (`Names`, `Before`, `After`, `Conflicts`, `Triggers`, `TriggeredBy` and so on) as
  set-valued **(design; V sorted only the four and ran the sorted form on two rows)**. Keep order for `DropInPaths`
  (application order), `Environment` and the `Exec*` argv.
- **REQ-23.** Plan time reads the unit with `LoadUnit`, not `GetUnit` (E50). An inactive unit is unloaded by the manager
  (E11), so `GetUnit` can fail on a unit that exists; that failure is not `target_not_found`. What loading the unit
  costs or changes is not examined.
- **REQ-24.** Do not rely on a journal line to detect reloads or staleness. If a later design reads the journal for a
  reload, it must match both `Reload requested` (257, 259) and `Reloading requested` (255) (E56) and is untested on other
  versions **(design, conditional)**.
- **REQ-14.** The loaded hash is what makes the identity cover "what a restart runs" (C1, C2). At bind time the
  approval card shows the loaded `ExecStart`, `User`, `Environment` values (what will run), not only file names.
- **Known hole, to be stated rather than closed.** A general property-level loaded-equals-disk comparison does not
  exist (O §7, T §7). Consequence for M13/M18-type changes made *before* binding: the bound loaded hash describes
  what a restart runs, so the restart executes what the approver saw; the bound disk hash then describes a config
  that a later reload would load, and the `NeedDaemonReload=no` check does not flag it. That is acceptable only if
  the card says the approval binds the loaded configuration and the disk files as read at that moment (5.9), and a
  later difference is reported as drift. Refusing to bind when the loaded and disk property values differ
  needs a unit-file parser and is **not demonstrated**.
- **Another known hole (V only).** On 255 a drop-in in a new directory is invisible to `NeedDaemonReload` and
  `systemctl cat` (E47). The disk enumeration of REQ-13 is the only evidenced detector; if it were removed or its
  directory list were incomplete, the dispatch check `NeedDaemonReload=no` would pass and the restart would run without
  the drop-in.

### 5.6 Plan time and dispatch time, in order

Plan time (read-only, helper operation `unit.identity`):

1. Validate the name (REQ-8). 2. Query the manager for `Id`, `LoadState`, `UnitFileState`, `Transient`,
`NeedDaemonReload`. 3. Apply refusals (5.7). 4. Check the helper allowlist and protected set against `Id` and all
`Names` (5.8) so that a plan for a unit that can never run is refused early. 5. Read fragment and drop-ins, enumerate
the disk side, compare (REQ-13). 6. Compute the loaded hash. 7. Return the identity; the plan hash includes
`identity_hash` through the RW-08 canonical target identity.

Dispatch time (helper, privileged, with a per-helper lock, I4):

1. Authorization checks that precede dispatch (RW-08: unexpired authorization, plan hash, operation `Running`,
   rate limits). 2. Allowlist and protected check by resolved `Id` and `Names`. 3. Recompute the identity
completely from scratch; any difference from the bound identity refuses with `target_changed` plus the first
differing field. 4. `NeedDaemonReload` must be false (`target_stale`). 5. Record `InvocationID`, `ExecMainPID`,
`ActiveState`, `NRestarts` before. 6. Issue `systemctl restart -- <Id>` as an argv vector, with a timeout (the
helper `limits.timeout`), no shell. 7. Treat a "changed on disk" warning on stderr as `stale_start_suspected`
(E07: the warning appears when disk differs from loaded; on V it was printed in every U22 case, also when
`NeedDaemonReload` read `no`, but only **after** the restart had already run the old configuration, so it is a
post-hoc signal, never a guard, and its wording is untested on other versions). 8. Post-verify (5.10). 9. Return the outcome; classify per
the action journal rules (RW-09), not invented here.

Order rationale: cheap and deterministic refusals first, the full recompute as late as possible, the restart
immediately after the last read (W2 is the only residual).

### 5.7 Refusals and codes

Proposed names, to be aligned with the existing error taxonomy and `describeError`:

| Code | When | Evidence |
|---|---|---|
| `target_changed` | recomputed identity differs from the bound one (RW-34's code); include which field | E07 to E10, E19 |
| `target_not_found` | `LoadState=not-found` | E25 |
| `target_unsupported` + reason | `masked`, `linked`, `transient`, `generated`, `location` (with path), `symlink`, `scope`, `type`, `bad_name`, `control_character` | C7, C8, E20 to E23, E34, E35 |
| `target_stale` | `NeedDaemonReload=yes`, or disk set differs from loaded set | E07, E30 |
| `target_protected` | `Id` or any of `Names` in the protected set | 5.8 |
| `target_not_allowlisted` | `Id` not in the allowlist | 04 section 8.3 |
| `read_failed` | any unreadable file, `busctl` failure, timeout | E26, E25 |
| `restart_failed` | non-zero exit | E17 |
| `start_limit_hit` | `Result=start-limit-hit`; unit is down | E17 |
| `wrong_start` | post-verify mismatch; alert, not refusal | E44 |

`target_changed` needs new approval, as RW-34 says. `read_failed` is not `target_changed`: nothing is known.

### 5.8 Helper allowlist shape

- **REQ-15.** `[systemd] restart_units` entries are exact canonical unit names including the `.service` suffix; no
  globs; templates only as explicit instance strings. At load the helper resolves every entry; an entry whose
  `Id` differs from the written name is a configuration error that names the `Id` (so `sshd.service` is rejected with
  "use `ssh.service`" on Debian and Ubuntu). Evidence: E18, 2.2 item 11 **(design)**.
- **REQ-16.** Protected and allowlist matching compares the resolved `Id` **and** every entry of `Names`. The
  installer default `protected_units` in `04` section 8.3 names `sshd.service`; on the Pi, host-c and V the canonical `Id` is
  `ssh.service`. On Ubuntu 24.04 `ssh.socket` is active and holds the listener, with `ssh.service` `TriggeredBy` it
  (E55): the protected set should also name `ssh.socket` and match a service by `TriggeredBy`. That a restart of the
  service alone leaves the listener up is V's reasoning and was **not tested**; a restart of the socket itself was not
  tested either (design). A restart addressed to `ssh.service` would not match a literal `sshd.service` entry. The installer
  should write both spellings or resolve at install time, and the helper unions the file with the defaults as today.
- **REQ-17.** The helper takes `{ id, identity_hash }` in the request (matching the group table: "restart by canonical
  unit id + config hash") and refuses a request whose `id` is not the manager's `Id` for that name.
- **REQ-18.** A per-unit helper-side restart limit below systemd's default burst (five in ten seconds, E17), with a
  refusal before dispatch rather than a failed unit **(design)**.

### 5.9 What the approval card says

Bound: the canonical unit name; the fragment file and each drop-in file by path and content hash; the loaded
configuration properties listed on the card (what the restart will run); that the unit sits in `/etc` or `/usr/lib`
and is not masked, linked, transient or generated.

Not bound: the contents of `EnvironmentFile=` files and of the programs the unit executes (E33); dependent units that
the restart may restart or stop (not tested); any change made by a privileged user between the last check and the
manager acting (the residual window, C9); the behaviour of the service after it starts; the host-wide state of other
units.

Also stated: a restart can fail and leave the service down (start limit, E17); if the card shows the configuration as
"stale" the approval cannot be issued until someone with root reloads the manager.

### 5.10 Post-restart verification and wrong start

- **REQ-19.** After the restart returns, wait for the job to finish (D-Bus job signal or `systemctl` blocking return,
  never a sleep) and check, in order: `InvocationID` differs from before; `ActiveState` is `active` for a long-running
  service; `Result` is not `start-limit-hit`.
- **REQ-20.** Read what the started process runs and compare with the bound values: `/proc/<ExecMainPID>/environ`
  against bound `Environment` (evidenced, E44), and **(design, untested)** `/proc/<pid>/cmdline` against the bound
  `ExecStart` argv and the process uid against `User`. Do not re-read loaded properties after the restart (E44).
- **REQ-21.** On a mismatch return `wrong_start`, raise a P0 attention item, count it toward the RW-23 circuit breaker,
  do **not** restart again automatically (a recovery restart has the same window, untested, O §9) and do **not** stop
  the unit automatically. Further action needs a new approval **(design)**.
- A `Type=oneshot` or a process that exits before the read cannot be verified this way (not tested); the helper
  reports `unverified` rather than success **(design)**.

### 5.11 Start limit

A `restart` refused by the start limit exits 1 and leaves the unit failed (E17, four hosts; test `Result=start-limit-hit`,
not the message text, which differs on 255). The helper must
not retry, must report `start_limit_hit` with the unit state, and must not call `reset-failed` on its own: that would
also reset the protection the limit provides **(design)**. REQ-18 prevents most occurrences.

### 5.12 Pseudocode

```text
fn resolve_identity(name, sys: &SystemdView, fs: &UnitFs, cfg: &HelperCfg)
    -> Result<UnitIdentityV1, Refusal>:
  require valid_unit_name(name) else Refusal::Unsupported(BadName)
  u = sys.load_unit_props(name)                  // busctl LoadUnit, never GetUnit (E50); GetAll form (E51)
  if u.load_state == "not-found": return Refusal::NotFound   // exit status is not a signal
  if u.load_state != "loaded":     return Refusal::Unsupported(LoadState(u.load_state))
  if u.transient:                  return Refusal::Unsupported(Transient)
  if u.unit_file_state not in {enabled, disabled, static, indirect}:
        return Refusal::Unsupported(UnitFileState(..))   // masked* (also a /usr/lib mask, E54), linked*, enabled-runtime,
                                                         // alias, generated, transient, bad
  if cfg.protected ∩ ({u.id} ∪ u.names) != ∅: return Refusal::Protected
  if {u.id} ∩ cfg.allow == ∅:              return Refusal::NotAllowlisted
  if u.need_daemon_reload:                 return Refusal::Stale

  frag  = canonical(u.fragment_path)
  drops = u.drop_in_paths.map(canonical)        // as-array from D-Bus, never split text
  for p in [frag] ++ drops:
      require no_control_chars(p) and under_allowed(p) else Refusal::Unsupported(Location(p))
      require fs.lstat(p).is_regular_file()    else Refusal::Unsupported(Symlink(p))
  disk = enumerate_disk(u.id, u.names, template_of(u.id))   // REQ-13, over all search dirs
  for p in disk: require under_allowed(p) else Refusal::Unsupported(Location(p))
  if disk.paths != drops.paths ++ [frag]: return Refusal::Stale   // drop-in added since load

  frag_h  = sha256(fs.read_once(frag)?)         // read failure -> Refusal::ReadFailed
  drop_hs = drops.map(|p| (p, sha256(fs.read_once(p)?)))
  loaded  = sys.service_props(u.id)             // ExecStart sans runtime fields, Environment, ...
  loaded  = sort_set_valued(loaded)             // Requires, BindsTo, PartOf, Wants, other list sets (REQ-22, E49)
  return UnitIdentityV1 { id: u.id, state: u.unit_file_state, fragment: (frag, frag_h),
                          drop_ins: drop_hs, loaded_hash: sha256(encode(loaded)) }

fn dispatch_unit_restart(req {id, identity_hash}, sys, fs, cfg, runner, lock):
  guard = lock.acquire()                          // helper-side serialisation only
  now_id = resolve_identity(req.id, sys, fs, cfg)?          // full recompute, all refusals of 5.7
  if hash(now_id) != req.identity_hash: return Err(TargetChanged(first_diff))
  before = sys.runtime(req.id)                    // InvocationID, ExecMainPID, NRestarts
  if helper_restart_budget_exhausted(req.id): return Err(RestartBudget)
  out = runner.run(["systemctl", "restart", "--", req.id], cfg.timeout)
  if out.failed:                      return Err(classify(out, sys.result(req.id)))  // start_limit_hit
  after = sys.runtime(req.id)
  if after.invocation_id == before.invocation_id: return Err(NoRestartObserved)
  if out.stderr contains "changed on disk":       flag(StaleStartSuspected)
  actual = proc_view(after.main_pid)              // environ, cmdline, uid
  if !matches(actual, now_id.loaded):             return Outcome::WrongStart(actual diff)
  return Outcome::Restarted
```

`LoadUnit` and `GetAll` were run on one host (V); the in-process D-Bus read is untested. The property list, the sort
rule beyond four properties, the cmdline and uid comparison, the helper file-read capability (REQ-1, E53) and the
restart budget are design choices, not findings.

## 6. Ideas beyond the evidence

Every item is an **idea**, not a finding and not a requirement. Each gives the expected benefit, the cost and how to
test it.

| Id | Idea | Expected benefit | Cost | How to test |
|---|---|---|---|---|
| I1 | **FAILED, do not use.** Use `systemctl cat` as a disk-side oracle: parse only its `# <path>` header lines for the path set and compare with the replica of REQ-13 | It caught M07 on all four hosts and M20 on O and T (E30). **Evidence against:** on V `systemctl cat` printed no warning and no header for a drop-in in a new directory in `/run`, `/etc` and `/usr/lib` (V U22 N2, N4, N5), and M20 columns b and c were `same` (V §7); `cat` of a linked unit is annotated differently by version (E28 M16); it silently omits files the caller cannot read (E53) | not applicable | none: a different oracle is needed; the replica of REQ-13 is the only one that caught every row (REQ-13) |
| I2 | Helper-run `daemon-reload` before bind and dispatch ("reload-then-verify"): loaded equals disk by construction | closes M13, M18, generator-input staleness and the new-directory miss (E47) without a parser | reload is global: it applies pending disk changes to **every** unit, 0.13 to 0.8 s here, and on shared hosts reloads other sessions' pending work; changes the owner's expectation of what a "restart" does; the helper may not be allowed to reload under a restricted directive set (not run, V §8) | measure on a host with pending unrelated edits; decide with Q3 in Section 9. Not recommended as a default |
| I3 | `systemd-run` or `systemctl --runtime` as a restart vehicle | none found: `--runtime` applies to `enable`, `mask`, `link`, `set-property`, not to `restart`; `systemd-run` makes a new transient unit that is not the allowlisted target. `set-property --runtime` is the *threat* (E32), already covered by REQ-5 | none | read the manual on a host; no probe needed |
| I4 | A helper-side lock (`flock`) around identity read, restart and post-verify | serialises helper requests, so two restarts or a future helper-initiated reload cannot interleave | does not stop third parties (root can always edit, K U11); small | two concurrent helper requests in a real-systemd test |
| I5 | **FAILED on 255.** Reload-free "reload-detect": bracket the dispatch-time read and the restart with the manager's `UnitsLoadFinishTimestampMonotonic` and flag if it moved | Would have detected the W2 race after the fact. **Evidence against:** the property (and `UnitsLoadStartTimestampMonotonic`) read identically before and after a `daemon-reload` on V, the script reloading between the two reads (V §8 D3, E52) | not applicable | one host only; if tried elsewhere, read the value around a reload first. Do not use as proposed |
| I6 | Cgroup and `/proc` verification: confirm `ExecMainPID` lives in the unit's `ControlGroup` (`/proc/<pid>/cgroup`), that its start time is after the dispatch time, and compare cmdline and uid | ties verification to the process that actually started; extends E44 beyond `Environment` | `/proc` visibility under hardening (`hidepid`), `Type=forking` main-PID semantics | extend the probe to change `ExecStart` argv and `User` drop-ins and count flagged wrong starts and false alarms |
| I7 | Hash the effective loaded config with a **denylist** over the unit's whole `Service`+`Unit` D-Bus property set (drop volatile ones: timestamps, PIDs, counters, memory and CPU usage, `ActiveState` family) instead of an allowlist | no property can be forgotten | false invalidations from volatile or version-added properties; list-valued properties arrive in no fixed order across reloads (E49) and must be sorted; `GetAll` returns 99 and 334 properties on 255 (E51) | compute twice across restart and reload on four hosts; count unstable properties, with set-valued ones sorted |
| I8 | Fail closed on any drift and surface it as an attention item that tells the owner to run `systemctl daemon-reload` themselves | keeps the helper out of reload (I2) and keeps the human in the loop | refusals on harmless `touch` (E13) | UI test with an injected stale state |
| I9 | Pin by `InvocationID` or `ActiveEnterTimestampMonotonic` as an optional precondition "restart only if the running incarnation is still the one the plan saw" | avoids a duplicate restart after someone else restarted the unit between approval and dispatch | refuses legitimate cases; semantics must be chosen per contract | integration test: restart after approval, dispatch, expect refusal |
| I10 | In-process D-Bus client (one connection for check and restart) instead of spawning `busctl` and `systemctl` | the reports infer that each spawned call (9 to 54 ms) widens W2; one connection could cut it to a few ms | a new bus-client dependency (workspace rule R11) and an ADR; inference not isolated (O §6, T §6) | re-run the O race rows with the client; compare rates with Wilson intervals at the same cadence |
| I11 | `--job-mode=fail` on the restart | refuses to replace a pending job from someone else instead of silently clobbering it | behaviour untested | run restart during a long `ExecStop` and observe |
| I12 | Refuse units that carry a "managed" marker (a drop-in or file written by configuration management) | removes the units most likely to be edited and reloaded concurrently | needs an owner-defined marker | list as an allowlist constraint; test the refusal |
| I13 | `try-restart` instead of `restart` when the plan is "if running" | never starts a stopped or nonexistent instance (E22) | does not start a stopped service; contract semantics differ | test on a stopped unit |
| I14 | Normalise comment-only and whitespace-only changes out of file hashes | fewer false invalidations (M10) | writes a unit-file lexer that must match systemd's; risk of hiding a real change | fuzz the normaliser against `systemd-analyze verify` outputs |
| I15 | A narrow read path for restricted files: grant the helper only `CAP_DAC_READ_SEARCH` (read, no write) beside `CAP_NET_ADMIN`, or run the file reads in a short-lived child that holds it | lets the helper hash files it does not own (mode 600 or 000, nobody-owned) that systemd itself can read (E53), instead of refusing every such unit | widens the directive set of the helper; interacts with the confinement design in `02`; untested | repeat V's `u20` with `CAP_DAC_READ_SEARCH` added; confirm the nobody-owned 600 and root 000 files read and that nothing else changes. Decide with Q10 |

## 7. Test plan for the implementation

Name tests after behaviour. Marks: **unit** (pure, with fixtures or a `FakeSystemd` in `domovoi-testkit`, per the
repository rule of adding a fake when a port appears), **host** (needs a real systemd, Linux), **gate** (invariant
test in `tests/gates.rs`; never weakened).

### 7.1 Fixtures

- Recorded `busctl --json=short` output for: plain unit, unit with package and `/etc` drop-ins, alias (`ssh.service`
  with `Names`), template instance, masked, transient, generated, linked, a unit with an awkward drop-in name, a
  not-found unit, a `Requires` set returned in two orders, an inactive unit (`GetUnit` failure and `LoadUnit` success), a
  mask in `/usr/lib`, `ssh.service` with `ssh.socket` (`Triggers`, `TriggeredBy`). Record on 255, 257 and 259 hosts
  (no `--json` in `systemctl`, quoted `DropInPaths`, `Requires` order); the reports' raw outputs under
  `probes/*/out/u15.out`, `u6_u9.out`, `u21.out` show the shapes.
- A `FakeSystemd` implementing the inspection port with settable loaded state, disk state and a reload counter, and a
  fake `ProcessExecutor` recording argv.
- A fake filesystem with regular files, symlinks and unreadable files.

### 7.2 Tests

| Test | Kind | Finding |
|---|---|---|
| `identity_binds_canonical_id_when_name_is_an_alias` | unit | E18 |
| `alias_retarget_after_approval_refuses_with_target_changed` | unit, host | E19 |
| `restart_is_issued_by_canonical_id_never_by_alias` | unit, gate | E19 |
| `protected_unit_matches_by_id_and_names` (sshd/ssh) | unit, gate | 2.2 item 11, E60 |
| `allowlist_entry_that_is_an_alias_is_a_config_error` | unit | E18 |
| `installer_default_protected_units_cover_ssh_spellings` | unit | 2.2 item 11 |
| `masked_unit_is_refused_even_though_fragment_path_is_not_dev_null` | unit, host | E21 |
| `transient_unit_is_refused_by_property_and_by_location` | unit, host | E23 |
| `generated_unit_is_refused` | unit, host | E34 |
| `generator_drop_in_on_ordinary_unit_is_refused` | unit, host | E35 |
| `system_control_drop_in_is_refused` | unit, host | E32 |
| `dash_prefix_drop_in_added_without_reload_is_detected_or_refused` | unit, host | E38, E30 |
| `new_drop_in_directory_without_reload_is_detected_by_disk_enumeration` (run for `/etc/systemd/system` and `/usr/lib/systemd/system`) | unit, host | E47 |
| `need_daemon_reload_no_does_not_clear_a_disk_enumeration_mismatch` | unit | E47, A11 |
| `system_cat_and_show_text_are_never_parsed_for_identity` | unit, gate | E41, E47, I1 |
| `no_code_path_passes_json_to_systemctl` | gate | E41 |
| `d_bus_set_valued_properties_are_sorted_before_hashing` (fixture: `Requires` in both orders gives one hash) | unit | E49, REQ-22 |
| `reload_alone_does_not_change_the_loaded_hash` | host | E49 |
| `plan_time_reads_an_inactive_unit_with_load_unit` | unit, host | E50, REQ-23 |
| `get_unit_failure_for_an_unloaded_unit_is_not_target_not_found` | unit | E50 |
| `unreadable_file_under_restricted_capabilities_refuses_read_failed_not_a_partial_hash` | unit, host | E53, REQ-1 |
| `system_cat_is_not_used_as_a_file_reader_because_it_drops_unreadable_files` | unit | E53 |
| `mask_in_usr_lib_is_refused_by_load_state_although_fragment_path_is_allowed` | unit, host | E54 |
| `enabled_runtime_unit_is_refused_unless_the_owner_accepts_it` | unit | E57, Q2 |
| `ssh_socket_is_in_the_protected_set_and_a_triggered_service_matches_by_triggered_by` | unit, gate | E55, REQ-16 |
| `start_limit_refusal_is_detected_by_result_not_message_text` | unit, host | E17, 5.11 |
| `restart_of_target_that_a_part_of_dependent_follows_is_reported_on_the_card` | host | E59 |
| `reload_journal_pattern_matches_both_wordings` (only if the journal is read, REQ-24) | unit | E56 |
| `usr_local_lib_fragment_is_refused` | unit, host | E05 |
| `linked_unit_is_refused` | unit, host | E20 |
| `symlinked_drop_in_is_refused` | unit | REQ-6, K U9 |
| `fragment_edit_without_reload_refuses_target_stale` | host | E07 |
| `drop_in_added_without_reload_is_detected_by_disk_enumeration` | unit, host | E30 |
| `mtime_preserved_edit_does_not_hide_a_disk_change_in_the_hash` | unit, host | E14 |
| `rename_replace_with_restored_mtime_is_caught_by_content_hash` | unit, host | E15 |
| `touch_alone_refuses_target_stale_and_new_approval_is_not_required_after_reload` | host | E13 |
| `identity_hash_stable_across_repeated_reads_reload_and_touch` | host | E27 |
| `identity_hash_changes_on_drop_in_move_and_comment_edit` | unit | M09, M10 |
| `identity_hash_unchanged_by_environment_file_and_binary_change` | unit | E33 |
| `identity_hash_changes_when_drop_in_precedence_changes` | unit | E02 |
| `awkward_drop_in_names_are_read_from_the_array_not_split` | unit | E39, E40 |
| `control_character_in_path_is_refused` | unit | REQ-4 |
| `show_exit_zero_with_not_found_is_refused_target_not_found` | unit | E25 |
| `unreadable_drop_in_refuses_read_failed_never_empty_hash` | unit, host | E26 |
| `template_instance_must_be_allowlisted_by_exact_instance` | unit | E22 |
| `bare_template_name_is_rejected` | unit | E22 |
| `dispatch_recomputes_identity_before_restart` | unit, gate | RW-34, RW-08 |
| `dispatch_refuses_when_need_daemon_reload_is_yes` | unit | E07 |
| `restart_stderr_changed_on_disk_flags_stale_start` | unit | E07 |
| `start_limit_hit_reports_failed_unit_and_does_not_retry` | unit, host | E17 |
| `helper_restart_budget_prevents_start_limit` | unit | REQ-18 |
| `post_restart_environ_mismatch_reports_wrong_start_and_counts_toward_circuit_breaker` | unit | E44, RW-23 |
| `wrong_start_does_not_trigger_automatic_second_restart` | unit | REQ-21 |
| `post_check_does_not_reread_loaded_properties` | unit | E44 |
| `unit_config_change_invalidates_approval` (roadmap M8 test) | unit, host | RW-34 |
| `unit_restart_refused_when_unit_not_in_helper_allowlist` (roadmap M8 test) | unit | 04 section 8.3 |
| `no_other_code_path_calls_systemctl_restart` | gate | R3, R4 |
| `helper_never_calls_daemon_reload` (if Q3 is answered "never") | gate | Section 9 |

### 7.3 Real-systemd tests

CI runs on Linux (GitHub-hosted Ubuntu runners are expected to have systemd as PID 1 and passwordless `sudo`; this
was not checked here). The host tests create units named with a fixed test prefix in `/etc/systemd/system` and
`/run/systemd/system`, never use a glob against `systemctl`, clean up in a drop guard, and verify `/dev/null` is
a character device (the K incident). Tests never sleep: wait on the job result or `systemctl --wait`. They are
`#[ignore]` unless an environment variable names an opted-in systemd host, so they do not run on developer machines.
Alternatives where PID 1 is not systemd: a privileged systemd container or a VM. A `/usr/lib` fragment test needs
root and must restore the directory afterwards. The new-directory test creates and removes a drop-in directory under `/etc/systemd/system` and `/usr/lib/systemd/system`; the restricted-capability test runs the identity read as a transient unit with the helper directive set. Race tests (W2) are research harnesses, not CI tests: they are
statistical and need hundreds of trials per row.

## 8. Follow-up edits to the design documents (list only, for the owner)

| # | Document and section | Current text, summarised | Proposed change |
|---|---|---|---|
| 1 | `03` RW-34, unit row, "Canonical identity" | `config_hash` is SHA-256 over `FragmentPath` and `DropInPaths` contents as (path, content) pairs sorted by path | Replace with the identity record of 5.5: canonical `Id`, accepted `UnitFileState`, fragment and drop-ins in reported order, a disk-side path set that must equal the reported set, and a hash of the loaded properties; state that a restart runs the loaded configuration |
| 2 | `03` RW-34, unit row, "Supported scope" | fragments under `/etc/systemd/system`, `/usr/lib/systemd/system` or `/lib/systemd/system`; generated and transient not valid | Drop `/lib` (never reported on merged hosts; keep only if a non-merged host is validated); apply the location rule to every drop-in, not only the fragment; list the refused classes: masked, linked, transient, generated, `/run`, `/usr/local/lib`, `system.control`, generator drop-ins, symlinked files |
| 3 | `03` RW-34, "Changes that invalidate" | any change to fragment or drop-in; alias resolving elsewhere | add: a drop-in added since the last load; a change to the loaded configuration; `NeedDaemonReload=yes` is a refusal (`target_stale`), not an invalidation |
| 4 | `03` RW-34, "Not bound" | files a unit references and image content are not bound | add dependent units, the programs the unit runs, and the window between the last check and the manager acting; the card says so (5.9) |
| 5 | `03` RW-34, "Lifecycle" | refused with `target_changed` | add the other refusal codes of 5.7 and distinguish `read_failed` from `target_changed` |
| 6 | `03` RW-34, "Name resolution" | container names are validated against Docker's grammar | add unit names: grammar, canonical `Id`, restart by `Id`, instance strings allowlisted exactly, aliases never bound |
| 7 | `03` RW-08, plan hash v2 | canonical target identity is a plan-hash input | add the identity version tag; cross-reference the unit identity encoding |
| 8 | `03` RW-23, first L2 contracts | `unit.restart` has a postcondition check | add the post-restart process check (5.10), wrong-start handling that counts toward the circuit breaker, and start-limit handling (5.11) |
| 9 | `03` helper group table, `systemd.write` | restart by canonical unit id + config hash, allowlisted, never protected | rename the hash `identity_hash`; add a read operation for identity and say which group holds it; match protected and allowlist by `Id` and `Names` |
| 10 | `04` section 8.3, helper allowlist | default `protected_units` includes `sshd.service`; example `restart_units = ["plex.service"]` | protected set matched by `Id` and `Names`; installer writes the host's spelling (`ssh.service` on Debian and Ubuntu, per the Pi, host-c and V outputs), also `ssh.socket` where the host socket-activates sshd (V), and the helper resolves entries; entries must be canonical |
| 11 | `04` section 8.3, helper default groups | read groups listed do not include a unit identity read | add one, or state that identity reads live in `systemd.write` and are off by default; this blocks the plan-time read, so decide (Q6) |
| 12 | `06` M8 | tests `unit_restart_refused_when_unit_not_in_helper_allowlist`, `unit_config_change_invalidates_approval` | add the tests of Section 7 that cover refusals and the post-check |
| 13 | `99` R-8 row | "test on supported distros" | leave open; add pointer to the four reports and this synthesis; state which owner decisions (Section 9) are pending; do not mark resolved |
| 14 | Implementation index, M8 and H1 rows and the R-8 evidence row | M8 gated on "R-8 accepted; H1 complete"; evidence row waits on the research environment | note that four-host evidence exists and acceptance is pending the owner decisions; note the helper work must provide the identity read and `systemd.write` before M8 |
| 15 | `03` RW-34, "Changes that invalidate" and the dispatch check | `NeedDaemonReload` is a guard (item 3 above) | state that `NeedDaemonReload` and `systemctl cat` can miss a drop-in in a new directory (V) and that the disk enumeration, not those, is the stale check |
| 16 | `02-r3-r9-helper-and-skill-runner-synthesis.md`, Section 5 directive set (owner of that file edits it; **not edited here**) | helper directive set with `CapabilityBoundingSet` limited | cross-reference REQ-1 and E53: file reads of foreign-owned restricted files fail under that set; decide the read capability together (Q10) |

## 9. Open questions needing an owner decision

| Q | Question | Options | Costs | Recommendation (marked as such) |
|---|---|---|---|---|
| Q1 | Supported distributions and systemd versions for `unit.restart` | (a) the tested set only (systemd 255, 257, 259.5 and 259.9 on merged `/usr`, four builds) (b) refuse below a minimum version and on non-merged `/usr` at runtime (c) wider validation first | (a) honest but narrow; (b) needs a version gate; (c) calendar time and hosts | **Recommended: (b)** with a floor on tested behaviour, a runtime version check and a clear refusal elsewhere. The tested range is now **255 to 259 on merged `/usr`** (255.4 one Ubuntu 24.04 build, 257.13 Debian aarch64, 259.5 WSL2, 259.9 Ubuntu 26.04), with **258 untested** and nothing older than 255 tested (252 and 249 are the next distribution versions). The algorithm of Section 5 (D-Bus reads with sorted sets, the disk enumeration, `LoadState`, `UnitFileState`, `Transient`, content hashes) worked on all four; the differences seen on 255 touch only things it must not rely on (`NeedDaemonReload` as a guard, `cat` text, `show` text and order, `--json`). A floor of 255 is therefore defensible **for builds shaped like V**, but it rests on one build; the gate must name tested versions (or a floor on tested behaviour), not infer behaviour for untested versions between them |
| Q2 | Valid targets: aliases, linked, generated, `/run`, `/usr/local/lib`, `set-property` drop-ins, `enabled-runtime` units | (a) only `/etc` and `/usr/lib` plain units (REQ-4, REQ-5) (b) add `/usr/local/lib` (c) add generator drop-ins and `system.control` | (a) refuses some real units (including units enabled with `--runtime`, which read `enabled-runtime`), simplest; (b),(c) enlarge what the hash must cover and add untested paths | **Recommended: (a)** for v1; revisit with real owner units; decide `enabled-runtime` explicitly (refused by the accepted set of 5.2) |
| Q3 | May the helper run `daemon-reload`? | (a) never; refuse stale (b) only on explicit owner approval per reload (c) automatic before dispatch | (a) owner runs the command; (b) new contract; (c) global side effects, reloads other work (I2) | **Recommended: (a)** |
| Q4 | Accept a residual W2 rate? | (a) accept, document, rely on the post-check (b) refuse units under config management (I12) (c) wait for an in-process bus client measurement (I10) | no guard reached zero; no production rate | **Recommended: (a)** plus the post-check, with (c) as a later improvement |
| Q5 | Do `EnvironmentFile=` and executable contents stay unbound? | (a) unbound, stated on the card (b) bind their hashes | (b) adds false invalidations from routine file updates; binary hashes change on every package upgrade | **Recommended: (a)** as RW-34 says |
| Q6 | Where does the plan-time identity read live in the helper's groups? | (a) a new `systemd.read` enabled by default (b) inside `systemd.write`, off by default | (a) widens the fresh-install default beyond reads of today; (b) no pre-flight on a fresh install | **Recommended: (a)**, since reads are the fresh-install default (04 section 8.3); decide together with Q10 |
| Q7 | Wrong start handling | (a) alert, no automatic action (b) automatic corrective restart (c) automatic stop | (b) repeats the race and was not tested; (c) causes an outage | **Recommended: (a)** |
| Q8 | Restart semantics for a stopped unit | (a) `restart` (starts it) (b) `try-restart` | (a) can start a never-started instance; (b) differs from the contract wording | **Recommended: (a)** with the exact instance allowlist (REQ-10) |
| Q9 | Bind drop-in order as reported, or sorted by path (RW-34) | as reported / sorted | order is a function of paths; reported adds a version-ordering check | **Recommended: as reported**; either is acceptable |
| Q10 | How does the helper read unit files under its directive set? (E53) | (a) hold `CAP_DAC_READ_SEARCH` beside `CAP_NET_ADMIN` (I15) (b) read D-Bus plus the files it can read and refuse the rest with `read_failed` (c) run unrestricted for the identity read | (a) widens the confinement of `02`; (b) refuses units whose drop-ins are foreign-owned 600 or 000 (systemd still ran such a unit, E26); (c) weakens confinement | **Recommended: (b) as the default, with (a) considered if real owner units need it**; decide together with REQ-1, Q6 and the directive set of `02`. Tested on one host with one polkit configuration |

## 10. What remains unproven, and the cheapest way to prove each

| Item | Hosts and versions needed | Cheapest experiment |
|---|---|---|
| Behaviour on systemd older than 255 (Debian 12, 252; Ubuntu 22.04, 249; RHEL 9, 252), at 258, and on RHEL, SUSE, Arch | one VM or container host per family | re-run `u2_u4`, `u5`, `u5b`, `u22`, `u6_u9`, `u10b_matrix`, `u15` and record `systemctl --version`; takes minutes per host |
| Non-merged `/usr` (`/lib` a real directory) and `/lib` spelling in reported paths | an old Debian or a custom image | `u9`/`u13` rows; check that REQ-4 normalisation and dedup work |
| Property-level loaded-equals-disk check | any host | build a scratch comparison of D-Bus loaded properties against a parse of the disk files for 30 real units; count disagreements and false alarms |
| D-Bus `LoadUnit` and `GetAll` (done on 255 only, E50, E51); stability of the loaded hash from D-Bus values across restart, reload, rename, with sets sorted (done on 255 for 23 rows, sorted form for 2) | 257 and 259 hosts | compute the sorted D-Bus hash twice across `M00`, `M02`, `M10`, `M16`, `M18`; compare with `d_clean` |
| `a_disk` replica against systemd's lookup in general (checked on 23 rows on O, T, V) | any host with many real units | compare replica path sets against reported `DropInPaths` for every loaded unit; expect zero differences |
| Post-restart checks for `ExecStart` argv, `User`, forking, `Type=oneshot`, `Type=notify` | any host | extend the race probe's start script; count flagged and false alarms |
| Recovery after wrong start | any host | repeat the race with a recovery restart and count a second wrong start |
| Real services: sockets, `ExecReload=`, dependencies beyond a `PartOf=` pair (E59), restart of a socket-activated service and of `ssh.socket` | any host | restart a real socket-activated service and a `Requires=`/`BindsTo=` pair; read before and after; check the listener stays up |
| SELinux and AppArmor with a confined helper | RHEL family with SELinux enforcing; Ubuntu with a profile attached | read files and call the bus as the confined helper; expect denials to surface as `read_failed` |
| Containers and non-systemd hosts | Docker with host PID or a systemd-in-container image; Alpine | check that `busctl` fails closed with `read_failed` |
| `UnitFileState` values `enabled`, `disabled`, `indirect`: observed on V only (E57); confirm on a second host, and decide `enabled-runtime` | any other host | read `ssh.service`, a disabled unit, a unit enabled through an alias |
| Persistent `set-property` drop-ins in `/etc/systemd/system.control`: done on V only (E58) | another host | repeat `set-property` without `--runtime`; read `DropInPaths` |
| Guard ranking and a production race rate | two or more hosts of different architecture and a quiet window | at least 1000 trials per row at one fixed cadence, one guard definition across hosts (the earlier reports' samples cannot rank guards) |
| Cause of the `cat` annotation and `show` order differences: isolated from distribution, architecture and kernel for four builds (V §5, §9); version against Ubuntu package change not separated | one 258 host; an Ubuntu 24.04 host with a newer systemd or a Debian 13 host at 257 against an Ubuntu 26.04 package | read the two outputs on each |
| `show --json` on 259.5 (absent on 255, 257, 259.9) | the K host | one command |
| Unit names or paths with spaces in a unit directory the helper allows | any host | already measured (E39); repeat REQ-4 refusal tests |
| Cause and scope of the new-directory miss of `NeedDaemonReload` (E47) and of the `mask` reload state (E48): 255 only so far | 257, 259.5, 259.9 and a 258 host; a second 255 build | run V's `u22` (cases N1 to N6) and the M14 row; add a new directory next to an existing directory of another name, and a new template-instance directory (not run) |
| Helper file reads under `CapabilityBoundingSet=CAP_NET_ADMIN` (E53): other polkit rules, no polkit, SELinux, non-root helper, `daemon-reload` from the restricted unit, `systemctl cat` exit status | hosts with those setups | repeat `u20` variants; add `CAP_DAC_READ_SEARCH` (I15) |
| Whether sorting four properties is enough for a stable D-Bus hash (E49) | any host | run the sorted hash over every row of the matrix and over 30 real units across reloads; count changes |
| `ssh.socket` handling: restart of `ssh.service` with the socket up; restart of the socket (E55) | a host with socket-activated sshd, console access | restart each on a test host with a second session open; observe the listener |

## 11. Contradictions and tensions between the reports

- Candidate (c) on M16: K and T `CHG`, O and V `same`; `cat` of a linked unit annotates `->` on K and T only. Partly
  isolated by V: distribution, architecture and kernel do not explain it; version (257.13 < x <= 259.5) or an Ubuntu
  package change remains.
- `show` property order: K and T agree, O and V agree with each other and differ from K and T. Same boundary.
- K says paths with spaces cannot be recovered from `--value` and recommends failing closed; O, T and V measured that
  they are quoted and C-escaped on one line, with `busctl` unambiguous. Not a contradiction of fact, a K assumption
  superseded by O, T and V.
- K recommends refusing on `NeedDaemonReload=yes` and a text-based `d_clean`; O, T and V show `NeedDaemonReload`
  false negatives (rename, generator, `set-property`, and on V a new drop-in directory) and unstable `show` text, and
  recommend D-Bus.
- **M20 and the new-directory case.** O and T read `NeedDaemonReload=yes` and a changed `cat` for M20 (a new dash-prefix
  directory); V reads `no` and an unchanged `cat` (E30, E47). V names the cause as unisolated and the synthesis does the
  same; it is a contradiction between hosts, not a K miss (K did not run M20).
- **M14.** `NeedDaemonReload` after `mask` of an active unit is `no` on O and T and `yes` on V (E48).
- The `/run` `noexec` effects (silent generator skip, `ExecStart=/run/...` failing) appear on O and V; T ran from
  `/run` (direct execution of a script in `/run` succeeded on T, raw `u18.out`; the unit-start part of T's E6 was
  weak, as V notes, and is not used). K ran on WSL2 with an executable `/run`.
- The guard effect at 50 ms: O showed a reduction; T and V could neither confirm nor refute it (T §6, V §6). K's row
  shows a reduction at 50 ms as well. At zero delay O and K's guarded rows are at or above the unguarded one, T's are
  not; V has no zero-delay rows.
- K did not run the rename-replace restart; O, T and V did and found the stale config running (E15).
- O's report says the rename case was "extended" over K; T says the same; neither contradicts K.
- V says its masks in `/usr/lib` were checked with `ls -l`; the raw output does not contain that listing (Section 1.3).
- V's text reports 574 to 733 ms for reloads; its raw journal lines span 472 to 712 ms (Section 1.3).

## 12. Revision log

This revision adds the fourth host report V (Ubuntu 24.04.5, systemd 255.4, kernel 6.8) to a document that covered K, O
and T. Nothing was re-measured; V's key claims were checked against its raw outputs (Section 1.3). R-8 stays open.

What changed, and why:

- **Title, scope, 1.1, 1.2 (new column V), shared constraints:** four hosts; tested range 255 to 259; `/run` `noexec`
  on O and V; the location confound gained V's allowed-location cases.
- **1.3 (new):** the raw-output checks and the discrepancies found (the `/usr/lib` mask `ls -l` claim, reload range, the
  `u20` and D7 label mismatches, T's E6, the unrecomputed 203).
- **2.1:** a V column on every row; "Reproduced" redefined as `k of n` with the untested hosts named; E16 split (the
  old-mtime added drop-in is `no` on V); E28, E29, E30, E37, E41 and E44 updated; rows E47 to E60 added for V-only findings
  (new-directory miss, M14, D-Bus set order, `LoadUnit`, `GetAll`, timestamps, directive set, `/usr/lib` mask, `ssh.socket`,
  journal wording, `UnitFileState` values, persistent `set-property`, `PartOf` restart, real aliases).
- **2.2:** counts raised to four hosts; item 12 (mask in an allowed location) added; item 11 now three hosts.
- **2.3, 2.4:** the `cat` and `show` order rows re-classified from "not isolated" to "partly isolated" (distribution,
  architecture and kernel excluded; version against package change open); `noexec` is no longer "Debian default"; the
  new-directory miss and M14 are listed as single-host and not isolated; not-tested list reduced where V tested it
  (restart propagation for `PartOf=`, `LoadUnit`, `GetAll`) and extended (new-directory shapes, restricted helper, sockets).
- **3 (analysis):** A1, A2, A4, A6, A7, A8, A9 and A11 updated; A11 now has five false-negative classes, the fifth on one
  host. **3.1:** V's race data added; stated as underpowered and unable to rank guards.
- **4 (conclusions):** C1 to C12 updated; C13 (`LoadUnit`), C14 (directive set against file readability) and C15 (I5 failed)
  added; 4.2 revised. C3 changed: `systemctl cat` is no longer a second oracle.
- **5 (requirements):** REQ-1 amended (directive set, `systemctl cat` is not a reader); REQ-3 (`LoadUnit`, `GetAll`, no
  `--json`); REQ-6 and 5.2 (mask in `/usr/lib`, `enabled-runtime`); REQ-13 (`a_disk` is the only evidenced oracle, must
  walk directories created after load); REQ-16 (`ssh.socket`); new REQ-22 (sort set-valued properties), REQ-23
  (`LoadUnit`), REQ-24 (journal wording, conditional); 5.6 step 7 (the warning is post-hoc only); 5.11 (test `Result`, not
  text); pseudocode (`LoadUnit`, set sorting, accepted states); a second known hole noted.
- **6 (ideas):** I1 and I5 marked failed with the evidence; I2 and I7 amended; I15 added.
- **7 (tests):** sixteen named tests added for the new findings; fixtures list extended; 7.3 extended.
- **8 (edit list):** items 10, 13, 14 updated; items 15 and 16 added (item 16 only cross-references `02`, which this
  revision does not edit).
- **9 (questions):** Q1 (tested range and recommendation), Q2 (`enabled-runtime`), Q6 amended; Q10 added (helper file
  reads).
- **10 (unproven):** rows updated or added for older versions and 258, D-Bus stability, `a_disk`, sockets, `UnitFileState`,
  persistent `set-property`, the cause of the `cat` and `show` differences, the new-directory miss, restricted helper reads,
  sorting sufficiency, `ssh.socket`.
- **11 (contradictions):** K/O/T/V statements merged; the M20, M14, `noexec` and the two V text discrepancies added.

Unchanged: REQ-2, REQ-4, REQ-5, REQ-7 to REQ-12, REQ-14, REQ-15, REQ-17 to REQ-21, Q3 to Q5, Q7 to Q9, I3, I4, I6, I8 to
I14.
