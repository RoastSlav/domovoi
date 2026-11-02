# Writing skills

The skill manifest loader and runner are implemented in the runtime libraries, but there is
no supported agent command, skill installer or starter-skill collection yet. The current
`domovoi` binary also does not provide `domovoi check skills`.

The first release will document `skill.toml`, parameter validation, risk levels,
verification and local testing. For now, treat the manifest format as an implementation
detail rather than installing skills on a production host.
