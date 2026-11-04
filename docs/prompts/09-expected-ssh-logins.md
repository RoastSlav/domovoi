# 09 · Feature: tell Domovoi which SSH logins are expected

Needs prompt 07 (ack rules). Prompt 08 is optional (used in step 5).

## Goal

Per host, the user can say which SSH logins are expected (who, from where, for how
long), and say "I am about to log in" for a short window. Expected logins are recorded
but never alert and are not triaged; every other login alerts as today.

## What is already known (verified)

- The agent check is `security.ssh_login`
  (`crates/domovoi-checks/src/security/ssh.rs`): one immediate signal per user per run,
  **target = the user name**, evidence `ip`, `method`, `port`, `network` (`lan` or `wan`,
  from `classify_ip` in `security/ip.rs`) and `count`. WAN logins are `warning`, LAN
  logins `info`.
- Ack rules (prompt 07) already match on host, check, target pattern and evidence
  predicates (`EvidencePredicate::{Equals, Contains, LessOrEqual, GreaterOrEqual}` in
  `crates/domovoi-core/src/ack.rs`). So "user alice from the LAN" is
  `target_pattern = "alice"`, `Equals { key: "network", value: "lan" }`, and
  "anyone for the next hour" is `target_pattern = None`, no predicates, `expires_at` in
  one hour. What is missing is matching an address range, and a friendly UI.

## Read these files

- `crates/domovoi-core/src/ack.rs` (and its tests in the same file or
  `crates/domovoi-core/tests/`)
- `crates/domovoi-hub/src/api/rules.rs` (from prompt 07)
- `web/src/features/rules/RulesPage.tsx`, `web/src/api/hooks/rules.ts`
- `web/src/features/hosts/HostDetailPage.tsx`

## Steps

1. **Core: match an address range.** Add
   ```rust
   /// Text value is an IP address inside `cidr`, such as `192.168.1.0/24` or `fd00::/8`.
   InNetwork { key: String, cidr: String },
   ```
   to `EvidencePredicate` (serde name `in_network`). Implement `holds` with a private
   pure helper `fn in_cidr(ip: &str, cidr: &str) -> bool`: parse `ip` as
   `std::net::IpAddr`; split `cidr` at `/`, parse the address and the prefix
   (0–32 for IPv4, 0–128 for IPv6); compare the first `prefix` bits using `u32` / `u128`
   from `octets()`. An IPv4 address never matches an IPv6 range. Bad input → `false`.
   Unit tests: inside, outside, `/0`, `/32`, mixed families, garbage.
2. **API.** In `rules.rs`, accept `op: "in_network"` in `PredicateDto`. Validate the CIDR
   when creating the rule (400 "not an address range" when `in_cidr("0.0.0.0", cidr)`
   style parsing fails; expose a `pub fn valid_cidr(cidr: &str) -> bool` from core for
   this). Regenerate OpenAPI.
3. **Web: an "SSH logins" tab on the host page.** In `HostDetailPage.tsx` add a tab
   rendered by new `web/src/features/hosts/SshLoginsTab.tsx`:
   - Text: "Every successful SSH login raises an incident. Logins you mark as expected
     are still recorded but never alert you."
   - **Expected logins**: ack rules from `useAckRules()` where `host === name` and
     `check === "security.ssh_login"`, shown as sentences ("alice from the local network,
     always", "anyone, until 14:30"), each with Remove.
   - **Add expected login** form: user (text, empty = anyone → no target pattern),
     from (select: Anywhere → no predicate; Local network → `network equals lan`; This
     address or range → `in_network` on key `ip` with a text input), for (1 hour, 1 day,
     1 week, Always), note (prefilled "Expected SSH login"). Submit creates the rule.
   - **I'm logging in now** button: creates `host`, `security.ssh_login`, no target, no
     predicates, 60 minutes, note "Login window opened from the web UI".
4. **Docs.** In `docs/book/src/reference/checks.md`, under `security.ssh_login`, add how
   to mark logins as expected.
5. **Optional, with prompt 08.** On an incident whose check is `security.ssh_login`, the
   Dismiss dialog shows a checkbox "Expect this user from this address again", which
   creates a rule with `target_pattern = <user>` and `Equals { key: "ip", value: <ip> }`
   (the IP comes from the incident's latest signal evidence; add `ip` to what the dialog
   receives, or let the API build the rule when `expect_again: true` is sent).

## Tests

- Core: the `in_cidr` / `InNetwork` tests from step 1, plus an `AckRule::matches` test
  for a `security.ssh_login` signal with `ip = 192.168.1.20` against `192.168.1.0/24`.
- Hub: creating a rule with a bad CIDR is a 400.
- Web: `SshLoginsTab` lists only SSH rules for the host, and the "I'm logging in now"
  button posts the expected body.

## Done when

- Expected logins can be added per host by user, by LAN, by address range, or as a
  one-hour window, and matching logins no longer alert.
- All tests pass; `ack.rs` stays under 500 lines.
