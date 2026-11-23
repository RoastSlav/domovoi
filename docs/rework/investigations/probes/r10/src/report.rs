//! The signed report: construction and the strict parser (RW-37, 04b).
//!
//! The parser works on the original bytes and never normalises: a value is accepted only if it
//! is already canonical.

use crate::config::ConfigLines;
use crate::encoding::{EncodingError, decode_field, parse_uint};
use crate::hash::HASH_SENTINEL;
use crate::lines::{
    EndpointRoute, LinkPeer, LocalAddr, Route, Rule, canonical_unit, check_container,
};
use crate::routes::RouteLines;
use crate::time::parse_unix;

/// Keys in report order, with whether they repeat.
pub const KEY_ORDER: [(&str, bool); 21] = [
    ("format", false),
    ("helper_key_id", false),
    ("nonce", false),
    ("boot_id", false),
    ("generation", false),
    ("config_hash", false),
    ("route_state_hash", false),
    ("complete", false),
    ("incomplete_reason", false),
    ("computed_at", false),
    ("cidr", true),
    ("protected_unit", true),
    ("protected_container", true),
    ("max_ttl_s", false),
    ("max_active", false),
    ("hub_addr", true),
    ("link_peer", true),
    ("local_addr", true),
    ("route", true),
    ("rule", true),
    ("endpoint_route", true),
];

/// How the parser treats an empty `incomplete_reason=` (the 04b section 1 versus RW-37 conflict).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyReasonPolicy {
    /// Harness default: empty exactly when `complete=true`, a token exactly when `complete=false`.
    EmptyIffComplete,
    /// 04b section 1 read literally ("one or more fields"): no value may be empty.
    RejectAllEmpty,
    /// Accept an empty value whatever `complete` says (no consistency check).
    AcceptAnywhere,
}

/// Why the strict parser (or the hash recomputation) refused a report.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// Bytes are not UTF-8 (checked as its own step, before any grammar).
    #[error("not valid UTF-8: {0}")]
    NotUtf8(String),
    /// Byte order mark at the start.
    #[error("byte order mark")]
    Bom,
    /// A CR byte.
    #[error("carriage return")]
    Cr,
    /// The last byte is not LF.
    #[error("missing final LF")]
    MissingFinalLf,
    /// An empty line.
    #[error("blank line {0}")]
    BlankLine(usize),
    /// A line ending in space or tab.
    #[error("trailing whitespace on line {0}")]
    TrailingWhitespace(usize),
    /// A line without `=`.
    #[error("line {0} has no '='")]
    NoEquals(usize),
    /// A key outside the fixed set.
    #[error("unknown key {0:?}")]
    UnknownKey(String),
    /// A single-valued key repeated.
    #[error("duplicate key {0}")]
    DuplicateKey(String),
    /// A key that appears after a later key.
    #[error("key {0} out of order")]
    OutOfOrder(String),
    /// A required key is absent.
    #[error("missing key {0}")]
    MissingKey(&'static str),
    /// Two identical repeated lines.
    #[error("duplicate line {0:?}")]
    DuplicateLine(String),
    /// Repeated lines not in bytewise ascending order.
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
    /// A value broke the strict encoding or a typed grammar.
    #[error("{key}: {err}")]
    Value {
        /// The key.
        key: &'static str,
        /// The violation.
        err: EncodingError,
    },
    /// A header value is not in its grammar.
    #[error("{key}: bad value {value:?}")]
    BadValue {
        /// The key.
        key: &'static str,
        /// The text.
        value: String,
    },
    /// A hash in the report differs from the hash recomputed from the report's own lines.
    #[error("{0} does not match the report's own lines")]
    HashMismatch(&'static str),
}

/// A parsed or constructed report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// 16 hex label.
    pub helper_key_id: String,
    /// The nonce the hub issued.
    pub nonce: [u8; 32],
    /// Lowercase UUID.
    pub boot_id: String,
    /// Generation (0 only in an incomplete report).
    pub generation: u64,
    /// `config_hash` as carried.
    pub config_hash: [u8; 32],
    /// `route_state_hash` as carried.
    pub route_state_hash: [u8; 32],
    /// `complete`.
    pub complete: bool,
    /// `incomplete_reason`; `None` is the empty value.
    pub incomplete_reason: Option<String>,
    /// `computed_at`, canonical RFC 3339.
    pub computed_at: String,
    /// Config block.
    pub config: ConfigLines,
    /// Route block.
    pub routes: RouteLines,
}

