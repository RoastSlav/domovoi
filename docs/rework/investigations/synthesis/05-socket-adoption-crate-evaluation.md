# Socket adoption without `unsafe`: crate and approach evaluation

Date of the facts below: 2026-11-22. Crate facts come from the crates.io API and the published crate sources (read, not guessed). Behaviour claims marked "measured" were run in WSL 2 (Ubuntu, systemd 259, `systemd-socket-activate`); everything else is read from source or documentation.

This note answers open question A4 of `docs/adr/0030-three-identities.md` ("how does the process adopt an inherited socket without `unsafe`") and D-6 / section 5.4 item 6 of `02-r3-r9-helper-and-skill-runner-synthesis.md`.

## 1. Constraints from the repository

- `[workspace.lints.rust] unsafe_code = "forbid"` applies to workspace crates only. Dependencies may contain `unsafe`. `deny.toml` has no unsafe check, and neither `xtask` nor `.github/workflows` mentions cargo-geiger or cargo-vet, so a dependency's internal `unsafe` is not policed by CI (a reviewer judgement).
- Edition 2024, `rust-version = "1.94"`. In edition 2024 `std::env::remove_var` and `set_var` are `unsafe fn`, so the workspace crates **cannot unset environment variables at all**.
- R11: versions only in `[workspace.dependencies]`; rustls only.
- `deny.toml` licence allow list: MIT, MIT-0, Apache-2.0 (and with LLVM exception), BSD-2/3, ISC, Zlib, 0BSD, BSL-1.0, CC0-1.0, Unicode-3.0, Unicode-DFS-2016, CDLA-Permissive-2.0, MPL-2.0, PolyForm-Noncommercial-1.0.0. `multiple-versions = "warn"`, `wildcards = "deny"`, `unknown-registry = "deny"`.
- Already in `Cargo.lock`: `nix 0.31.3` (workspace dependency, features `signal process user fs`, used by `domovoi-agent`, `domovoi-runner`, `domovoi-checks`), `rustix 1.1.6` (transitive), `libc 0.2.191`, `cfg-if`, `bitflags 1.3.2` and `2.13.2`, `linux-raw-sys 0.12.1`. Not present: `listenfd`, `sd-notify`, `sd-listen-fds`, `libsystemd`, `systemd`, `tokio-listener`.
- No crate in `crates/` adopts an inherited socket today. The only unix-socket code is `std::os::unix::net` in `crates/domovoi-checks/src/containers/detect.rs` (a client connect and a test listener). Runtime is tokio (`tokio = "1.54.1"`, `full` in `domovoi`).
- The runner's spawn code (`crates/domovoi-runner/src/exec/spawn.rs`) calls `.env_clear().envs(env)` and sets stdin to `Stdio::null()` or a pipe, never inherit. A skill child therefore never inherits `LISTEN_*` or fd 0 from the runner.

The shipped unit shapes in `02` (sections 5.1 and 5.3) both use `Accept=yes` with `StandardInput=socket`: the connection is fd 0, `LISTEN_FDS=0` (F-ACT-2). Neither needs fd 3 or a listening socket.

## 2. Measured facts about the no-dependency routes

Measured with a 20-line std-only probe under `systemd-socket-activate` (not a real unit; see section 8).

| Question | Result |
| --- | --- |
| `File::open("/proc/self/fd/N")` on an inherited unix socket | `ENXIO` ("No such device or address"). A socket cannot be reopened through `/proc`. This route is impossible. |
| `std::io::stdin().as_fd().try_clone_to_owned()` then `UnixStream::from(owned)`, with a socket on fd 0 (`--inetd -a`) | Works with no `unsafe` and no dependency. `local_addr()` and `peer_addr()` succeed, read and write round-trip. `try_clone_to_owned` uses `F_DUPFD_CLOEXEC`, so the new fd is close-on-exec. |
| Same, `UnixListener::from(owned)` with the listening socket on fd 0 (`--inetd`, no `-a`) | Works: `accept()` serves a client. Real systemd with `Accept=no` and `StandardInput=socket` was not tested. |
| Fd 3 (`LISTEN_FDS=1`, `LISTEN_PID` set) | `/proc` open gives `ENXIO`; std offers no safe way to name fd 3 (`BorrowedFd::borrow_raw` and `OwnedFd::from_raw_fd` are `unsafe`). |
| Non-socket on fd 0 | `local_addr()` fails `ENOTSOCK` ("Socket operation on non-socket"), so the check is a free socket-type validation. |
| `systemd-socket-activate --inetd` | sets neither `LISTEN_PID` nor `LISTEN_FDS` (real systemd sets `LISTEN_FDS=0`, F-ACT-2). |

