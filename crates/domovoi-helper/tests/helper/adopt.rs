use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;

use domovoi_core::identity::RefusalToken;
use domovoi_helper::adopt::{AdoptError, adopt_connection, check_listen_env};
use nix::fcntl::{FcntlArg, FdFlag, fcntl};

// covers: adopt_unexpected_listen_fds
#[test]
fn refuses_listen_fds_greater_than_zero() {
    for value in ["1", "2", "3"] {
        let err = check_listen_env(Some(value)).expect_err("a passed socket is refused");
        assert!(matches!(err, AdoptError::UnexpectedListenFds(_)));
        assert_eq!(err.token(), Some(RefusalToken::AdoptUnexpectedListenFds));
    }
}

#[test]
fn accepts_listen_fds_unset_or_zero() {
    assert!(check_listen_env(None).is_ok());
    assert!(check_listen_env(Some("0")).is_ok());
}

#[test]
fn refuses_malformed_listen_fds() {
    for value in ["", "abc", "-1", "00", " 0", "0 "] {
        assert!(
            check_listen_env(Some(value)).is_err(),
            "`{value}` must be refused"
        );
    }
}

#[test]
fn adopts_a_connected_unix_socket_as_stdin() {
    let (ours, mut theirs) = UnixStream::pair().expect("pair");
    let mut adopted = adopt_connection(ours.as_fd()).expect("adopts a socket");
    adopted.write_all(b"hi").expect("write through the copy");
    let mut got = [0_u8; 2];
    theirs.read_exact(&mut got).expect("read from the peer");
    assert_eq!(&got, b"hi");
}

// covers: adopt_not_a_socket
#[test]
fn refuses_a_non_socket_descriptor() {
    let file = tempfile::tempfile().expect("tempfile");
    let err = adopt_connection(file.as_fd()).expect_err("a file is not a socket");
    assert!(matches!(err, AdoptError::NotASocket(_)));
    assert_eq!(err.token(), Some(RefusalToken::AdoptNotASocket));
}

#[test]
fn adopted_stream_is_close_on_exec() {
    let (ours, _theirs) = UnixStream::pair().expect("pair");
    let adopted = adopt_connection(ours.as_fd()).expect("adopts a socket");
    let flags = fcntl(adopted.as_fd(), FcntlArg::F_GETFD).expect("F_GETFD");
    assert!(FdFlag::from_bits_truncate(flags).contains(FdFlag::FD_CLOEXEC));
}
