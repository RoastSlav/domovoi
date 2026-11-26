//! The UDP sender of the leak self-test, run inside the sender namespace by `ip netns exec`.
//!
//! It is the same binary re-invoked as `domovoi helper nft-selftest-sender`. It binds one source
//! address, sends small datagrams as fast as the machine allows for a fixed time and prints
//! `sent=<n>`. It uses ordinary UDP sockets: no raw socket, no `unsafe`, no privilege.

use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

/// What one sender run was asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SenderArgs {
    /// The source address; the element under test.
    pub src: Ipv4Addr,
    /// The receiver's address.
    pub dst: Ipv4Addr,
    /// The receiver's UDP port.
    pub port: u16,
    /// How long to send, in seconds.
    pub seconds: u32,
}

/// Sends until the time is up and returns how many datagrams the kernel accepted.
pub fn run(args: SenderArgs) -> std::io::Result<u64> {
    let socket = UdpSocket::bind(SocketAddrV4::new(args.src, 0))?;
    socket.set_nonblocking(true)?;
    let target = SocketAddrV4::new(args.dst, args.port);
    let end = Instant::now() + Duration::from_secs(u64::from(args.seconds));
    let payload = [0u8; 16];
    let mut sent = 0u64;
    while Instant::now() < end {
        // Check the clock once per batch so the loop is mostly sending.
        for _ in 0..256 {
            if socket.send_to(&payload, target).is_ok() {
                sent += 1;
            }
        }
    }
    Ok(sent)
}

/// Parses the text of `--src`, `--dst`, `--port` and `--seconds` arguments.
pub fn parse(args: &[String]) -> Option<SenderArgs> {
    let mut src = None;
    let mut dst = None;
    let mut port = None;
    let mut seconds = None;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let value = it.next()?;
        match flag.as_str() {
            "--src" => src = value.parse().ok(),
            "--dst" => dst = value.parse().ok(),
            "--port" => port = value.parse().ok(),
            "--seconds" => seconds = value.parse().ok(),
            _ => return None,
        }
    }
    Some(SenderArgs {
        src: src?,
        dst: dst?,
        port: port?,
        seconds: seconds?,
    })
}