`std::io::Stdin` implements `AsFd` and never closes fd 0, so borrowing it is the one safe way to name an inherited descriptor by number.

## 3. Candidates

"Caller unsafe" means the application would have to write `unsafe` to obtain an owned or listener object.

| Candidate | Version, last release | Owned fd without caller `unsafe` | Internal `unsafe` | LISTEN_PID check | Unsets env | CLOEXEC | Stdin socket (Accept=yes) | Licence vs `deny.toml` | New transitive crates | MSRV | tokio | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| std only (`Stdin::as_fd().try_clone_to_owned()`) | std, rust 1.94 | yes (measured) | none | not applicable to fd 0 | not needed (see 5) | yes (`F_DUPFD_CLOEXEC`) | yes, this is the route | n/a | none | 1.94 | `from_std` after `set_nonblocking` | **fits both units** |
| `sd-listen-fds` | 0.2.0, 2023-08-27; owner Ralith; 168k downloads | yes: `get()` returns `OwnedFd` convertible to `UnixListener`/`UnixStream` | yes: `mem::transmute::<u32, OwnedFd>`; `get()` can be called twice and hands out two owners of one fd (double close), so the safe API is unsound | yes (parse and compare, malformed is an error) | **no** | **no** | no (reads `LISTEN_FDS` only; zero fds returns empty) | MIT OR Apache-2.0, ok | none (no dependencies) | not declared (edition 2021) | std types | no: unmaintained since 2023, unsound by design, no env unset, no CLOEXEC |
| `listenfd` | 1.0.2, 2025-01-19; owner mitsuhiko; 7.5M downloads | yes: `take_unix_listener`, `take_tcp_listener`, `take_custom` | yes: `fstat`, `getsockname`, `getsockopt`, `fcntl`, `from_raw_fd` | **weak**: a missing or empty `LISTEN_PID` is accepted (`Err(NotPresent) \| Ok("")` is treated as ok), so a bare `LISTEN_FDS=1` adopts fd 3 | yes, unconditionally, including on PID mismatch (std `remove_var`, legal in its edition 2018) | yes (`FD_CLOEXEC`) | no | Apache-2.0, ok | `libc` only on unix (already in lockfile); `uuid` and `winapi` only on Windows | not declared (edition 2018) | std types | **best crate if fd 3 is ever needed**; wrap with an own `LISTEN_PID` check |
| `libsystemd` | 0.7.2, 2025-04-30; owner lucab; 10.4M downloads | **no**: `receive_descriptors` yields `FileDescriptor` with only `IntoRawFd` (the open issue "Implement From<FileDescriptor> for OwnedFd" confirms), so the caller needs `from_raw_fd` | yes | yes, unset happens before the check | yes (`unset_env` argument; its own open issue asks to make it `unsafe`) | yes | no | MIT/Apache-2.0, ok | heavy: `nix 0.29` (a second `nix` beside 0.31.3: `multiple-versions` warn), `hmac`, `sha2`, `nom 8`, `serde`, `uuid`, `thiserror 2`, `once_cell`, `log` | 1.69 | std | no: fails the main requirement and adds a duplicate `nix` |
| `systemd` (rust-systemd) | 0.11.1, 2026-10-28; codyps; 6.7M downloads | no: `daemon::listen_fds` yields raw fds | yes, plus FFI to `libsystemd-sys` (needs the C library) | via libsystemd C | via C | via C | no | **LGPL-2.1-or-later WITH GCC-exception-2.0: not in the allow list** | `libsystemd-sys`, `foreign-types`, `utf8-cstr`, `memchr`, `log` | not declared | n/a | no: licence and a C link dependency |
| `sd-notify` | 0.5.0, 2026-03-09; lnicola; 18.4M downloads | **no**: `listen_fds()` returns `RawFd`s | yes | yes | `listen_fds_and_unset_env` is itself `unsafe fn` (edition 2024 `remove_var`), so callable only with caller `unsafe` | yes (sets `O_CLOEXEC`) | no | MIT OR Apache-2.0, ok | `libc` (optional `sendfd`) | 1.82 | n/a | notify only (`READY=1`, `STOPPING=1`); not an adoption crate |
| `rustix` (1.1.6, transitive) | 1.1.6, 2026-10-27; bytecodealliance; 1.45B downloads | no: there is no safe "fd number to owned fd"; `rustix::stdio::take_stdin` and `BorrowedFd::borrow_raw` are `unsafe`. `rustix::stdio::stdin()` and `dup2_stdin` are safe | yes (it is the `unsafe` layer under std-like APIs) | n/a | n/a | via `fcntl_setfd` | only what std already gives | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT, ok | already present | 1.65 | n/a | adds nothing over std for adoption; useful only for `dup2_stdin` (see 6) |
| `nix` (0.31.3 in workspace; 0.32.0 released 2026-11-17) | 0.31.3 | no: no safe fd-number constructor; `dup2_raw`, `dup3_raw` are `unsafe`; `dup2_stdin` and `dup3` take `AsFd` | yes | n/a | n/a | via `fcntl` | only what std already gives | MIT, ok | already a workspace dependency | 1.69 | n/a | no benefit; may supply `dup2_stdin` |
| `tokio-listener` | 0.5.2, 2025-09-11; vi; 119k downloads | n/a: it binds and accepts addresses (`sd-listen` address kind), not an adopted-owned-fd API that bypasses `unsafe` in its dependants; pulls `nix 0.26` (a third `nix`), `socket2`, `pin-project`, `tracing` | yes | its own implementation | not documented | not documented | no | MIT OR Apache-2.0, ok | several, including an old `nix` | not declared | yes | no: wrong layer, more surface than the problem |

