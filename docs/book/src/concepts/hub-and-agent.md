# Hub and agents

Domovoi is one binary with two roles. The **hub** holds state, the web UI, chat, triage and approvals. An **agent** runs on each guarded host, runs checks and skills, and connects out to the hub over mutually authenticated TLS. Run both on one machine to start, or put the hub on another machine so it notices when a host dies.

More in v0.1 (build-plan prompt P25).
