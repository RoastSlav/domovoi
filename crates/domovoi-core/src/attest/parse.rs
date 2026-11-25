//! The strict report parser.
//!
//! It works on the received bytes and never normalises: a value is accepted only when it is
//! already canonical, so rendering a parsed report gives back the bytes that were parsed.

use jiff::Timestamp;

use super::MAX_REPORT_BYTES;
use super::boot::{BootId, Nonce32};
use super::config::{ConfigLines, canonical_unit, check_container};
use super::field::{EncodeError, decode_field};
use super::hash::Hash32;
use super::lines::{EndpointRoute, LinkPeer, LocalAddr, Route, Rule};
use super::reason::IncompleteReason;
use super::report::{CompleteReport, IncompleteReport, Report, ReportError, RouteLines};
use super::scalar::{Cidr, parse_ip, parse_uint};

/// Keys in report order, with whether they repeat.
const KEY_ORDER: [(&str, bool); 21] = [
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

fn bad(key: &'static str, value: &str) -> ReportError {
    ReportError::BadValue {
        key,
        value: value.to_owned(),
    }
}

fn typed<T>(key: &'static str, result: Result<T, EncodeError>) -> Result<T, ReportError> {
    result.map_err(|err| ReportError::Value { key, err })
}

fn lower_hex(value: &str, digits: usize) -> bool {
    value.len() == digits
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn name(key: &'static str, value: &str) -> Result<String, ReportError> {
    String::from_utf8(typed(key, decode_field(value))?).map_err(|_| bad(key, value))
}

/// Validates one repeated-key value and returns its canonical text, which must equal `value`.
fn canonical_repeated(key: &'static str, value: &str) -> Result<String, ReportError> {
    Ok(match key {
        "cidr" => typed(key, Cidr::parse_report(value))?.render(),
        "protected_unit" => {
            let unit = name(key, value)?;
            let canonical = canonical_unit(&unit).map_err(|_| bad(key, value))?;
            if canonical != unit {
                return Err(bad(key, value));
            }
            canonical
        }
        "protected_container" => {
            let container = name(key, value)?;
            check_container(&container).map_err(|_| bad(key, value))?;
            container
        }
        "hub_addr" => typed(key, parse_ip(value))?.to_string(),
        "link_peer" => typed(key, LinkPeer::parse(value))?.render(),
        "local_addr" => typed(key, LocalAddr::parse(value))?.render(),
        "route" => typed(key, Route::parse(value))?.render(),
        "rule" => typed(key, Rule::parse(value))?.render(),
        "endpoint_route" => typed(key, EndpointRoute::parse(value))?.render(),
        _ => return Err(bad(key, value)),
    })
}

fn parse_timestamp(value: &str) -> Result<Timestamp, ReportError> {
    let shape = value.len() == 20
        && value.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 => byte == b'-',
            10 => byte == b'T',
            13 | 16 => byte == b':',
            19 => byte == b'Z',
            _ => byte.is_ascii_digit(),
        });
    if !shape {
        return Err(bad("computed_at", value));
    }
    let at: Timestamp = value.parse().map_err(|_| bad("computed_at", value))?;
    if at.strftime("%Y-%m-%dT%H:%M:%SZ").to_string() == value {
        Ok(at)
    } else {
        Err(bad("computed_at", value))
    }
}

#[derive(Default)]
struct Parsed {
    helper_key_id: String,
    nonce: Option<Nonce32>,
    boot_id: Option<BootId>,
    generation: u64,
    config_hash: Option<Hash32>,
    route_state_hash: Option<Hash32>,
    complete: bool,
    reason: String,
    computed_at: Option<Timestamp>,
    config: ConfigLines,
    routes: RouteLines,
    repeated: usize,
}

