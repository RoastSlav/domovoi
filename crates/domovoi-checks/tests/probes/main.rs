//! TCP, HTTP and gateway probes. These open real loopback sockets; no test waits on time.

#[cfg(test)]
mod gateway;
#[cfg(test)]
mod harness;
#[cfg(test)]
mod http;
#[cfg(test)]
mod tcp;
