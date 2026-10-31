# Security policy

Domovoi runs on servers with broad access: the agent can read logs and run approved scripts as root. We take security reports seriously.

## Reporting a vulnerability

**Do not open a public issue.** Use GitHub's private vulnerability reporting: go to the repository's **Security** tab and choose **Report a vulnerability**. Include what you found, how to reproduce it, the impact you expect, and the version or commit.

You will get an acknowledgement within 72 hours and a first assessment within 7 days. We will keep you informed, agree on a disclosure date with you, and credit you in the advisory unless you prefer otherwise.

## Supported versions

Before 1.0, only the latest release receives security fixes.

## Scope

Especially interesting:

- Any path where model output, chat text, webhook payloads or log content can reach a process, an approval or a configuration change.
- Approving actions without an authenticated human, or approving destructive actions from chat.
- Bypassing the agent's local re-validation of skills (manifest, risk policy, plan hash).
- Agent link authentication, enrollment tokens, certificate pinning.
- Session, CSRF or authentication flaws in the web UI and API.
- Secrets leaking into logs, prompts, chat messages or API responses.

Out of scope: issues requiring an already compromised hub host or root on the guarded host, and findings in third-party services (LLM providers, Discord).

The security invariants and how they are enforced are listed in [ARCHITECTURE.md §15](ARCHITECTURE.md#15-security-invariants).
