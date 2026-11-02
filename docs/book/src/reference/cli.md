# Command line

The product CLI is not wired to the hub and agent runtimes yet. The current `domovoi`
binary has only these commands:

| Command | Purpose |
| --- | --- |
| `domovoi` or `domovoi version` | Print the binary version. |
| `domovoi openapi` | Hidden developer command used to regenerate the OpenAPI document. |

The OpenAPI command is hidden from normal help output. In a source checkout, use
`cargo xtask openapi` to write the generated API file, or `cargo xtask openapi --check`
to check it.

To preview the web UI with seeded development data, follow [Quick start](../quick-start.md).
Commands such as `domovoi hub run`, `domovoi agent enroll`, `domovoi check config` and
`domovoi check skills` are planned, but are not available in this version.
