# 11 — Capability catalogue and opportunity map

This is an intentionally broad inventory of things Domovoi could eventually observe, diagnose, explain or operate. It is not a v1.0 commitment. A listed item becomes a real capability only when there is a hub-owned typed contract, an implementation, clear scope, policy, verification and tests.

**Priority shorthand:** `v0.1` = exists in the current product (there is no separate v0.1 release; owner decision D-01); `v1` = initial adaptive operator; `v1.x` = useful expansion; `later` = scale/niche/research. `R` = read, `W` = bounded write, `S` = security/protective (the **Contain** effect class), `D` = destructive/high-impact. A risk label is indicative; actual policy must consider context, and the hub, not a provider, owns the effect class (RW-07).

## How to read this catalogue after the rework

### The v1 first set

Most `v1` rows are read capabilities and are part of the initial supported set in `10`. The **mutating and containing** capabilities that ship in v1 are deliberately few. Marked below with **first set**; everything else that mutates or contains is `v1.x` or later.

| Capability | Class | Milestone | Status |
|---|---|---|---|
| Restart one named container (not recreate) | W | M8, L2 | First set. Goes through the root-owned first-party helper's `docker.write` group, which is restart by allowlisted container id only (H1, owner decisions D-24, D-29); unavailable for unattended use until the helper and its ADR land, and the group needs explicit owner configuration (a fresh install enables read groups only). |
| Restart one named systemd unit | W | M8, L2 | First set. |
| Temporary inbound block of one external address or small prefix (`firewall.block_source`, kernel-enforced timeout, 30 s two-step confirmation) | S | M9, L3 | **First set, and the only v1 containment capability** (owner decisions D-17, D-22). |
| Pause one named container with a time limit (`container.pause`) | S | v1.x at earliest | **DEFERRED candidate (D-17).** An agent timer is not reliable automatic expiry. The v1 helper has no `docker.pause` or `docker.unpause`; reconsidering needs the `docs/rework/04 §9.3` checklist and a helper ADR adding a typed operation with independent expiry. |
| Stop one named systemd unit with a time limit (`unit.stop_temporary`) | S | v1.x at earliest | **DEFERRED candidate.** Same reason. Needs the `docs/rework/04 §9.3` checklist and a protected-unit list. |

`container.pause`, `unit.stop_temporary` and the other containment candidates (container network isolation, SSH session termination, account lock) are DEFERRED: weaker recovery or lockout stories, or an expiry that would depend on the agent (`docs/rework/06` "Deferred and after-v1"; `docs/rework/04 §9.3`). The v1 helper has no pause, unpause or unit-stop operation and never a generic `systemd-run`, `run_as` or command execution. Unattended containment on a host also requires owner-confirmed, current management paths that a verified helper attestation shows covered (owner decision D-28, RW-37). Automatic containment is triggered only by a deterministic detector's typed signal naming a target that is the operation's pinned subject, never by free-form model output (owner decision D-04, RW-13). Destructive entries are never policy-allowed in v1 (owner decision D-06); a human can still approve one with typed confirmation.

### Entries flagged unsafe until a prerequisite lands

The repository investigation ([`docs/rework/investigations/D-protocol.md`](../rework/investigations/D-protocol.md)) judged the following entries unsafe or contradictory with current invariants. Each is marked `[FLAG]` in its row. They stay in the catalogue as ideas; none may be built before its prerequisite.

| Flag | Entries | Why | Prerequisite |
|---|---|---|---|
| **Self-upgrade** | Agent and hub upgrade/restart; self-upgrade plan. | Pushes executable code to a host, contradicting the invariant that the hub cannot push scripts (INV-3) and that the hub never executes commands; restarting the agent mid-action destroys the channel its result travels on. Signed capability packages (owner decision D-11) are for capabilities, not for upgrading the agent or hub. | DEFERRED, not planned (owner decision D-20): upgrades follow the install procedure, hub first and then agents, and a mismatched agent shows as `Incompatible`. Changing the no-push rule would need its own ADR. |
| **Lost result** | Host reboot/shutdown, update-then-reboot, power cycle, IPMI/PDU control, UPS-driven shutdown, cloud instance reboot, reboot coordination. | The connection drops mid-action by design. | `OutcomeUnknown` handling and boot-id based reconciliation (RW-09), plus a dependency model that knows what powers the hub. |
| **Docker socket write** | Pull/redeploy/rollback, container stop, pause, network disconnect, container isolation, restart/recreate. | Write access to the Docker socket is root on the host, so the existing read-only rationale and the automatic default no longer hold. | The narrow root-owned first-party helper with its own ADR (owner decisions D-24, D-29, RW-39); the agent runs unprivileged and gets no general Docker socket write. In v1 `docker.write` is restart by allowlisted container id only: stop, pause, network disconnect, isolation and recreate are not helper operations. Docker write capabilities stay unavailable for unattended use until then. |
| **Secret handling** | Rotate an API key, certificate or credential; disable an account; revoke a token; rotate VPN keys. | No secret store or redaction design exists for values that must be generated, delivered and replaced; secret handling contradicts the redaction invariant until one does. | A secret-store design (not planned for v1). |