Notes on the table:

- "Owned fd without caller `unsafe`" is the hard requirement. Only std (for fd 0), `sd-listen-fds` and `listenfd` meet it. `sd-listen-fds` is rejected for unsoundness and for not unsetting the environment or setting CLOEXEC.
- The workspace policy forbids `unsafe` in the workspace's own source, not in dependencies. Accepting `listenfd` would import a small `unsafe` surface (about six calls, all visible in `src/unix.rs`, 136 lines). No tool in this repository measures it, so the check is a one-time reading of that file at the pinned version.
- Environment unsetting matters because a skill child must not inherit `LISTEN_FDS`. Here the runner already uses `env_clear()` in `spawn.rs`, so the property holds without any crate. A crate that unsets the environment helps only the helper's own later spawns, and the helper has none.

## 4. Rejected approaches

- **Reading the socket through `/proc/self/fd` or a pidfd**: impossible for sockets, measured: `ENXIO` on open. `pidfd_getfd` needs the same `unsafe` or a crate and a second process.
- **A tiny `unsafe` module behind an ADR exception to R7**: possible (`OwnedFd::from_raw_fd(3)` after the `LISTEN_PID` check, about 15 lines) but unnecessary given section 2, and it would break `forbid`, which cannot be locally overridden (`forbid` rejects even `#[allow(unsafe_code)]`), so it would need a separate crate with a weaker lint level. This costs more than a dependency and is the worst of the options.
- **The helper binds its own socket**: loses socket activation, so the service must run permanently as root before any client, and the `SocketUser/Group/Mode` layer measured in F-ACT-4 would move into the helper code (create the node, `chown`, `chmod`, remove stale nodes). It also changes the `Accept=yes` one-process-per-request model in `02` 5.3 that the lock design (0032 D9) assumes. Not recommended.

## 5. Recommendation

**This section is a recommendation, not a decision.**

1. **Adopt fd 0 with std only, no new dependency.** Both shipped unit shapes are `Accept=yes` with `StandardInput=socket`. The process borrows `Stdin`, duplicates it with `try_clone_to_owned()` (close-on-exec), converts the owned fd to `std::os::unix::net::UnixStream`, validates it with `local_addr()` (fails `ENOTSOCK` for anything else), sets non-blocking and converts with `tokio::net::UnixStream::from_std`. No `unsafe`, no `Cargo.toml` change, no ADR exception to R7 or R11.
2. Reject (fail closed) when `LISTEN_FDS` is set and is not `0`: the unit is not the shipped shape, and the process would otherwise silently ignore an inherited listener.
3. **Do not unset `LISTEN_*`.** It cannot be done safely in edition 2024, and it is not needed: the runner spawns with `env_clear()` and stdin null or piped, so no skill child inherits `LISTEN_*` or the connection. Pin that with a test (section 7) rather than with code.
4. **Fallback if a unit ever needs fd 3 (`Accept=no`) or a listener:** `listenfd` 1.0.2, with the application first doing its own strict check that `LISTEN_PID` is present and equals `std::process::id()` and that `LISTEN_FDS` is exactly `1` (because `listenfd` accepts a missing `LISTEN_PID`), then `ListenFd::from_env().take_unix_listener(0)`. Read `src/unix.rs` once before pinning. Reject `sd-listen-fds`, `libsystemd`, `systemd`, `sd-notify` (for adoption), `tokio-listener`.
5. Keep `READY=1` out of scope: neither unit is `Type=notify`. If it ever is, `sd-notify` `notify` (safe, MIT OR Apache-2.0, `libc` only) is the candidate, never `notify_and_unset_env`.

