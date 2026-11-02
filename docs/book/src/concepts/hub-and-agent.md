# Hub and agents

The target product is one binary with two roles. The **hub** holds state, the web UI, chat,
triage and approvals. An **agent** runs on each guarded host, runs checks and skills, and
connects out to the hub over mutually authenticated TLS. Run both on one machine to start,
or put the hub on another machine so it notices when a host dies.

The role runtimes and their protocol are implemented as libraries, but the current binary
does not expose supported `hub` or `agent` commands. See [Quick start](../quick-start.md) for
the available development preview.