The investigation also flagged file and volume destruction (never a typed-path capability without a path-scope model), third-party deployment and smart-home entries (outside the overseer's trust envelope until third-party trust is decided), message retry and failover (need real idempotency keys), and hub-side router/hypervisor/cloud providers (a different security design that holds third-party credentials). Treat those as non-goals for v1.

## 1. Generic Linux host operations

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Host identity, OS, kernel, architecture, uptime, boot ID, CPU/memory inventory | R | v0.1 | Foundation for profiles and platform compatibility. |
| CPU, load, memory, swap, pressure stall information, process count | R | v0.1/v1 | Time-series and trend context later. |
| Process list, resource-heavy processes, process tree, open files/sockets | R | v0.1/v1 | Sensitive command-line arguments must be redacted. |
| Systemd unit list/status/show/dependencies/timers | R | v0.1/v1 | Include enabled, active, failed and restart history. |
| Journal and application logs; bounded query by unit/time/severity | R | v0.1/v1 | Prefer structured log providers; strict size/time limits. |
| Filesystem usage, inode usage, largest directories, mount table, mount options | R | v0.1/v1 | Directory scans must have path allowlists, depth and time caps. |
| Kernel messages, OOM events, hardware errors, reboot-required state | R | v0.1 | Current checks provide a base. |
| Network interfaces, addresses, routes, neighbors, DNS config and resolver status | R | v1 | Do not expose secrets from network config. |
| Listening sockets, connection counts, firewall state and packet counters | R | v0.1/v1 | Firewall changes are separate privileged capabilities. |
| Package inventory, repository status, pending updates, held packages | R | v1 | Cross-distribution providers. |
| Controlled package update, rollback and reboot orchestration | W | v1.x | Approval by default; maintenance windows and snapshot/backup prerequisites. **[FLAG: lost result]** the reboot step needs `OutcomeUnknown` handling. |
| Time sync, NTP offset, clock drift, timezone | R/W | v1 | Time drift affects TLS and offline authorization. |
| Users, groups, sudoers summary, login sessions, SSH config summary | R | v1 | Never return password hashes or private keys. |
| Host reboot/shutdown | W | v1.x | Human approval; recovery path and dependency impact mandatory. **[FLAG: lost result]** the link drops mid-action; needs `OutcomeUnknown` and boot-id reconciliation first. |
| Agent self-diagnostics, version, config validation, queue/spool state | R | v0.1/v1 | Domovoi must be able to diagnose itself. |
| Agent upgrade/restart and hub upgrade/restart | W | later | **[FLAG: self-upgrade]** contradicts INV-3 (the hub cannot push scripts) and destroys the result channel. DEFERRED, not planned (owner decision D-20): upgrades follow the install procedure, hub first, then agents. Not part of the capability-package install path. |

## 2. Service and workload management

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Discover services and relationships (systemd, container, supervisor) | R | v0.1/v1 | Feed the entity graph. |
| Restart one named systemd unit | W | v1 | **First set (M8, L2)** only for opted-in, low-impact units; dependency-aware. |
| Stop one named systemd unit temporarily (time limit, management units excluded) | S | v1.x | **DEFERRED candidate** (`unit.stop_temporary`): its only expiry would be an agent timer. See the `docs/rework/04 §9.3` checklist. |
| Start, stop or reload other systemd units | W | v1.x | Human approval initially; no time-limit-free unattended stop. |
| Inspect service dependencies and reverse dependants | R | v1 | Essential before planned restarts. |
| Compare service config fingerprints/mtime and known-good baseline | R | v1.x | Avoid storing secret config contents. |
| Validate service config before reload | R | v1.x | Use native validation commands with fixed argv. |
| Schedule service restart in maintenance window | W | v1.x | Delay/recheck incident state before dispatch. |
| Detect crash loops, restart storms and failed health checks | R | v0.1/v1 | Suppress destructive remediation loops. |
| Supervisor, cron and systemd timer inventory/status | R | v1 | Later support for other service managers. |

## 3. Containers and orchestration

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Docker container list, inspect, logs, events, stats, health and restart history | R | v0.1 | Current foundation. |
| Docker Compose project/stack inventory and service dependencies | R | v1 | Link containers to projects and volumes. |
| Restart one selected container | W | v1 | **First set (M8, L2).** Verify health and dependent services. **[FLAG: Docker socket write]** needs the root-owned helper (`docker.write`, restart only) and its ADR first (owner decisions D-24, D-29). Recreate is later. |
| Start/stop a Compose service or restart a selected stack | W | v1.x | Approval initially; concurrency and downtime constraints. |
| Compose config validation and rendered diff | R | v1.x | Avoid exposing secrets in rendered config. |
| Image inventory, digest drift, outdated image and vulnerability metadata | R | v1.x | Scanning can be an external provider. |
| Pull image, redeploy service, rollback to previous digest | W | v1.x | Human approval by default, known-good image required for rollback. **[FLAG: Docker socket write]** |
| Docker network/volume inventory and dangling-resource reporting | R | v1 | Never infer that unused means safe to delete. |
| Pause one named container with a time limit | S | v1.x | **DEFERRED candidate** (`container.pause`, D-17): its expiry would depend on the agent, and the v1 helper has no pause operation (needs a helper ADR). **[FLAG: Docker socket write]** |
| Container isolation, stop or network disconnect | S/W | v1.x | Security action with scope, TTL/recovery and owner policy. Deferred: weaker lockout/recovery story. **[FLAG: Docker socket write]** |
| Podman and containerd providers | R/W | v1.x | Reuse the container capability family. |
| Kubernetes cluster/node/pod/deployment/service/ingress inventory | R | later | Need cluster and namespace scope. |
| Kubernetes rollout status, restart deployment, scale replicas | W | later | Requires workload ownership and availability budgets. |
| Kubernetes network policy / isolate workload | S | later | Requires cluster-specific safety model. |
| k3s and lightweight cluster operations | R/W | v1.x/later | Follow Kubernetes provider contracts. |

## 4. Storage, filesystems and data protection

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Block devices, SMART/NVMe health, wear, errors, temperature | R | v0.1 | Current SMART base; trend remaining life cautiously. |
| Mounts, filesystem type, capacity, inode and read-only state | R | v1 | Link path to device, pool and host. |
| RAID/ZFS/Btrfs/mdraid/LVM pool and volume health | R | v1.x | Provider-specific health states and supported repairs. |
| SnapRAID status, scrub age, sync state and parity health | R | v1.x | Homelab-specific, valuable as optional provider. |
| MergerFS pool composition, branch capacity and policy | R | v1.x | Detect uneven capacity and full branches. |
| Disk SMART short/long self-test scheduling | W | v1.x | Schedule carefully; avoid concurrent tests. |
| Scrub, resilver, balance, repair and pool expansion | W | later | High impact; approval, health prerequisites and recovery plan. |
| Snapshot inventory and snapshot creation | R/W | v1.x | Keep retention and storage capacity limits. |
| Backup job status, last success, duration, bytes and destination health | R | v1 | Include freshness and source/destination relationship. |
| Backup verification, repository check and sample restore test | R/W | v1.x | A verified restore is stronger than a successful backup exit code. |
| Restore a selected file/directory to an alternate path | W | v1.x | Avoid overwriting the live source by default. |
| Restore an entire host/service from backup | W | later | Guided human-approved disaster recovery. |
| Backup policy coverage and RPO/RTO compliance reporting | R | v1.x | Requires owner-provided importance and target objectives. |
| Safe cleanup candidates (cache/temp/log retention) | R | v1 | Report candidates first; never broad recursive deletion. |
| File deletion, volume destruction, formatting, backup pruning | D | later/deny-default | Never policy-allowed in v1 (owner decision D-06). Human approval with typed confirmation only. A generic typed-path deletion capability would need a path-scope model; prefer documented manual runbooks. |

## 5. Network and connectivity

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Ping/TCP/HTTP/HTTPS and gateway probes | R | v0.1 | Current checks/probes. |
| DNS lookup, resolver chain, authoritative response, reverse lookup | R | v1 | Check split DNS and local resolvers. |
| Route tracing, MTU/path diagnostics, packet loss and latency | R | v1.x | Bound output and collection time. |
| TLS certificate expiry, chain validity, hostname mismatch and issuer | R | v1 | Certificate context should link to endpoint and service. |
| Port exposure and listener changes versus baseline | R | v0.1/v1 | Current detection should evolve with asset context. |
| Network interface, bond, VLAN, bridge and virtual network inventory | R | v1.x | Include hypervisor/container relationships. |
| Reverse proxy route/health/config status (Nginx, Traefik, Caddy, NPM) | R | v1.x | Start read-only; config validation before reload. |
| Reload/restart reverse proxy after validated config | W | v1.x | Verify routes and management access. |
| VPN tunnel and peer health (WireGuard, Tailscale, OpenVPN) | R | v1.x | Know whether the tunnel is a management path. |
| Add/remove or rotate VPN peers/keys | W | later | Approval and identity/recovery model required. **[FLAG: secret handling]** |
| Router inventory, WAN status, routes, DHCP, DNS and port forwards | R | v1.x | API provider for selected router vendor. |
| Temporary block of a confirmed external source IP | S | v1 limited | **First set (M9, L3):** `firewall.block_source`, with a kernel-enforced timeout and a 30 s two-step confirmation (an unconfirmed block expires by itself). Only with owner-confirmed, current management paths covered by a verified, fresh attestation of the helper's effective set (owner decision D-28, RW-37; research R-10), helper-enforced exclusions, rate limits and `OutcomeUnknown` handling, applied through the helper's `nft` group (not production-ready until R-2 and R-10), because the agent dials out and a rule can sever its own control link. Triggered only by a deterministic detector's typed signal. |
| Remove/restrict a port forward or WAN exposure | S/W | v1.x | Higher blast radius; stage and verify alternate access. |
| Firewall rule changes, network segmentation and VLAN changes | W/S | later | Human approval until robust simulation and rollback exist. |
| ISP/WAN outage diagnosis and failover status | R/W | later | Depends on router dual-WAN support. |
| Wi-Fi AP/controller health and client inventory | R | later | Ubiquiti/UniFi and other ecosystems. |

## 6. Defensive security

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| SSH auth failures, successful remote logins and brute-force trends | R | v0.1 | Existing check foundation. |
| Sudo/auth failures, new users/groups and privilege changes | R | v0.1/v1 | Distinguish normal admin work from suspicious access. |
| Fail2ban status, jail health and ban list | R | v0.1/v1 | Existing check foundation. |
| Unexpected listeners, new public ports and exposure drift | R | v0.1/v1 | Correlate with service owner and ingress path. |
| Package/image vulnerability findings | R | v1.x | Integrate scanners; explain exploitability carefully. |
| File integrity / critical config drift | R | v1.x | Hash important files; do not collect secrets. |
| Suspicious process, persistence, cron/service and startup change | R | v1.x | Heuristic findings require evidence and uncertainty labels. |
| Secret leak detection in logs/config/repository | R | v1.x | Never echo the secret value; show location and type. |
| Container privilege/capability/mount exposure audit | R | v1.x | Policy recommendations, not automatic hardening at first. |
| Temporary block offending external IP at host firewall | S | v1 limited | **First set (M9, L3)**, same capability as above. Must exclude trusted ranges, gateway, VPN and current management session. |
| Add temporary Fail2ban jail/ban with expiry | S | v1.x | Prefer native expiry and verify the rule. |
| Isolate one compromised container/network namespace | S | v1.x | Preserve forensic evidence and dependencies. Deferred containment candidate. **[FLAG: Docker socket write]** |
| Disable compromised account or revoke session/token | S/W | later | Requires identity provider integration, owner controls and recovery. |
| Rotate exposed API key/certificate/credential | W/S | later | Secret handling and dependent service reload make this high impact. **[FLAG: secret handling]** no secret store design exists. |
| Restrict WAN SSH to VPN/trusted IPs | S/W | later | Safe only with tested alternate management route. |
| Incident evidence collection and forensic snapshot | R/W | v1.x | Preserve bounded, integrity-protected artifacts. |
| Host isolation / quarantine | S | later | Emergency response with explicit break-glass recovery. |
| Automatic patch installation or vulnerability remediation | W | later | Separate detect, stage, approve, apply and verify. |

## 7. Updates, maintenance and lifecycle

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Pending OS/package updates and security updates | R | v1 | Group by host/service and reboot need. |
| Container image update availability and changelog/release info | R | v1.x | Pin digest, don't blindly use latest. |
| Maintenance calendar and maintenance-window awareness | R/W | v1 | Avoid planned changes during backup or active workloads. |
| Pre-maintenance health and dependency check | R | v1 | Block/ask when prerequisites are unknown or unhealthy. |
| Create backup/snapshot before a supported update | W | v1.x | Verify backup is usable, not just scheduled. |
| Stage an update and show a diff/plan | W | v1.x | Human approval for applying. |
| Apply updates, restart affected services and verify | W | later/v1.x | Explicit host policy, rollback and outage budget. |
| Reboot coordination across dependent hosts | W | later | Respect quorum and criticality. **[FLAG: lost result]** |
| Certificate renew and service reload | W | v1.x | Good L2 candidate when provider is deterministic. |
| Log rotation and bounded cache cleanup | W | v1.x | Exact age/path/size constraints, never broad delete. |
| Retire/decommission host or service | W/D | later | Must preserve dependencies, secrets and backups. |

## 8. Databases, caches and stateful services

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Database endpoint health, replication, connections, locks and storage | R | v1.x | Begin with health/observability. |
| PostgreSQL, MySQL/MariaDB, Redis, SQLite service diagnostics | R | v1.x | Separate read/query credentials from admin credentials. |
| Slow query and lock analysis with redaction | R | later | Query text may contain sensitive data. |
| Controlled service failover or replica promotion | W | later | Requires topology, quorum and recovery semantics. |
| Backup and restore validation for database | W | v1.x | Restore into isolated target by default. |
| Vacuum/compaction/maintenance operations | W | later | May affect latency and storage. |
| Cache flush or key eviction | W/D | later | Explicit namespaces and impact preview; no broad clear by default. |

## 9. Common self-hosted and homelab applications

These should be application profiles layered over generic capabilities. Do not create a different orchestration engine for each app.

| Application/domain | Read capabilities | Possible operations | Priority |
|---|---|---|---|
| Plex / Jellyfin / Emby | service health, libraries, transcode/session state, logs, disk access | restart service, refresh metadata/task queue | v1.x |
| Sonarr / Radarr / Lidarr / Readarr | API health, queue, download client, root paths, indexer health | retry/import/refresh queue actions, restart service | v1.x |
| Prowlarr / Bazarr / Overseerr | API health, indexer/subtitle/request status, logs | refresh indexers, retry metadata/subtitle task | later |
| qBittorrent / Transmission | queue, connection, tracker, disk and download path health | pause/resume selected job, restart service | v1.x |
| Minecraft / Vintage Story / Project Zomboid servers | process, player count, world save health, logs, version | announce, graceful save/stop/restart, backup world | v1.x |
| Home Assistant | API health, integrations, automations and logs | reload selected integration/automation, restart with approval | later |
| Nextcloud / Immich / photo/file services | health, background jobs, storage, database and queue | restart worker, requeue bounded task | later |
| Git forge / CI runners | runner health, queue, webhook delivery, disk | restart runner, drain queue, scale workers | later |
| Reverse proxy / DNS ad blocker | routes, upstream health, certs, DNS query errors | validated reload, temporary route disable | v1.x |
| Download/indexer/media stack as a dependency group | end-to-end data flow across services | investigate and repair the failing link in the chain | v1.x |

## 10. Local AI and GPU workloads

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| GPU inventory, driver, temperature, utilization, memory and power | R | v1 | NVIDIA first; abstract provider interface. |
| Inference server health (Ollama, llama.cpp server, vLLM, LM Studio) | R | v1.x | Endpoint health, loaded model and active requests. |
| Model inventory, model metadata and disk/VRAM footprint | R | v1.x | Do not expose private prompt content. |
| Active model/session/queue and saturation | R | v1.x | Context for overload and latency incidents. |
| Unload an unused model | W | v1 | Possible L2 candidate with active-session check. |
| Restart inference service | W | v1 | Possible L2 candidate with request drain and health check. |
| Change model/quantization/parallelism settings | W | later | High impact; human approval and benchmark verification. |
| Download/cache model | W | later | Disk capacity, provenance and checksum validation. |
| Delete models/cache | D | later/deny-default | Do not grant autonomy by default. |
| Schedule model loads based on demand | W | later | Avoid oscillation and ensure adequate VRAM. |
| Benchmark throughput/latency and recommend model placement | R | later | Keep recommendations separate from policy changes. |

## 11. Virtualization, hardware and power

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Hypervisor and VM inventory, resource, state and host pressure | R | later |
| Start/stop/restart VM; snapshots and controlled migration | W | later | Respect service dependencies and storage. |
| Proxmox/Libvirt API integration | R/W | v1.x/later | Start read-only. |
| IPMI/Redfish power, temperature, fan, PSU and sensor health | R | later |
| Remote power cycle or reset | W | later | Break-glass only, explicit owner authorization. **[FLAG: lost result]** needs a model of what powers the hub. |
| UPS/NUT status, battery runtime, load and test | R | v1.x |
| Graceful shutdown on low runtime / power loss | W | v1.x | Policy plus dependency-aware shutdown order. **[FLAG: lost result]** |
| Smart PDU outlet status/control | R/W | later | Risk of cutting off networking or hub. |

## 12. External systems and integration providers

Hub-side integrations and providers (router, hypervisor, NAS, cloud) are DEFERRED: each needs hub-held credentials and its own contracts (`docs/rework/06` "Deferred and after-v1"). The rows below are an idea inventory, not a v1 commitment.

| Provider family | Useful capabilities | Priority |
|---|---|---|
| Alertmanager, Uptime Kuma, generic webhook | receive signals, correlate and enrich alert | v0.1 |
| Prometheus-compatible metrics | query metrics, compare baseline, collect alert evidence | v1.x |
| Grafana | dashboard/link context, alert state, annotation | later |
| Docker API / Compose | container and stack lifecycle | v0.1/v1 |
| systemd / D-Bus | service manager capabilities | v0.1/v1 |
| Nginx Proxy Manager, Nginx, Traefik, Caddy | routes, certs, upstream and validated reload | v1.x |
| OPNsense, pfSense, MikroTik, UniFi, OpenWrt | network topology, firewall, DHCP, DNS and containment | v1.x/later |
| WireGuard, Tailscale, OpenVPN | tunnel/peer health and scoped management | v1.x |
| Proxmox, Libvirt, TrueNAS, Synology, QNAP | host/VM/NAS/storage inventory and operations | later/v1.x |
| Restic, Borg, Kopia, Proxmox Backup Server | backup jobs, repository health, verification and restore | v1.x |
| GitHub / GitLab | CI run status, repository release, deploy metadata, issue/PR linkage | later |
| Cloud APIs | inventory, health, budget and selected operations | v2+ |
| Home Assistant | state and service health, selected home automation | later |
| Email/chat systems | notifications, interaction, user approvals/questions | v0.1/v1 |

## 13. Domovoi's own operations (self-observability)

- Hub health, memory/CPU/DB size, task queue, scheduler lag and request latency.
- Agent heartbeat, spool size, capability inventory drift and local policy version.
- LLM provider health, latency, token usage, budget and fallback frequency.
- Failed/redacted tool calls, context size, phase duration and cycle count.
- Database backup age, integrity-check status, migration version and restore test.
- Action queue, outstanding approvals, policy denials and expired permits.
- Notification delivery failure and webhook signature failure.
- Config validation, rollback and post-upgrade self-test. Self-upgrade itself is **[FLAG: self-upgrade]**: out-of-band only until an ADR designs signed host-pinned artifacts.
- Detect a misbehaving agent/provider and automatically disable its capability from discovery (not silently alter its owner policy).

## 14. Knowledge and experience capabilities

- Discover entities from agent inventory, provider APIs, configs and user onboarding.
- Propose facts and relationships with source, confidence, timestamp and freshness.
- Confirm, correct, reject, supersede, expire or delete facts.
- Explain why a fact is believed and show conflicting sources.
- Link service-to-host, container-to-volume, endpoint-to-certificate, service-to-database, backup-to-source, host-to-router and service-to-dependency.
- Search exact values, text, host scope, tags, incident history and graph relationships.
- Search experience by symptom, capability, host, time, policy decision, outcome and failed verification.
- Suggest reusable runbook notes, checks, capability gaps and onboarding questions.
- Import existing notes and profile data without silently elevating inferred data to user-confirmed knowledge.

## 15. Orchestration and user interaction capabilities

- Ask a focused user question when the missing answer changes risk or likely diagnosis.
- Request additional data from the relevant host/provider, with a clear reason and budget.
- Run parallel independent read operations; serialize conflicting mutations.
- Build a dependency-aware plan with explicit preconditions and postconditions.
- Pause a plan for approval, user input, maintenance window or external dependency.
- Resume without losing phase/cycle state; revalidate stale evidence and policy.
- Cancel, amend or reprioritize a plan when new evidence arrives.
- Verify immediate outcome and monitor for recurrence over a configured window.
- Escalate with the exact blocker and a safe manual recovery path.
- Notify via incident thread, digest, webhook and future chat providers.
- Render a human-readable explanation of what is known, uncertain, planned, authorized and completed.

## 16. Cloud and remote infrastructure

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---:|---|
| Cloud account/project/region inventory and asset discovery | R | later | Read-only first; scope credentials to selected accounts. |
| VM, instance, disk, load balancer and network health | R | later | Link cloud resources to services and owners. |
| Cloud metrics, logs, events, audit events and service health | R | later | Add provider-specific evidence adapters. |
| Cloud budget, quota, usage and spend anomaly detection | R | later | Do not autonomously scale cost-bearing resources by default. |
| Start/stop/reboot a selected instance | W | later | Approval, dependency and cost controls. **[FLAG: lost result]** for reboot; needs hub-side provider design with third-party credentials. |
| Resize instance or managed database | W | later | High-impact operation with cost and downtime preview. |
| Manage security group / firewall ingress | S/W | later | Same trusted-path and temporary-rule safety model. |
| Object storage inventory, capacity and lifecycle policy status | R | later | Avoid collecting object contents. |
| Managed database health, backup, replica and failover status | R | later | Start with health and backup verification. |
| Cloud DNS and certificate state | R/W | later | Validate record ownership and TTL/propagation. |
| Cross-region backup and disaster-recovery tests | R/W | v2+ | Require explicit RPO/RTO and cost ceilings. |

## 17. Developer platforms and delivery pipelines

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| GitHub/GitLab repository, branch, release and deployment inventory | R | later |
| CI workflow status, queued jobs, failed steps and runner health | R | v1.x |
| Link incident start to recent commits, PRs, deployments and config changes | R | v1.x | Strong change-correlation source. |
| Re-run a failed workflow or selected safe CI job | W | later | Must distinguish idempotent tests from deployment workflows. |
| Drain/restart/scale a self-hosted runner | W | later | Preserve queued jobs and avoid interrupting active deployments. |
| Build image and compare digest/SBOM before deploy | R/W | later | Build is not the same as deployment authorization. |
| Deploy a selected version to a selected environment | W | later | Require environment policy, health checks and rollback. |
| Roll back deployment to last known-good version | W | later | Verify compatibility with schema/data migrations. |
| Detect config drift between Git and live host | R | v1.x |
| GitOps status and reconciliation health | R | later | Avoid fighting the controller by changing live resources directly. |
| Create issue/PR with incident evidence or a proposed fix | W | later | Human-visible draft first; never commit secrets or raw logs. |
| Release-note/changelog summaries and dependency risk summaries | R | later | Model-generated summaries must be sourced and reviewable. |

## 18. Identity, secrets and access infrastructure

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Identity provider availability and authentication error diagnostics | R | later |
| User, group, role and session inventory (metadata only) | R | later | Never retrieve password hashes or session secrets. |
| Certificate authority, ACME account and certificate lifecycle status | R | v1.x |
| Secret manager health, secret version age and rotation status | R | later | Do not fetch secret values into model context. |
| Detect overbroad or expiring API tokens and SSH keys | R | later | Report fingerprint/owner/scope, not private material. |
| Revoke selected token or SSH key | S/W | later | High impact; require identity confirmation and recovery. |
| Rotate a service credential and update dependent services | W | later | Multi-step operation with staged rollback and secret-safe execution. **[FLAG: secret handling]** no secret store design exists. |
| SSO/OIDC/SAML integration for Domovoi users | R/W | v2+ | Organization-scale access control. |
| Break-glass user and recovery access validation | R/W | later | Ensure emergency access remains available and audited. |

## 19. Smart home, IoT and edge devices

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Home Assistant health, integration availability and automation errors | R | later |
| MQTT broker health, topic rates and client status | R | later | Topic payloads may contain personal data. |
| Zigbee/Z-Wave coordinator and device availability | R | later |
| Camera/NVR health, recording gaps and storage status | R | later | Strong privacy boundaries; avoid viewing or retaining streams by default. |
| NAS/IoT/smart plug/AP/UPS firmware and uptime inventory | R | later |
| Toggle a selected smart plug or restart a selected IoT device | W | later | Can cut power to networking or critical equipment; require dependency model. |
| Environmental sensor alerting (temperature, humidity, leak, smoke) | R | later | Sensor quality and alarm severity must be represented. |
| Automate energy-saving schedules or load shedding | W | later | Owner-defined safety and comfort constraints. |
| Smart lock, alarm and access-control operations | S/W | later/strictly constrained | High consequence; require explicit security and identity model. |

## 20. Messaging, observability and incident ecosystem

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Prometheus metrics queries and time-window comparison | R | v1.x |
| Grafana dashboard/panel links and annotations | R/W | later |
| Loki, Elasticsearch/OpenSearch and journald log search | R | later | Query caps, tenant scope and secret redaction. |
| OpenTelemetry traces and service dependency evidence | R | later |
| Alert deduplication, enrichment, routing and silence status | R/W | v1.x |
| PagerDuty/Opsgenie incident status and on-call schedule | R/W | later |
| Status page updates and post-incident report drafts | W | later | Human approval for public communications. |
| Metrics/alert rule suggestions from repeated incidents | R/W | v1.x | Suggest only; user applies. |
| SLO/error-budget tracking and service-level health | R | later | Needs service ownership and SLO definitions. |
| Scheduled operational reports and weekly environment health summaries | R | v1.x |
| Multi-channel notifications and escalation policy | W | v1.x | Avoid duplicate/noisy notification loops. |

## 21. Data services, messaging and application queues

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Queue depth, consumer lag and dead-letter inventory (RabbitMQ, Redis, Kafka, NATS) | R | later |
| Retry or requeue a selected failed message | W | later | Message replay may duplicate side effects. |
| Pause/resume a consumer or worker pool | W | later | Respect backpressure and maintenance state. |
| Object/file processing queues and stuck-job diagnosis | R | v1.x |
| Search index health, lag and disk usage | R | later |
| Reindex or rebuild a selected index | W | later | Capacity and downtime preview required. |
| Data pipeline freshness, schema drift and failed ETL jobs | R | later |
| Trigger a bounded pipeline rerun | W | later | Require idempotency and source watermark control. |

## 22. User-facing operational assistance

- Explain a host or service in plain language using verified knowledge, not guesses.
- Answer “what changed?”, “what depends on this?”, “what is exposed?”, “what is currently unsafe?” and “what happens if I restart it?” from current evidence.
- Show planned action impact before authorization: affected entities, dependencies, expected downtime, rollback and confidence gaps.
- Compare alternative remediation plans and explain why one is safer.
- Let the owner set maintenance intent conversationally, then present a structured confirmation before storing it.
- Turn a resolved incident into a human-editable runbook proposal.
- Produce a post-incident summary with timeline, hypotheses, evidence, actions, policy decisions, verification and follow-up items.
- Provide a “why did you not act?” explanation showing unavailable evidence, missing permissions, stale policy or safety gates.
- Support user correction of entity identity, dependency, criticality and fact provenance.
- Provide a clear emergency stop / pause-all-mutations control, separate from read-only monitoring.
- Provide a per-host capability test/self-check and a policy simulation (“what would Domovoi do if this happened now?”).

## 23. Capability quality checklist

Before adding a capability, specify:

1. Stable ID, version, provider and supported host/platform.
2. Plain-language purpose and when it is appropriate.
3. Typed inputs and outputs; strict target scoping.
4. Read/write/security/destructive effect and likely blast radius.
5. Required credentials, permissions, prerequisites and availability detection.
6. Time, output, concurrency and rate limits.
7. Idempotency and behavior when the result is unknown.
8. Precondition checks and postcondition verification.
9. Rollback, compensation or expiry strategy; explicitly say when none exists.
10. Policy dimensions and the default authorization level.
11. Audit fields, secret redaction and user-visible explanation.
12. Unit, integration, failure, restart, offline and negative-target tests.

A capability is not ready for autonomous use until this checklist is complete and its relevant incident drills pass.