**Main risks of the recommendation**

- The fd-0 route was measured with `systemd-socket-activate --inetd`, not with real units. The socket on fd 0 for `Accept=yes` is also the measured F-ACT-2 shape, but the combination with `StandardOutput=journal` and the `/run` mask is still the unmeasured item already listed in `02` 5.2.
- The original fd 0 stays open next to the duplicate. This is harmless while spawn code keeps stdin non-inherit (verified in `spawn.rs` today) and is why the test in section 7 must exist. Closing it safely needs `rustix::stdio::dup2_stdin` or `nix` `dup2_stdin` onto `/dev/null`; whether the workspace's `nix` feature set includes it was not checked.
- Fd 3 and `Accept=no` are unsupported by the primary route; the fallback adds one small `unsafe`-containing dependency and a custom `LISTEN_PID` check.

## 6. Workspace entry and code sketch

Primary route: no `Cargo.toml` change.

Fallback only (do not add now):

```toml
# root Cargo.toml, [workspace.dependencies]
listenfd = "1.0.2"

# the one crate that needs it
[target.'cfg(unix)'.dependencies]
listenfd = { workspace = true }
```

`deny.toml` needs no edit (Apache-2.0 is allowed, `libc` already present).

Sketch of the primary route (a `Result`-returning function in the helper and runner entry code; error type per R9):

```rust
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;

/// Takes the connection systemd handed to this instance as standard input.
///
/// Fails closed unless the unit is the shipped `Accept=yes` shape.
pub fn adopt_stdin_connection() -> Result<tokio::net::UnixStream, AdoptError> {
    // Not the shipped unit shape: a listener was passed on fd 3, refuse.
    match std::env::var("LISTEN_FDS") {
        Err(std::env::VarError::NotPresent) => {}
        Ok(n) if n == "0" => {}
        _ => return Err(AdoptError::UnexpectedListenFds),
    }
    // Duplicate fd 0 (F_DUPFD_CLOEXEC); std never closes the original.
    let owned = std::io::stdin().as_fd().try_clone_to_owned()?;
    let stream = UnixStream::from(owned);
    // ENOTSOCK for a tty, file or pipe; also rejects a connection from nowhere.
    stream.local_addr().map_err(AdoptError::NotASocket)?;
    stream.set_nonblocking(true)?;
    Ok(tokio::net::UnixStream::from_std(stream)?)
}
```

Notes: never read through `std::io::stdin()` or `tokio::io::stdin()` afterwards; peer identity still comes from `SO_PEERCRED` through the tokio stream (`peer_cred()`), as in `02` section 2.1; `LISTEN_PID` is irrelevant on this path because fd 0 is adopted by its role, not by an advertised range. The runner and the helper both call this function once and own the stream for the whole instance.

Sketch of the fallback (`listenfd`):

```rust
pub fn adopt_listener() -> Result<tokio::net::UnixListener, AdoptError> {
    let pid_ok = std::env::var("LISTEN_PID").ok().and_then(|p| p.parse::<u32>().ok())
        == Some(std::process::id());
    let one_fd = std::env::var("LISTEN_FDS").ok().as_deref() == Some("1");
    if !(pid_ok && one_fd) {
        return Err(AdoptError::NotActivated);
    }
    // validates the type, sets FD_CLOEXEC, unsets LISTEN_PID and LISTEN_FDS
    let std_listener = listenfd::ListenFd::from_env()
        .take_unix_listener(0)?
        .ok_or(AdoptError::NotActivated)?;
    std_listener.set_nonblocking(true)?;
    Ok(tokio::net::UnixListener::from_std(std_listener)?)
}
```

`listenfd` does not check `SO_ACCEPTCONN` (that the socket listens); the first `accept()` reports `EINVAL` for a connected socket.

## 7. Test plan

Unit (no systemd, no unsafe, fake environment passed as a parameter so tests do not call `set_var`, which is `unsafe` in edition 2024; split the function into a pure `check_listen_env(fds: Option<&str>)` and the fd work):

