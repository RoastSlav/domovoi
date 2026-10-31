# P24 — Packaging and deployment

| | |
| --- | --- |
| Phase | 6, Ship |
| Depends on | P23 |
| Crates | `deploy/`, `docker/`, `Dockerfile`, `install.sh`, `.github/workflows/*` |
| Size | M |
| Invariants touched | none new; hardening of the runtime environment |

## Goal

Make installation boring: a verified one-line installer, hardened systemd units for hub and agent, Docker images for the hub and for container-mode agents (Unraid, TrueNAS SCALE, Synology), Compose examples with a Docker socket proxy, and CI jobs that prove the artifacts work.

## Read first

- `ARCHITECTURE.md` §7, §10
- ADR 0008 (release pipeline), the existing `.github/workflows/release.yml`, `Dockerfile`, `docker/Dockerfile.release`

## In scope

### Installer (`install.sh`, POSIX sh, `shellcheck` clean)

- `curl -fsSL https://github.com/OWNER/domovoi/releases/latest/download/install.sh | sh -s -- --role agent` (and `--role hub`, `--version vX.Y.Z`, `--prefix`, `--no-service`).
- Detect `x86_64` or `aarch64`; download the matching archive and `.sha256` from GitHub Releases; verify with `sha256sum -c`; print how to verify the build attestation with `gh attestation verify`.
- Install the binary to `/usr/local/bin/domovoi`, create the `domovoi` system user for the hub, create `/etc/domovoi` (0750) and `/var/lib/domovoi` (0750), copy example configs only if absent, install the matching systemd unit, `systemctl daemon-reload`, and print next steps (edit config, `domovoi check config`, enable the service). Never overwrite existing configs. Idempotent.
- Add `install.sh` to the release assets in `release.yml`.

### systemd units (`deploy/systemd/`)

- `domovoi-hub.service`: `User=domovoi`, `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectHome=yes`, `ReadWritePaths=/var/lib/domovoi`, `PrivateTmp=yes`, `ProtectKernelTunables=yes`, `ProtectKernelModules=yes`, `ProtectControlGroups=yes`, `RestrictSUIDSGID=yes`, `LockPersonality=yes`, `MemoryDenyWriteExecute=yes`, `RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX`, `SystemCallArchitectures=native`, `CapabilityBoundingSet=`, `Restart=on-failure`.
- `domovoi-agent.service`: runs as root (needs SMART, the journal and user switching for skills) but with `ProtectSystem=full`, `ReadWritePaths=/var/lib/domovoi-agent`, `PrivateTmp=yes`, `ProtectKernelModules=yes`, `RestrictSUIDSGID=no` (skills may need it; document why), `Restart=always`. Document the trade-off and how to run it unprivileged with reduced checks.

### Images

- `Dockerfile` (build from source): Node stage builds `web/`; Rust stage (`rust:1-alpine` + `musl-dev`) builds `domovoi --release --features embed-ui`; runtime `gcr.io/distroless/static-debian12:nonroot`; `EXPOSE 7420 7421`; `ENTRYPOINT ["domovoi"]`; `CMD ["hub", "run", "--config", "/etc/domovoi/hub.toml"]`.
- `docker/Dockerfile.release`: same runtime from prebuilt binaries (already used by the release workflow; keep it in sync).
- `docker/Dockerfile.agent`: Alpine runtime with `smartmontools`, `util-linux` and `ca-certificates`, binary from the release build, `ENTRYPOINT ["domovoi", "agent", "run"]`. The release workflow publishes it as `ghcr.io/OWNER/domovoi-agent`.

### Compose examples (`deploy/compose/`)

- `hub.yml`: the hub with a named volume and the config mounted read-only.
- `agent.yml` (container mode): the agent image with `pid: host`, read-only mounts of `/proc` → `/host/proc`, `/sys` → `/host/sys`, `/var/log/journal` and `/run/log/journal`, `/etc/os-release`; `host_root = "/host"` in its config; a `docker-socket-proxy` service (`tecnativa/docker-socket-proxy`) exposing only `CONTAINERS=1`, `EVENTS=1`, `INFO=1`, `IMAGES=1` and, if write skills are wanted, `POST=1` with a warning; SMART needs `--privileged` or device mappings: document both.
- Short notes for Unraid (Community Apps template XML in `deploy/unraid/domovoi-agent.xml`), TrueNAS SCALE (custom app with the compose file) and Synology (Container Manager project).

### CI

- `shellcheck` job for `install.sh` and any `deploy/**/*.sh`.
- `container-smoke` job on PRs touching `Dockerfile*` or `deploy/`: build the source image, run `domovoi version`, start the hub with `examples/hub.toml` (triage disabled) and `curl /healthz`.
- Release workflow: also build and push the agent image; attach `install.sh`.

## Out of scope

- Distribution packages (deb, rpm, AUR, Nix). Note them as future work in the book.

## Tests required

1. `shellcheck` passes.
2. Installer tested in CI inside `ubuntu:24.04` and `debian:12` containers against a locally served fake release (a tiny HTTP server serving a built tarball and checksum): installs, is idempotent, refuses a bad checksum.
3. `container-smoke` passes.
4. `systemd-analyze verify` on both units (in a job with systemd available, or document the manual check if not possible on hosted runners).

## Architecture gates

- `cargo xtask ci` unaffected; `actionlint` clean.

## Acceptance criteria

- `docs/book/src/install/*.md`: binary + systemd, Docker, container-mode agent (with the caveats per NAS platform), upgrading, uninstalling.

## Deliverables

- `install.sh`, `deploy/**`, `docker/Dockerfile.agent`, updated `Dockerfile` and workflows, install docs

## PR

Title: `feat(deploy): verified installer, hardened units, images and compose examples`
