//! The report types, their rendering and the errors of the strict parser.

use core::net::IpAddr;
use std::collections::BTreeSet;

use jiff::Timestamp;

use super::boot::{BootId, Nonce32};
use super::config::ConfigLines;
use super::field::EncodeError;
use super::hash::{Hash32, ZERO_HASH, domain_hash};
use super::lines::LinkPeer;
use super::reason::IncompleteReason;
use super::scalar::{Cidr, parse_ip};
use super::{REPORT_FORMAT, ROUTES_PREFIX};

/// Why the strict parser, or the hash recomputation, refused a report.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReportError {
    /// The bytes are not valid UTF-8. Checked as its own step, before any grammar.
    #[error("not valid UTF-8: {0}")]
    NotUtf8(String),
    /// The report starts with a byte order mark.
    #[error("byte order mark")]
    Bom,
    /// The report contains a carriage return.
    #[error("carriage return")]
    Cr,
    /// The last byte is not LF.
    #[error("missing final LF")]
    MissingFinalLf,
    /// A line is empty.
    #[error("blank line {0}")]
    BlankLine(usize),
    /// A line ends in a space or a tab.
    #[error("trailing whitespace on line {0}")]
    TrailingWhitespace(usize),
    /// A line has no `=`.
    #[error("line {0} has no '='")]
    NoEquals(usize),
    /// A key outside the fixed set.
    #[error("unknown key {0:?}")]
    UnknownKey(String),
    /// A single-valued key appears twice.
    #[error("duplicate key {0}")]
    DuplicateKey(String),
    /// A key appears after a key that must follow it.
    #[error("key {0} out of order")]
    OutOfOrder(String),
    /// A required key is absent.
    #[error("missing key {0}")]
    MissingKey(&'static str),
    /// Two identical repeated lines.
    #[error("duplicate line {0:?}")]
    DuplicateLine(String),
    /// Repeated lines that are not in bytewise ascending order.
    #[error("unsorted line {0:?}")]
    UnsortedLine(String),
    /// `format` is not `1`.
    #[error("unsupported format {0:?}")]
    BadFormat(String),
    /// An empty value where a value is required.
    #[error("empty value for {0}")]
    EmptyValue(&'static str),
    /// `complete` and `incomplete_reason` disagree.
    #[error("incomplete_reason inconsistent with complete: {0}")]
    ReasonInconsistent(&'static str),
    /// A value broke the field codec or a typed grammar.
    #[error("{key}: {err}")]
    Value {
        /// The key.
        key: &'static str,
        /// The violation.
        err: EncodeError,
    },
    /// A header value is outside its grammar.
    #[error("{key}: bad value {value:?}")]
    BadValue {
        /// The key.
        key: &'static str,
        /// The offending text.
        value: String,
    },
    /// A complete report whose generation is zero.
    #[error("a complete report has generation 0")]
    GenerationZero,
    /// A complete report whose named hash is the zero hash.
    #[error("{0} is the zero hash in a complete report")]
    ZeroHash(&'static str),
    /// An incomplete report that is not the one minimal shape.
    #[error("incomplete report is not the minimal shape: {0}")]
    IncompleteShape(&'static str),
    /// The report is longer than the size cap.
    #[error("report is {0} bytes, over the cap")]
    TooLarge(usize),
    /// A hash in the report differs from the hash of the report's own lines.
    #[error("{0} does not match the report's own lines")]
    HashMismatch(&'static str),
}

/// The route half of a report: encoded values, sorted bytewise and de-duplicated.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RouteLines {
    /// `hub_addr` values.
    pub hub_addr: Vec<String>,
    /// `link_peer` values.
    pub link_peer: Vec<String>,
    /// `local_addr` values.
    pub local_addr: Vec<String>,
    /// `route` values.
    pub route: Vec<String>,
    /// `rule` values.
    pub rule: Vec<String>,
    /// `endpoint_route` values.
    pub endpoint_route: Vec<String>,
}

impl RouteLines {
    /// The `key=value` lines in report order.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for (key, values) in [
            ("hub_addr", &self.hub_addr),
            ("link_peer", &self.link_peer),
            ("local_addr", &self.local_addr),
            ("route", &self.route),
            ("rule", &self.rule),
            ("endpoint_route", &self.endpoint_route),
        ] {
            lines.extend(values.iter().map(|value| format!("{key}={value}")));
        }
        lines
    }

    /// The `route_state_hash`: SHA-256 of the routes prefix and the lines.
    #[must_use]
    pub fn hash(&self) -> Hash32 {
        domain_hash(ROUTES_PREFIX, &self.lines())
    }

    /// Sorts every group bytewise and drops exact duplicates.
    pub fn normalise(&mut self) {
        for group in [
            &mut self.hub_addr,
            &mut self.link_peer,
            &mut self.local_addr,
            &mut self.route,
            &mut self.rule,
            &mut self.endpoint_route,
        ] {
            let sorted: BTreeSet<String> = group.drain(..).collect();
            group.extend(sorted);
        }
    }
}

/// A report that carries the full effective safety set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteReport {
    /// A label for the key, 16 lowercase hex characters. Never used to verify or decide.
    pub helper_key_id: String,
    /// The nonce the hub issued.
    pub nonce: Nonce32,
    /// The boot id the helper read.
    pub boot_id: BootId,
    /// The generation, at least 1.
    pub generation: u64,
    /// The hash of the config lines.
    pub config_hash: Hash32,
    /// The hash of the route lines.
    pub route_state_hash: Hash32,
    /// When the helper computed it. For display only; the hub never trusts it.
    pub computed_at: Timestamp,
    /// The config lines.
    pub config: ConfigLines,
    /// The route lines.
    pub routes: RouteLines,
}

/// The one minimal shape of `complete=false`: a signed statement of why no set is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncompleteReport {
    /// A label for the key, 16 lowercase hex characters.
    pub helper_key_id: String,
    /// The nonce the hub issued.
    pub nonce: Nonce32,
    /// The boot id the helper read.
    pub boot_id: BootId,
    /// Why the set could not be computed.
    pub reason: IncompleteReason,
    /// When the helper computed it. For display only.
    pub computed_at: Timestamp,
}