- `refuses_listen_fds_greater_than_zero`, `accepts_listen_fds_unset_or_zero`, `refuses_malformed_listen_fds`.
- `adopts_a_connected_unix_socket_as_stdin` against a `UnixStream::pair()` duplicated onto the checked descriptor path (the descriptor-taking inner function takes `BorrowedFd`, tested with the pair's fd; the stdin wrapper is covered by the integration test below).
- `refuses_a_non_socket_descriptor` (a pipe or `/dev/null` `File`) returns `NotASocket`.
- `adopted_stream_is_close_on_exec` (check `fcntl_getfd` through `nix`, already a workspace dev dependency in `crates/domovoi`, behind `cfg(unix)`).
- Runner: `spawned_skill_gets_no_listen_vars_and_no_connection` (extend the existing spawn tests: parent environment containing `LISTEN_FDS`, `LISTEN_PID`; the child prints its environment and `ls /proc/self/fd`; assert no `LISTEN_*`, fd 0 is null or the pipe, no socket fd). This pins the property that replaces the environment unset.
- If the fallback is ever adopted: `listenfd_route_refuses_missing_listen_pid` (the application-side check, because the crate alone would accept it) and `refuses_listen_pid_of_another_process`.

Integration (Linux, WSL or CI, `#![cfg(unix)]` like `tests/single_box.rs`):

- `systemd-socket-activate --inetd -a -l <tmp>.sock <binary> skill-runner` with a Python or Rust client: one request, one reply, EOF cancellation, exit 0. This was exercised here with a std probe and works.
- Real unit: `systemd-run --user --socket-property=ListenStream=<path> --property=StandardInput=socket --property=Accept=yes ...` is not documented for `--socket-property` together with `Accept=`; write the transient pair as unit files in `$XDG_RUNTIME_DIR/systemd/user/` instead and `systemctl --user start` the socket. Cases: connection works, `LISTEN_FDS=0` reaches the process, journal logging while stdout is not the connection (the item still open in `02` 5.2), the `/run` mask case.
- The root helper unit can only be exercised in system scope on a host with the two real users already used by the earlier measurements; keep it as the existing `M`-style harness, not CI.

## 8. Consequences for the ADRs and documents

- `docs/adr/0030-three-identities.md`, open question A4: resolve as "the connection is adopted from standard input with std only (`Stdin::as_fd().try_clone_to_owned()`); no wrapper crate, no exception to R7 or R11; a listener on fd 3 needs a separate decision and, if taken, `listenfd` 1.0.2 behind a strict `LISTEN_PID` check". Update the A4 row and the "check of the workspace crates" cell.
- `02-r3-r9-helper-and-skill-runner-synthesis.md`: section 5.4 item 6 ("Adopting a descriptor without `unsafe` is open") becomes a pointer to this note; D-6 gets the same resolution. Add `runner_adopts_stdin_connection_without_unsafe` and the spawn-environment test above to the section 7.1 test list.
- Section 5.1 and 5.3 unit text needs no change (`Accept=yes`, `StandardInput=socket` are what the route requires). Add a comment that `Accept=no` would need the fd 3 fallback.
- `ARCHITECTURE.md` R7 and R11 are untouched. If the fallback is later taken, `listenfd` is the one new entry under `[workspace.dependencies]`, and the dependency review notes its `unsafe` is the `fstat`/`getsockopt`/`fcntl`/`from_raw_fd` set in `src/unix.rs`.

## 9. What could not be verified

- Real systemd units (not `systemd-socket-activate`): fd 0 as the connection with `Accept=yes`, and whether `Accept=no` with `StandardInput=socket` gives the listening socket on fd 0. The latter worked only with `systemd-socket-activate --inetd`.
- The exact value of `LISTEN_PID` under real systemd for `StandardInput=socket` instances (F-ACT-2 records `LISTEN_FDS=0`); the recommendation does not depend on it.
- MSRV of `listenfd`, `sd-listen-fds` (neither declares one; edition 2018 and 2021 code builds on 1.94) and a compile of `listenfd` 1.0.2 inside this workspace (not built).
- Whether `nix` `dup2_stdin` is reachable with the workspace's current `nix` features, and `rustix` signatures beyond the docs.rs module listing (only the module page was read; the safe or unsafe status of `dup2_stdin`, `take_stdin` follows the module's marking).
- `cargo deny` was not run against a changed lockfile: no change is proposed for the primary route; for the fallback, the licence and duplicate-version outcomes above are read from crate metadata.
- Download counts and last-push dates are a snapshot of 2026-11-22.
