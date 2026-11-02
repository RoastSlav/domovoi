# Hardening

There is no supported production deployment to harden yet. Do not expose the development
preview to a network or grant it access to real Docker sockets, credentials or hosts.

The first release guide will cover reverse proxies, Docker socket restrictions, agent risk
policy and secret handling. The [security model](../security.md) describes the intended
runtime invariants, not a deployment checklist for the preview.