impl Report {
    /// The exact signed byte string.
    #[must_use]
    pub fn render(&self) -> Vec<u8> {
        let mut s = String::new();
        let mut push = |k: &str, v: &str| {
            s.push_str(k);
            s.push('=');
            s.push_str(v);
            s.push('\n');
        };
        push("format", "1");
        push("helper_key_id", &self.helper_key_id);
        push("nonce", &hex::encode(self.nonce));
        push("boot_id", &self.boot_id);
        push("generation", &self.generation.to_string());
        push("config_hash", &hex::encode(self.config_hash));
        push("route_state_hash", &hex::encode(self.route_state_hash));
        push("complete", if self.complete { "true" } else { "false" });
        push("incomplete_reason", self.incomplete_reason.as_deref().unwrap_or(""));
        push("computed_at", &self.computed_at);
        for l in self.config.lines().into_iter().chain(self.routes.lines()) {
            s.push_str(&l);
            s.push('\n');
        }
        s.into_bytes()
    }

    /// The hub's recomputation: each carried hash must equal the hash of the report's own lines,
    /// or be the sentinel in an incomplete report. The hub never normalises.
    ///
    /// # Errors
    /// [`ParseError::HashMismatch`].
    pub fn check_hashes(&self) -> Result<(), ParseError> {
        let ok = |carried: [u8; 32], real: [u8; 32]| {
            carried == real || (!self.complete && carried == HASH_SENTINEL)
        };
        if !ok(self.config_hash, self.config.hash()) {
            return Err(ParseError::HashMismatch("config_hash"));
        }
        if !ok(self.route_state_hash, self.routes.hash()) {
            return Err(ParseError::HashMismatch("route_state_hash"));
        }
        Ok(())
    }
}

fn bad(key: &'static str, value: &str) -> ParseError {
    ParseError::BadValue { key, value: value.to_owned() }
}

fn val<T>(key: &'static str, r: Result<T, EncodingError>) -> Result<T, ParseError> {
    r.map_err(|err| ParseError::Value { key, err })
}

fn hex_n<const N: usize>(key: &'static str, v: &str) -> Result<[u8; N], ParseError> {
    if v.len() != N * 2 || !v.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(bad(key, v));
    }
    hex::decode(v).ok().and_then(|b| b.try_into().ok()).ok_or_else(|| bad(key, v))
}

fn uuid(key: &'static str, v: &str) -> Result<(), ParseError> {
    let ok = v.len() == 36
        && v.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        });
    if ok { Ok(()) } else { Err(bad(key, v)) }
}