/// A parsed report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    /// `complete=true`. Boxed: it is several times the size of the incomplete shape.
    Complete(Box<CompleteReport>),
    /// `complete=false`.
    Incomplete(IncompleteReport),
}

fn timestamp_text(at: Timestamp) -> String {
    at.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn header(
    key_id: &str,
    nonce: &Nonce32,
    boot_id: &BootId,
    generation: u64,
    hashes: (&Hash32, &Hash32),
    status: (bool, &str),
    computed_at: Timestamp,
) -> String {
    let mut out = String::new();
    let mut push = |key: &str, value: &str| {
        out.push_str(key);
        out.push('=');
        out.push_str(value);
        out.push('\n');
    };
    push("format", &REPORT_FORMAT.to_string());
    push("helper_key_id", key_id);
    push("nonce", &nonce.to_string());
    push("boot_id", boot_id.as_str());
    push("generation", &generation.to_string());
    push("config_hash", &hashes.0.to_string());
    push("route_state_hash", &hashes.1.to_string());
    push("complete", if status.0 { "true" } else { "false" });
    push("incomplete_reason", status.1);
    push("computed_at", &timestamp_text(computed_at));
    out
}

/// Renders a complete report: the exact bytes that are signed.
#[must_use]
pub fn render_complete(report: &CompleteReport) -> Vec<u8> {
    let mut out = header(
        &report.helper_key_id,
        &report.nonce,
        &report.boot_id,
        report.generation,
        (&report.config_hash, &report.route_state_hash),
        (true, ""),
        report.computed_at,
    );
    for line in report
        .config
        .lines()
        .into_iter()
        .chain(report.routes.lines())
    {
        out.push_str(&line);
        out.push('\n');
    }
    out.into_bytes()
}

/// Renders the minimal incomplete report: generation 0, both hashes zero, `max_ttl_s=0`,
/// `max_active=0` and no repeated line.
#[must_use]
pub fn render_incomplete(report: &IncompleteReport) -> Vec<u8> {
    let mut out = header(
        &report.helper_key_id,
        &report.nonce,
        &report.boot_id,
        0,
        (&ZERO_HASH, &ZERO_HASH),
        (false, report.reason.as_str()),
        report.computed_at,
    );
    out.push_str("max_ttl_s=0\nmax_active=0\n");
    out.into_bytes()
}

impl CompleteReport {
    /// Recomputes both hashes from the report's own lines and compares them with the carried
    /// ones. The hub never normalises what it received.
    ///
    /// # Errors
    ///
    /// [`ReportError::HashMismatch`] naming the first hash that differs.
    pub fn check_hashes(&self) -> Result<(), ReportError> {
        if self.config_hash != self.config.hash() {
            return Err(ReportError::HashMismatch("config_hash"));
        }
        if self.route_state_hash != self.routes.hash() {
            return Err(ReportError::HashMismatch("route_state_hash"));
        }
        Ok(())
    }

    /// The configured management networks, parsed from the report's `cidr` lines.
    #[must_use]
    pub fn cidrs(&self) -> Vec<Cidr> {
        self.config
            .cidr
            .iter()
            .filter_map(|text| Cidr::parse_report(text).ok())
            .collect()
    }

    /// The hub addresses, parsed from the report's `hub_addr` lines.
    #[must_use]
    pub fn hub_addrs(&self) -> Vec<IpAddr> {
        self.routes
            .hub_addr
            .iter()
            .filter_map(|text| parse_ip(text).ok())
            .collect()
    }

    /// The link peers, parsed from the report's `link_peer` lines.
    #[must_use]
    pub fn link_peers(&self) -> Vec<(IpAddr, u16)> {
        self.routes
            .link_peer
            .iter()
            .filter_map(|text| LinkPeer::parse(text).ok())
            .map(|peer| (peer.ip, peer.port))
            .collect()
    }
}
