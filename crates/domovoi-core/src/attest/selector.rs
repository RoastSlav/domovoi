//! The selector of a policy rule, with interface names escaped inside their own item.

use super::field::{EncodeError, decode_field, encode_field, not_canonical};
use super::scalar::{Cidr, check_ifname, parse_uint};

/// The selector of a policy rule: `all`, or the items that are present, in a fixed order.
///
/// Interface names are bytes. [`Selector::render`] strict-encodes each name before the items
/// are joined with `;`, so a name holding `;`, `=` or `|` cannot be read as another item; the
/// rule line then encodes the whole selector once more as one field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selector {
    /// `from=<cidr>`
    pub from: Option<Cidr>,
    /// `to=<cidr>`
    pub to: Option<Cidr>,
    /// `iif=<encoded name>`
    pub iif: Option<Vec<u8>>,
    /// `oif=<encoded name>`
    pub oif: Option<Vec<u8>>,
    /// `fwmark=0x<value>/0x<mask>`
    pub fwmark: Option<(u32, u32)>,
    /// `ipproto=<0..=255>`
    pub ipproto: Option<u8>,
    /// `sport=<start>-<end>`
    pub sport: Option<(u16, u16)>,
    /// `dport=<start>-<end>`
    pub dport: Option<(u16, u16)>,
    /// `uidrange=<start>-<end>`
    pub uidrange: Option<(u32, u32)>,
    /// `not`
    pub not: bool,
}

impl Selector {
    /// The selector text: `all`, or the present items joined by `;` with names strict-encoded
    /// inside their item.
    #[must_use]
    pub fn render(&self) -> String {
        let mut items: Vec<String> = Vec::new();
        if let Some(cidr) = &self.from {
            items.push(format!("from={}", cidr.render()));
        }
        if let Some(cidr) = &self.to {
            items.push(format!("to={}", cidr.render()));
        }
        if let Some(name) = &self.iif {
            items.push(format!("iif={}", encode_field(name)));
        }
        if let Some(name) = &self.oif {
            items.push(format!("oif={}", encode_field(name)));
        }
        if let Some((value, mask)) = self.fwmark {
            items.push(format!("fwmark=0x{value:x}/0x{mask:x}"));
        }
        if let Some(proto) = self.ipproto {
            items.push(format!("ipproto={proto}"));
        }
        if let Some((a, b)) = self.sport {
            items.push(format!("sport={a}-{b}"));
        }
        if let Some((a, b)) = self.dport {
            items.push(format!("dport={a}-{b}"));
        }
        if let Some((a, b)) = self.uidrange {
            items.push(format!("uidrange={a}-{b}"));
        }
        if self.not {
            items.push("not".to_owned());
        }
        if items.is_empty() {
            "all".to_owned()
        } else {
            items.join(";")
        }
    }

    /// Parses decoded selector text strictly: accepted only when it renders back identically.
    ///
    /// # Errors
    ///
    /// [`EncodeError`] for a bad item, a bad name, an out-of-range number, a repeated or
    /// misordered item, or any other deviation from the canonical text.
    pub fn parse(text: &str) -> Result<Self, EncodeError> {
        let bad = || not_canonical("selector", text);
        if text == "all" {
            return Ok(Self::default());
        }
        let mut selector = Self::default();
        for item in text.split(';') {
            let (key, value) = item
                .split_once('=')
                .map_or((item, None), |(key, value)| (key, Some(value)));
            match (key, value) {
                ("from", Some(v)) => selector.from = Some(Cidr::parse_report(v)?),
                ("to", Some(v)) => selector.to = Some(Cidr::parse_report(v)?),
                ("iif", Some(v)) => selector.iif = Some(name(v)?),
                ("oif", Some(v)) => selector.oif = Some(name(v)?),
                ("fwmark", Some(v)) => selector.fwmark = Some(fwmark(v).ok_or_else(bad)?),
                ("ipproto", Some(v)) => {
                    selector.ipproto = Some(
                        parse_uint(v)
                            .ok()
                            .and_then(|n| u8::try_from(n).ok())
                            .ok_or_else(bad)?,
                    );
                }
                ("sport", Some(v)) => selector.sport = Some(range16(v).ok_or_else(bad)?),
                ("dport", Some(v)) => selector.dport = Some(range16(v).ok_or_else(bad)?),
                ("uidrange", Some(v)) => selector.uidrange = Some(range32(v).ok_or_else(bad)?),
                ("not", None) => selector.not = true,
                _ => return Err(bad()),
            }
        }
        if selector.render() == text {
            Ok(selector)
        } else {
            Err(bad())
        }
    }
}

fn name(text: &str) -> Result<Vec<u8>, EncodeError> {
    let bytes = decode_field(text)?;
    check_ifname(&bytes)?;
    Ok(bytes)
}

fn range(text: &str) -> Option<(u64, u64)> {
    let (start, end) = text.split_once('-')?;
    let (start, end) = (parse_uint(start).ok()?, parse_uint(end).ok()?);
    (start <= end).then_some((start, end))
}

fn range16(text: &str) -> Option<(u16, u16)> {
    let (start, end) = range(text)?;
    Some((u16::try_from(start).ok()?, u16::try_from(end).ok()?))
}

fn range32(text: &str) -> Option<(u32, u32)> {
    let (start, end) = range(text)?;
    Some((u32::try_from(start).ok()?, u32::try_from(end).ok()?))
}

/// Lowercase hex without leading zeros, at most eight digits.
fn hexn(text: &str) -> Option<u32> {
    let digits = text.strip_prefix("0x")?;
    let canonical = !digits.is_empty()
        && digits.len() <= 8
        && digits
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && (digits == "0" || !digits.starts_with('0'));
    if canonical {
        u32::from_str_radix(digits, 16).ok()
    } else {
        None
    }
}

fn fwmark(text: &str) -> Option<(u32, u32)> {
    let (value, mask) = text.split_once('/')?;
    Some((hexn(value)?, hexn(mask)?))
}