fn reason_token(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 64
        && v.as_bytes()[0].is_ascii_lowercase()
        && v.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn check_repeated(key: &'static str, v: &str) -> Result<(), ParseError> {
    // Parse strictly, then require the canonical re-rendering to equal the input.
    let canon = match key {
        "cidr" => val(key, crate::encoding::parse_cidr(v)).map(|c| c.render())?,
        "protected_unit" => {
            let name = String::from_utf8(val(key, decode_field(v))?).map_err(|_| bad(key, v))?;
            let c = val(key, canonical_unit(&name))?;
            if c != name {
                return Err(bad(key, v));
            }
            c
        }
        "protected_container" => {
            let name = String::from_utf8(val(key, decode_field(v))?).map_err(|_| bad(key, v))?;
            val(key, check_container(&name))?;
            name
        }
        "hub_addr" => val(key, crate::encoding::parse_ip(v))?.to_string(),
        "link_peer" => val(key, LinkPeer::parse(v))?.render(),
        "local_addr" => val(key, LocalAddr::parse(v))?.render(),
        "route" => val(key, Route::parse(v))?.render(),
        "rule" => val(key, Rule::parse(v))?.render(),
        "endpoint_route" => val(key, EndpointRoute::parse(v))?.render(),
        _ => return Err(bad(key, v)),
    };
    if canon == v { Ok(()) } else { Err(bad(key, v)) }
}

/// Strict parse of the received report bytes.
///
/// # Errors
/// The first violation, checked in this order: UTF-8, BOM, CR, final LF, then line by line.
pub fn parse_report(bytes: &[u8], policy: EmptyReasonPolicy) -> Result<Report, ParseError> {
    let text = std::str::from_utf8(bytes).map_err(|e| ParseError::NotUtf8(e.to_string()))?;
    if text.starts_with('\u{FEFF}') {
        return Err(ParseError::Bom);
    }
    if text.contains('\r') {
        return Err(ParseError::Cr);
    }
    let body = text.strip_suffix('\n').ok_or(ParseError::MissingFinalLf)?;
    let mut idx = 0usize; // index into KEY_ORDER of the last accepted key
    let mut seen = [false; KEY_ORDER.len()];
    let mut prev_line: Option<&str> = None;
    let mut prev_key: Option<usize> = None;
    let mut r = Report {
        helper_key_id: String::new(),
        nonce: [0; 32],
        boot_id: String::new(),
        generation: 0,
        config_hash: [0; 32],
        route_state_hash: [0; 32],
        complete: false,
        incomplete_reason: None,
        computed_at: String::new(),
        config: ConfigLines::default(),
        routes: RouteLines::default(),
    };
    let mut reason_raw: Option<&str> = None;
    for (n, line) in body.split('\n').enumerate() {
        let n = n + 1;
        if line.is_empty() {
            return Err(ParseError::BlankLine(n));
        }
        if line.ends_with(' ') || line.ends_with('\t') {
            return Err(ParseError::TrailingWhitespace(n));
        }
        let (k, v) = line.split_once('=').ok_or(ParseError::NoEquals(n))?;
        let ki = KEY_ORDER
            .iter()
            .position(|(name, _)| *name == k)
            .ok_or_else(|| ParseError::UnknownKey(k.to_owned()))?;
        let (key, repeats) = KEY_ORDER[ki];
        if ki < idx {
            return Err(ParseError::OutOfOrder(k.to_owned()));
        }
        if ki == idx && seen[ki] && !repeats {
            return Err(ParseError::DuplicateKey(k.to_owned()));
        }
        if repeats && prev_key == Some(ki) {
            let p = prev_line.unwrap_or("");
            if line == p {
                return Err(ParseError::DuplicateLine(line.to_owned()));
            }
            if line.as_bytes() < p.as_bytes() {
                return Err(ParseError::UnsortedLine(line.to_owned()));
            }
        }
        idx = ki;
        seen[ki] = true;
        prev_key = Some(ki);
        prev_line = Some(line);
        if v.is_empty() && key != "incomplete_reason" {
            return Err(ParseError::EmptyValue(key));
        }
        match key {
            "format" => {
                if v != "1" {
                    return Err(ParseError::BadFormat(v.to_owned()));
                }
            }
            "helper_key_id" => r.helper_key_id = hex::encode(hex_n::<8>(key, v)?),
            "nonce" => r.nonce = hex_n::<32>(key, v)?,
            "boot_id" => {
                uuid(key, v)?;
                v.clone_into(&mut r.boot_id);
            }
            "generation" => r.generation = val(key, parse_uint(v))?,
            "config_hash" => r.config_hash = hex_n::<32>(key, v)?,
            "route_state_hash" => r.route_state_hash = hex_n::<32>(key, v)?,
            "complete" => {
                r.complete = match v {
                    "true" => true,
                    "false" => false,
                    _ => return Err(bad(key, v)),
                }
            }
            "incomplete_reason" => reason_raw = Some(v),
            "computed_at" => {
                if parse_unix(v).is_none() {
                    return Err(bad(key, v));
                }
                v.clone_into(&mut r.computed_at);
            }
            "max_ttl_s" => r.config.max_ttl_s = val(key, parse_uint(v))?,
            "max_active" => r.config.max_active = val(key, parse_uint(v))?,
            _ => {
                check_repeated(key, v)?;
                let t = v.to_owned();
                match key {
                    "cidr" => r.config.cidr.push(t),
                    "protected_unit" => r.config.protected_unit.push(t),
                    "protected_container" => r.config.protected_container.push(t),
                    "hub_addr" => r.routes.hub_addr.push(t),
                    "link_peer" => r.routes.link_peer.push(t),
                    "local_addr" => r.routes.local_addr.push(t),
                    "route" => r.routes.route.push(t),
                    "rule" => r.routes.rule.push(t),
                    _ => r.routes.endpoint_route.push(t),
                }
            }
        }
    }
    for (i, (name, repeats)) in KEY_ORDER.iter().enumerate() {
        if !repeats && !seen[i] {
            return Err(ParseError::MissingKey(name));
        }
    }
    let rv = reason_raw.unwrap_or("");
    if rv.is_empty() {
        match policy {
            EmptyReasonPolicy::RejectAllEmpty => return Err(ParseError::EmptyValue("incomplete_reason")),
            EmptyReasonPolicy::EmptyIffComplete if !r.complete => {
                return Err(ParseError::ReasonInconsistent("complete=false needs a reason"));
            }
            _ => {}
        }
    } else {
        if !reason_token(rv) {
            return Err(bad("incomplete_reason", rv));
        }
        if policy == EmptyReasonPolicy::EmptyIffComplete && r.complete {
            return Err(ParseError::ReasonInconsistent("complete=true must have an empty reason"));
        }
        r.incomplete_reason = Some(rv.to_owned());
    }
    Ok(r)
}

/// Validate one repeated-key line (`key=value`) in isolation, with the same strict grammar the
/// full parser applies.
///
/// # Errors
/// The violation found.
pub fn check_line(line: &str) -> Result<(), ParseError> {
    let (k, v) = line.split_once('=').ok_or(ParseError::NoEquals(1))?;
    let (key, repeats) = KEY_ORDER
        .iter()
        .find(|(n, _)| *n == k)
        .copied()
        .ok_or_else(|| ParseError::UnknownKey(k.to_owned()))?;
    if !repeats {
        return Err(ParseError::UnknownKey(format!("{k} (not a repeated key)")));
    }
    check_repeated(key, v)
}
