# Security model

Domovoi is designed so that a confused or manipulated model cannot cause harm on its own:

1. The model only gets read-only tools, and everything it reads from logs and alerts is treated as untrusted data.
2. Only an authenticated human can approve an action, from the web UI or from an allowlisted chat account.
3. Actions are skills you install on the host: argv-only execution, validated parameters, never a shell, and re-validated by the agent right before running.
4. Destructive skills need a typed confirmation in the web UI and can never be approved from chat.
5. Every proposal, approval and execution is written to an append-only audit log.

The full list of invariants and how each is enforced is in [ARCHITECTURE.md §15](https://github.com/RoastSlav/domovoi/blob/main/ARCHITECTURE.md#15-security-invariants). To report a vulnerability, see [SECURITY.md](https://github.com/RoastSlav/domovoi/blob/main/SECURITY.md).
