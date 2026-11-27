//! Running a check's journal read, and telling an unreadable journal from a quiet one.

use domovoi_core::ports::{ProcessOutput, ProcessSpec};

use crate::commands::journal_access_refused;
use crate::framework::{CheckContext, CheckError, run_command};
use crate::system::kernel::CURSOR_STATE;

/// What to tell the owner when journalctl refused the account.
const REFUSED: &str = "this agent account may not read the system journal (add it to the \
                       systemd-journal group and restart the agent)";

/// Why `output` is not a read of the journal, or `None` when it is one. Output cut by the
/// cap is a read, whatever the exit status.
fn unread_reason(output: &ProcessOutput) -> Option<String> {
    let said = output.stderr.trim();
    if journal_access_refused(said) {
        return Some(format!("{REFUSED}. journalctl said: {said}"));
    }
    if output.timed_out {
        return Some("journalctl did not finish in time".to_owned());
    }
    if said.contains("No journal files were") {
        return Some(format!("journalctl found no journal to read: {said}"));
    }
    if output.exit_code != Some(0) && !output.truncated {
        let status = output
            .exit_code
            .map_or_else(|| "a signal".to_owned(), |code| format!("status {code}"));
        let said = if said.is_empty() { "nothing" } else { said };
        return Some(format!("journalctl ended with {status} and said: {said}"));
    }
    None
}

/// Runs a journal read for a check. An unreadable journal is an error that says why, and
/// leaves the cursor where it was, so the lines are read once access is back. A cursor
/// journalctl cannot seek to is dropped, so the next run starts from a recent window.
pub(crate) async fn read(
    ctx: &mut CheckContext<'_>,
    spec: ProcessSpec,
) -> Result<ProcessOutput, CheckError> {
    let output = run_command(ctx, spec).await?;
    let Some(why) = unread_reason(&output) else {
        return Ok(output);
    };
    if output.stderr.contains("seek to cursor") {
        ctx.state.remove(CURSOR_STATE);
    }
    Err(CheckError::JournalUnreadable(why))
}
