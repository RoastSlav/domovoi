//! `net.read`: listening sockets with the process that owns each.

use domovoi_core::helper::read::{ListenerWire, MAX_LISTENERS, ReadFailure, ReadToken, WireProto};

use super::{Tool, ToolEnv, command_failed, net_args, run};

/// The longest process name reported.
const MAX_PROCESS_NAME: usize = 64;

/// The listeners `ss` reports, at most [`MAX_LISTENERS`], and whether the list was cut. Lines
/// that do not parse are skipped; output with lines but none that parse is malformed.
pub async fn listeners(env: ToolEnv<'_>) -> Result<(Vec<ListenerWire>, bool), ReadFailure> {
    let output = run(env, Tool::Ss, net_args()).await?;
    if output.exit_code != Some(0) && output.stdout.trim().is_empty() {
        return Err(command_failed(&output));
    }
    let (mut found, seen_lines) = parse(&output.stdout);
    if found.is_empty() && seen_lines > 0 {
        return Err(ReadFailure::bare(ReadToken::OutputMalformed));
    }
    let truncated = found.len() > MAX_LISTENERS || output.truncated;
    found.truncate(MAX_LISTENERS);
    Ok((found, truncated))
}

/// Parses `ss` output; returns the listeners and how many non-empty lines were seen.
#[must_use]
pub fn parse(text: &str) -> (Vec<ListenerWire>, usize) {
    let mut seen = 0;
    let found = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .inspect(|_| seen += 1)
        .filter_map(parse_line)
        .collect();
    (found, seen)
}

/// `tcp LISTEN 0 4096 0.0.0.0:22 0.0.0.0:* users:(("sshd",pid=1,fd=3))`.
fn parse_line(line: &str) -> Option<ListenerWire> {
    let mut columns = line.split_whitespace();
    let proto = match columns.next()? {
        "tcp" => WireProto::Tcp,
        "udp" => WireProto::Udp,
        _ => return None,
    };
    let local = columns.nth(3)?;
    let (addr, port) = split_local(local)?;
    let (process, pid) = parse_users(line);
    Some(ListenerWire {
        proto,
        addr,
        port,
        process,
        pid,
    })
}

/// Splits `addr:port` on the last `:`; an IPv6 address is in brackets and an interface zone
/// (`%lo`) is dropped.
fn split_local(local: &str) -> Option<(String, u16)> {
    let (addr, port) = local.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    let addr = addr.trim_start_matches('[').trim_end_matches(']');
    let addr = addr.split('%').next().unwrap_or(addr);
    (!addr.is_empty()).then(|| (addr.to_owned(), port))
}

/// The first `users:(("name",pid=N,fd=M),...)` entry.
fn parse_users(line: &str) -> (Option<String>, Option<u32>) {
    let Some((_, users)) = line.split_once("users:((\"") else {
        return (None, None);
    };
    let Some((name, rest)) = users.split_once("\",pid=") else {
        return (None, None);
    };
    let pid = rest
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|digits| digits.parse().ok());
    (Some(sanitise(name)), pid)
}

/// At most 64 bytes, every byte outside printable ASCII replaced by `?`.
fn sanitise(name: &str) -> String {
    name.bytes()
        .take(MAX_PROCESS_NAME)
        .map(|b| {
            if b.is_ascii_graphic() || b == b' ' {
                char::from(b)
            } else {
                '?'
            }
        })
        .collect()
}
