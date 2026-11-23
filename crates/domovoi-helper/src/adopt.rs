//! Adopting the connection systemd hands the helper on descriptor 0.
//!
//! The socket unit uses `Accept=yes`, so each helper process gets exactly one connected socket as
//! standard input. Adoption uses std only: it duplicates the descriptor with close-on-exec set and
//! asks the kernel whether the duplicate is a Unix socket. `LISTEN_*` is checked but never used to
//! find a descriptor and never changed; a non-zero `LISTEN_FDS` means a unit with `Accept=no`,
//! which this helper does not support.

use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::UnixStream;

use domovoi_core::identity::RefusalToken;

/// Why the connection could not be adopted.
#[derive(Debug, thiserror::Error)]
pub enum AdoptError {
    /// `LISTEN_FDS` is set and is not `0`, so the unit is not the supported `Accept=yes` shape.
    #[error(
        "LISTEN_FDS is `{0}`; the helper supports only Accept=yes with the connection on descriptor 0"
    )]
    UnexpectedListenFds(String),
    /// Descriptor 0 is not a Unix socket (a terminal, a file or a pipe).
    #[error("descriptor 0 is not a Unix socket: {0}")]
    NotASocket(std::io::Error),
    /// The descriptor could not be duplicated or switched to non-blocking mode.
    #[error("cannot adopt the connection: {0}")]
    Io(std::io::Error),
}

impl AdoptError {
    /// The refusal token the failure is reported under, when it has one.
    #[must_use]
    pub const fn token(&self) -> Option<RefusalToken> {
        match self {
            Self::UnexpectedListenFds(_) => Some(RefusalToken::AdoptUnexpectedListenFds),
            Self::NotASocket(_) => Some(RefusalToken::AdoptNotASocket),
            Self::Io(_) => None,
        }
    }
}

/// Refuses an environment that describes passed listening sockets.
///
/// `None` and `"0"` pass; any other value, including text that is not a number, fails.
pub fn check_listen_env(listen_fds: Option<&str>) -> Result<(), AdoptError> {
    match listen_fds {
        None | Some("0") => Ok(()),
        Some(other) => Err(AdoptError::UnexpectedListenFds(other.to_owned())),
    }
}

/// Duplicates `fd` (close-on-exec) and checks that the duplicate is a Unix socket.
pub fn adopt_connection(fd: BorrowedFd<'_>) -> Result<UnixStream, AdoptError> {
    let owned = fd.try_clone_to_owned().map_err(AdoptError::Io)?;
    let stream = UnixStream::from(owned);
    stream.local_addr().map_err(AdoptError::NotASocket)?;
    Ok(stream)
}

/// Adopts the connection on descriptor 0 as a tokio stream.
///
/// Reads `LISTEN_FDS`, borrows standard input without reading from it, switches the duplicate to
/// non-blocking mode and registers it with the running tokio runtime.
pub fn adopt_stdin_connection() -> Result<tokio::net::UnixStream, AdoptError> {
    let listen = std::env::var_os("LISTEN_FDS");
    let text = listen.as_ref().map(|value| value.to_string_lossy());
    check_listen_env(text.as_deref())?;
    let stdin = std::io::stdin();
    let stream = adopt_connection(stdin.as_fd())?;
    stream.set_nonblocking(true).map_err(AdoptError::Io)?;
    tokio::net::UnixStream::from_std(stream).map_err(AdoptError::Io)
}
