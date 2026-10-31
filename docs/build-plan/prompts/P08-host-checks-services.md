# P08 — Host checks II: containers, units, SMART, security, ports, probes

| | |
| --- | --- |
| Phase | 2, Agent side |
| Depends on | P07 |
| Crates | `domovoi-checks` (features `host`, `net`) |
| Size | L |
| Invariants touched | fingerprint rule (volatile values only in evidence) |

## Goal

Add the service-level checks: Docker containers with learned baselines and criticality, systemd units, SMART with delta tracking, SSH/sudo/fail2ban security events, listening ports, and network probes (TCP, HTTP, default gateway) that the hub also uses.

## Read first

- `ARCHITECTURE.md` §5.4 (baselines), §7.2
- P07 framework
- Lessons that shaped these checks: throwaway `docker run --rm` containers must never become expected; counters alert on increase, not on non-zero; repeated waves of the same event are one incident.

## In scope

Add `bollard`, `reqwest` (rustls-no-provider), `url`. Network probes are behind feature `net`; everything else behind `host`.

### Containers (`containers/`)

- Internal trait `ContainerSource { async fn list(&self) -> Result<Vec<ContainerInfo>>; async fn events_since(&self, since) -> Result<Vec<ContainerEvent>>; }` with a bollard implementation (`DockerSource`, endpoint from config: unix socket or `tcp://` socket proxy) and a fake for tests.
- `ContainerInfo { name, image, image_created, state, health: Option<Health>, restart_count, labels, auto_remove: bool, started_at }`.
- Managed = has `com.docker.compose.project` label (Portainer stacks have it too). Never learn `auto_remove` containers or unmanaged ones.
- Baseline learning in `CheckState`: a managed container seen running in 2 consecutive runs becomes expected; criticality from `[containers] critical = [...]` (names), default warning; entries not seen for `baseline_expiry` (7 d) are dropped and reported as a fact `baseline.expired` for an advisory.
- Check `container.expected_down` (target = container name): expected container missing or not running → severity by criticality.
- Check `container.unhealthy` (target = name): health `unhealthy` → warning.
- Check `container.restart_loop` (target = name): restart count increased by ≥ 3 within 1 h (window in state) → warning; evidence: restarts, window.
- Check `container.new` (target = name): a new unmanaged, non-auto-remove container appears → info, once per container.
- Facts: `containers.expected_running` ("24/24 expected containers running"), per-container `image.age_days` for advisories.
- `events_since` feeds a 24 h ring buffer kept in state for the `DockerEvents` read tool and the triage "what changed" context (P09 exposes it).

### systemd units (`units/`)

- Check `unit.failed`: `systemctl_failed` template (parse `--output=json` when available, plain text otherwise) → one observation per failed unit, warning.
- Check `unit.watch` for units listed in config: `systemctl_show` with `ActiveState,SubState,NRestarts,Result` → not active → warning (critical if configured); `NRestarts` delta ≥ 3/h → restart loop warning.

### SMART (`smart/`)

- Cadence 1 h. `smartctl_scan` then `smartctl_all` per device, JSON output.
- NVMe: `critical_warning != 0` → critical; `media_errors` increase → critical; `percentage_used ≥ 90` → warning; temperature ≥ 70 °C → warning.
- ATA: attributes 5 (reallocated), 197 (pending), 198 (offline uncorrectable), 187 (reported uncorrect), 199 (CRC). Fire on **increase** versus the last acknowledged value in state, never on non-zero alone; evidence includes `value` (the tracked counter), previous, delta, power-on hours, model, temperature. 197 or 198 increasing → critical; 5 or 187 increasing → warning; 199 increasing → warning with "check cable" hint.
- Facts: per device health summary.

### Security (`security/`)

- `security.ssh_login` (target = user): parse `journal_since_cursor` for `sshd`/`ssh` units: `Accepted (password|publickey) for USER from IP port N`. LAN sources (RFC 1918, loopback, link-local, CGNAT 100.64/10, ULA fc00::/7) → info; WAN → warning with evidence ip, method, port.
- `security.sudo_failure` (target = user): `authentication failure` / `incorrect password attempts` → warning.
- `security.fail2ban` (target = jail): optional (config), `fail2ban_status`; bans in the last hour ≥ threshold (default 20) → info; counts are evidence only.
- `security.new_port` (target = `addr:port/proto`): parse `/proc/net/tcp{,6}` and `/proc/net/udp{,6}` for listening sockets; baseline learned like containers; a new non-loopback listener → warning.

### Network probes (`net/`, feature `net`)

- `probe.tcp` (target = `host:port`): connect with 5 s timeout.
- `probe.http` (target = URL): GET with 10 s timeout, expected status set, records latency; TLS certificate days-to-expiry as evidence when HTTPS (< 14 days → warning).
- `probe.gateway`: read default gateway from `/proc/net/route`; TCP connect to gateway ports 53, 80, 443 (any success = reachable). Exposed as a function `gateway_reachable(fs) -> bool` for the hub's self-doubt check (P13).

## Out of scope

- Converting observations to signals and scheduling (P09). Webhook parsers (P14).

## Tests required

1. Containers (fake source): `managed_container_is_learned_after_two_runs`, `auto_remove_and_unmanaged_containers_are_never_learned`, `expected_container_down_uses_configured_criticality`, `restart_loop_fires_on_three_restarts_within_an_hour`, `new_unmanaged_container_reports_once`, `baseline_entry_expires_after_seven_days`.
2. `docker_source_maps_api_responses`: wiremock serving `/containers/json` and `/containers/{id}/json` fixtures.
3. Units: `failed_units_parse_json_and_plain_output`, `watched_unit_restart_loop`.
4. SMART (fixture JSON for one NVMe and one ATA disk): `ata_counters_fire_on_increase_not_on_nonzero`, `pending_sector_increase_is_critical`, `crc_increase_hints_cable`, `nvme_critical_warning_is_critical`, `first_run_records_baseline_without_firing`.
5. Security: `wan_ssh_login_is_warning_and_lan_is_info`, `sudo_failure_detected`, `fail2ban_counts_are_evidence_only`, `new_listening_port_fires_after_baseline`, `ip_classification_table` (v4 and v6 ranges).
6. Probes: `tcp_probe_reports_down_on_refused_connection`, `http_probe_checks_status_and_cert_expiry` (local test server with self-signed cert), `gateway_is_parsed_from_proc_net_route`.
7. Fingerprint rule: for every new check, `targets_never_contain_counts_or_timestamps` (assert targets are drawn from names, devices, users, jails or ports only).

## Architecture gates

- Commands only through the `commands.rs` templates from P07 and the `ProcessExecutor` port; no process spawning in checks.
- `net` probes compile without `host`.

## Acceptance criteria

- `cargo xtask ci` passes.
- `docs/book/src/reference/checks.md` updated with every new check.

## Deliverables

- `crates/domovoi-checks/src/{containers,units,smart,security,net}/*`
- Fixtures under `tests/fixtures/{docker,smart,journal,proc}/`

## PR

Title: `feat(checks): containers, units, SMART, security and network probes`