impl Parsed {
    fn set(&mut self, key: &'static str, value: &str) -> Result<(), ReportError> {
        match key {
            "format" => {
                if value != "1" {
                    return Err(ReportError::BadFormat(value.to_owned()));
                }
            }
            "helper_key_id" => {
                if !lower_hex(value, 16) {
                    return Err(bad(key, value));
                }
                value.clone_into(&mut self.helper_key_id);
            }
            "nonce" => self.nonce = Some(Nonce32::parse(value).ok_or_else(|| bad(key, value))?),
            "boot_id" => self.boot_id = Some(BootId::parse(value).map_err(|_| bad(key, value))?),
            "generation" => self.generation = typed(key, parse_uint(value))?,
            "config_hash" => {
                self.config_hash = Some(value.parse().map_err(|_| bad(key, value))?);
            }
            "route_state_hash" => {
                self.route_state_hash = Some(value.parse().map_err(|_| bad(key, value))?);
            }
            "complete" => {
                self.complete = match value {
                    "true" => true,
                    "false" => false,
                    _ => return Err(bad(key, value)),
                };
            }
            "incomplete_reason" => value.clone_into(&mut self.reason),
            "computed_at" => self.computed_at = Some(parse_timestamp(value)?),
            "max_ttl_s" => self.config.max_ttl_s = typed(key, parse_uint(value))?,
            "max_active" => self.config.max_active = typed(key, parse_uint(value))?,
            _ => {
                if canonical_repeated(key, value)? != value {
                    return Err(bad(key, value));
                }
                self.repeated += 1;
                let text = value.to_owned();
                match key {
                    "cidr" => self.config.cidr.push(text),
                    "protected_unit" => self.config.protected_unit.push(text),
                    "protected_container" => self.config.protected_container.push(text),
                    "hub_addr" => self.routes.hub_addr.push(text),
                    "link_peer" => self.routes.link_peer.push(text),
                    "local_addr" => self.routes.local_addr.push(text),
                    "route" => self.routes.route.push(text),
                    "rule" => self.routes.rule.push(text),
                    _ => self.routes.endpoint_route.push(text),
                }
            }
        }
        Ok(())
    }
}

/// Parses received report bytes strictly.
///
/// The checks run in this order: size, UTF-8, byte order mark, carriage return, final LF, then
/// line by line, then the shape of the whole report. A parsed report renders back to exactly
/// the bytes given. Hashes are not recomputed here; see [`CompleteReport::check_hashes`].
///
/// # Errors
///
/// The first [`ReportError`] found.
pub fn parse_report(bytes: &[u8]) -> Result<Report, ReportError> {
    if bytes.len() > MAX_REPORT_BYTES {
        return Err(ReportError::TooLarge(bytes.len()));
    }
    let text = core::str::from_utf8(bytes).map_err(|e| ReportError::NotUtf8(e.to_string()))?;
    if text.starts_with('\u{FEFF}') {
        return Err(ReportError::Bom);
    }
    if text.contains('\r') {
        return Err(ReportError::Cr);
    }
    let body = text.strip_suffix('\n').ok_or(ReportError::MissingFinalLf)?;
    let mut parsed = Parsed::default();
    let mut seen = [false; KEY_ORDER.len()];
    let mut last_key = 0_usize;
    let mut previous: Option<(usize, &str)> = None;
    for (number, line) in body.split('\n').enumerate() {
        let number = number + 1;
        if line.is_empty() {
            return Err(ReportError::BlankLine(number));
        }
        if line.ends_with(' ') || line.ends_with('\t') {
            return Err(ReportError::TrailingWhitespace(number));
        }
        let (key_text, value) = line.split_once('=').ok_or(ReportError::NoEquals(number))?;
        let index = KEY_ORDER
            .iter()
            .position(|(name, _)| *name == key_text)
            .ok_or_else(|| ReportError::UnknownKey(key_text.to_owned()))?;
        let (key, repeats) = KEY_ORDER[index];
        if index < last_key {
            return Err(ReportError::OutOfOrder(key_text.to_owned()));
        }
        if index == last_key && seen[index] && !repeats {
            return Err(ReportError::DuplicateKey(key_text.to_owned()));
        }
        if let Some((previous_key, previous_line)) = previous
            && repeats
            && previous_key == index
        {
            if line == previous_line {
                return Err(ReportError::DuplicateLine(line.to_owned()));
            }
            if line.as_bytes() < previous_line.as_bytes() {
                return Err(ReportError::UnsortedLine(line.to_owned()));
            }
        }
        last_key = index;
        seen[index] = true;
        previous = Some((index, line));
        if value.is_empty() && key != "incomplete_reason" {
            return Err(ReportError::EmptyValue(key));
        }
        parsed.set(key, value)?;
    }
    for (index, (name, repeats)) in KEY_ORDER.iter().copied().enumerate() {
        if !repeats && !seen[index] {
            return Err(ReportError::MissingKey(name));
        }
    }
    finish(parsed)
}

