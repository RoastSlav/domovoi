# Examples

- `hub.toml`: a short, commented example. The file is optional: a new hub is set up in the web UI, and anything set here is locked there.
- `hub.full.toml`: every `hub.toml` option, for running the hub as config as code.
- `agent.toml`: a fully commented agent configuration. `domovoi agent enroll` writes one when none exists.
- `skills/`: starter skills. Validate a skills folder with `domovoi check skills <dir>`.

Release archives ship this folder next to the binary.
