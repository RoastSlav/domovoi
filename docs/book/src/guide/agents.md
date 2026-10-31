# Agents and enrollment

An agent runs on each host Domovoi watches. It connects out to the hub, so the host needs no open ports. The connection is TLS, and both sides prove who they are: the agent pins the hub's certificate, and the hub checks a signature made with the agent's own key.

## Enrolling a host

1. On the hub, create a token for the host. If the host is new, this registers it.
2. The hub prints one command, valid for 15 minutes:

   ```text
   domovoi agent enroll --hub wss://hub.lan:7421/ --token <token> --fingerprint 3A:9F:…:C2
   ```

3. Run it on the host. The agent makes a new key pair, connects, checks that the hub's certificate matches the fingerprint, and sends its public key together with the token.

The token works once. The hub keeps only its SHA-256, so a copy of the database cannot enroll anything. A wrong, used or expired token is refused after a short delay.

The address in the command comes from `[agent_server] public_url`. Without it, the hub uses `wss://<hub name>:<port>`.

## The certificate

On first start the hub creates a self-signed certificate under `<data_dir>/tls/`, valid for 10 years. Its key is readable by the hub user only. Agents trust that certificate and no other: names and certificate authorities play no part.

Keep `<data_dir>/tls/` with your backups. If the certificate is lost, the hub makes a new one, and every agent must enroll again.

## Connecting

Each time it connects, the agent says hello. The hub answers with a random challenge, which the agent signs with its key. The signature also covers the TLS session itself, so it cannot be replayed on another connection. Only then does the session start and the hub send work to the agent.

The hub refuses an agent that it does not know, one that has been revoked, or one whose protocol version is too far from its own. A new connection from the same agent replaces the old one.

## Revoking and re-enrolling

Revoking a host's agent closes its connection at once, and the agent can never connect again with that key.

To reinstall a host, create a new token for it and enroll again. The new agent replaces the old one, and the old key is revoked.

## Host down, agent down, or the hub's own network

Agents send a heartbeat every `[agent_server] heartbeat` (10 seconds by default). When an agent has been silent for three heartbeats, the hub looks into it once:

1. **The hub doubts itself first.** It checks that its own gateway answers and that it can reach `[agent_server] self_check` (`1.1.1.1:53` by default). If not, it reports `hub.network_degraded` about itself and blames no host.
2. **Then it probes the host.** It uses the host's `[[probes]]` TCP targets if there are any, otherwise the address the agent last connected from, on ports 22 and 443. A refused connection still means the host is up.
   - The host answers: `agent.down`, a warning. The machine is up, but its agent is not.
   - Nothing answers: `host.down`, critical, sent at once.

These become incidents like any other. When the agent is heard from again, they resolve.

```toml
[agent_server]
bind = "0.0.0.0:7421"
public_url = "wss://hub.lan:7421"
self_check = "1.1.1.1:53"
heartbeat = "10s"

[[probes]]
host = "nas"
kind = "tcp"
target = "nas.lan:445"
```
