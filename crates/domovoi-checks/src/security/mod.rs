//! Security checks: SSH logins, sudo failures, fail2ban bans and new listening ports.
//!
//! The log-based checks read the journal since a cursor kept in their state, so every
//! line is looked at once. Targets are users, jails or `addr:port/proto`; counts, addresses
//! and times are evidence.

pub mod fail2ban;
pub mod ip;
pub mod ports;
pub mod ssh;
pub mod sudo;

pub use fail2ban::{Fail2banBans, JailStatus, parse_jail_status};
pub use ip::{Network, classify_ip};
pub use ports::{Listener, NewPorts, Proto, parse_port_range, parse_proc_net};
pub use ssh::{SshLogin, SshLogins, parse_accepted};
pub use sudo::{SudoFailures, parse_sudo_failure};

use crate::commands::{JournalCursor, JournalScope};
use crate::framework::{CheckContext, CheckError, run_command};
use crate::system::kernel::{CURSOR_STATE, parse_cursor_line};

/// Journal messages in `scope` since this check's cursor, without the cursor line. Moves
/// the cursor forward; output cut by the cap starts over from a recent window next time.
pub(crate) async fn journal_since_cursor(
    ctx: &mut CheckContext<'_>,
    scope: JournalScope<'_>,
) -> Result<Vec<String>, CheckError> {
    let previous = ctx
        .state
        .get(CURSOR_STATE)
        .and_then(|c| JournalCursor::parse(c).ok());
    let spec = ctx
        .commands
        .journal_since_cursor(previous.as_ref(), scope)?;
    let output = run_command(ctx, spec).await?;
    let mut cursor = None;
    let mut lines = Vec::new();
    for line in output.stdout.lines() {
        match parse_cursor_line(line) {
            Some(found) => cursor = Some(found),
            None => lines.push(line.to_owned()),
        }
    }
    match cursor {
        Some(cursor) => ctx.state.set(CURSOR_STATE, cursor.as_str()),
        None if output.truncated => ctx.state.remove(CURSOR_STATE),
        None => {}
    }
    Ok(lines)
}
