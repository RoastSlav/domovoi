# 41 · Feature: change the password and sign out other sessions from the web UI

## Background (verified on `main`)

- The password can only be changed with the CLI: `domovoi hub set-password`
  (`crates/domovoi/src/cmd/hub.rs:25,61`). Read how it hashes (argon2id,
  `crates/domovoi-hub/src/auth/password.rs`) and stores
  (`UserRepo::set_password_hash`); reuse those, do not hash differently.
- `SessionRepo::delete_for_user(user)` exists
  (`crates/domovoi-core/src/ports/repos/accounts.rs`) and **nothing calls it**: changing
  the password with the CLI leaves every existing session signed in.
- Sessions: hashed ids, 12 h idle, 7 days absolute (`crates/domovoi-hub/src/auth/session.rs`,
  `auth/mod.rs`). Logout deletes one session (`api/auth.rs:73`).
- Re-prompting for the password on sensitive saves already exists
  (`crates/domovoi-hub/src/settings/service.rs:232-243`) and shares the login rate
  limiter (`auth/limit.rs`). Use the same path for the current-password check.

## Steps

1. **API** in `crates/domovoi-hub/src/api/auth.rs` (or a new `api/account.rs` if
   `auth.rs` would pass 300 lines):
   - `POST /api/v1/account/password` `{ current, new }`: check `current` through the
     limiter-backed verify; refuse `new` shorter than the setup minimum (find it in
     `auth/mod.rs` setup; reuse the constant); store the new hash; delete **all** of the
     user's sessions, then create a fresh one for this browser and set its cookie, so the
     caller stays signed in and everyone else is out. Audit `account.password_changed`.
   - `POST /api/v1/account/sessions/revoke-others` `{ current }`: delete every session
     of the user except the caller's. Needs `SessionRepo::delete_for_user_except(user, keep: &[u8;32])`
     (port, store, testkit). Audit `account.sessions_revoked`.
   - Regenerate OpenAPI; add both to the `writes` list in
     `crates/domovoi-hub/tests/gates.rs`.
2. **CLI.** `domovoi hub set-password` also calls `delete_for_user` and prints
   "Signed out N web sessions."
3. **Web** (the Settings page and the user menu already have slots for Account):
   Settings → an "Account" section, also linked from the user menu:
   - Change password: `Field`s for current, new and repeat with a visibility toggle,
     the password rule as a live checklist (reuse the one from account setup),
     mismatch shown inline, `bad_credentials` and rate limits (countdown) through
     `describeError`, submit with `loading`. On success a toast "Password changed.
     Other sessions were signed out."
   - "Sign out other sessions": `ConfirmDialog` explaining the consequence, then
     `PasswordPrompt`; toast with the result.
4. **Book.** `docs/book/src/guide/hardening.md` and `reference/cli.md`: both ways to
   change the password and that they end other sessions.

## Tests

- Hub: a wrong current password is refused and counted by the limiter; after a change
  the old session cookie of another client gets 401 and the caller's new cookie works;
  revoke-others keeps the caller.
- CLI test in `crates/domovoi/tests/cli.rs`: set-password removes sessions.
- Web: the form validates and shows the result.

## Done when

- Changing the password anywhere ends every other session.