fn finish(parsed: Parsed) -> Result<Report, ReportError> {
    let (Some(nonce), Some(boot_id), Some(config_hash), Some(route_state_hash), Some(at)) = (
        parsed.nonce,
        parsed.boot_id,
        parsed.config_hash,
        parsed.route_state_hash,
        parsed.computed_at,
    ) else {
        return Err(ReportError::MissingKey("header"));
    };
    if parsed.complete {
        if !parsed.reason.is_empty() {
            return Err(ReportError::ReasonInconsistent(
                "complete=true must have an empty reason",
            ));
        }
        if parsed.generation == 0 {
            return Err(ReportError::GenerationZero);
        }
        if config_hash.is_zero() {
            return Err(ReportError::ZeroHash("config_hash"));
        }
        if route_state_hash.is_zero() {
            return Err(ReportError::ZeroHash("route_state_hash"));
        }
        return Ok(Report::Complete(Box::new(CompleteReport {
            helper_key_id: parsed.helper_key_id,
            nonce,
            boot_id,
            generation: parsed.generation,
            config_hash,
            route_state_hash,
            computed_at: at,
            config: parsed.config,
            routes: parsed.routes,
        })));
    }
    if parsed.reason.is_empty() {
        return Err(ReportError::ReasonInconsistent(
            "complete=false needs a reason",
        ));
    }
    let reason = IncompleteReason::new(&parsed.reason)
        .ok_or_else(|| bad("incomplete_reason", &parsed.reason))?;
    let shape = if parsed.generation != 0 {
        Some("generation is not 0")
    } else if !config_hash.is_zero() || !route_state_hash.is_zero() {
        Some("a hash is not the zero hash")
    } else if parsed.config.max_ttl_s != 0 || parsed.config.max_active != 0 {
        Some("max_ttl_s and max_active must be 0")
    } else if parsed.repeated != 0 {
        Some("repeated lines are not allowed")
    } else {
        None
    };
    if let Some(why) = shape {
        return Err(ReportError::IncompleteShape(why));
    }
    Ok(Report::Incomplete(IncompleteReport {
        helper_key_id: parsed.helper_key_id,
        nonce,
        boot_id,
        reason,
        computed_at: at,
    }))
}

/// Validates one repeated-key line (`key=value`) in isolation, with the grammar the full parser
/// applies to that line.
///
/// # Errors
///
/// The [`ReportError`] the full parser would give for the line.
pub fn check_line(line: &str) -> Result<(), ReportError> {
    let (key_text, value) = line.split_once('=').ok_or(ReportError::NoEquals(1))?;
    let (key, repeats) = KEY_ORDER
        .iter()
        .copied()
        .find(|(name, _)| *name == key_text)
        .ok_or_else(|| ReportError::UnknownKey(key_text.to_owned()))?;
    if !repeats {
        return Err(ReportError::UnknownKey(format!(
            "{key_text} (not a repeated key)"
        )));
    }
    if canonical_repeated(key, value)? == value {
        Ok(())
    } else {
        Err(bad(key, value))
    }
}
