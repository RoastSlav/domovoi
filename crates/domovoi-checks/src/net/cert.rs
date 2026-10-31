//! Reads a certificate's expiry from its DER encoding.
//!
//! Only the path `Certificate → tbsCertificate → validity → notAfter` is walked; nothing
//! else is interpreted. The certificate has already been verified by rustls when the probe
//! reads it, so this needs no trust decisions, but it still treats its input as hostile:
//! every length is bounds-checked and any surprise yields `None`.

use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::TimeZone;

const SEQUENCE: u8 = 0x30;
const INTEGER: u8 = 0x02;
const EXPLICIT_VERSION: u8 = 0xa0;
const UTC_TIME: u8 = 0x17;
const GENERALIZED_TIME: u8 = 0x18;

/// Splits one DER element off `input`: its tag, its contents and what follows it.
fn element(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    if tag & 0x1f == 0x1f {
        // Multi-byte tags never occur on this path.
        return None;
    }
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let (bytes, rest) = rest.split_at(count);
        let len = bytes
            .iter()
            .fold(0_usize, |acc, b| (acc << 8) | usize::from(*b));
        (len, rest)
    };
    if rest.len() < len {
        return None;
    }
    let (contents, after) = rest.split_at(len);
    Some((tag, contents, after))
}

fn expect(input: &[u8], tag: u8) -> Option<(&[u8], &[u8])> {
    let (found, contents, rest) = element(input)?;
    (found == tag).then_some((contents, rest))
}

fn digits(text: &[u8]) -> Option<i64> {
    if text.is_empty() || !text.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(text).ok()?.parse().ok()
}

fn parse_time(tag: u8, text: &[u8]) -> Option<Timestamp> {
    let (year, rest) = match tag {
        UTC_TIME => {
            let yy = digits(text.get(..2)?)?;
            (if yy >= 50 { 1900 + yy } else { 2000 + yy }, text.get(2..)?)
        }
        GENERALIZED_TIME => (digits(text.get(..4)?)?, text.get(4..)?),
        _ => return None,
    };
    if rest.len() != 11 || rest.last() != Some(&b'Z') {
        return None;
    }
    let field = |i: usize| -> Option<i8> { i8::try_from(digits(rest.get(i..i + 2)?)?).ok() };
    let datetime = DateTime::new(
        i16::try_from(year).ok()?,
        field(0)?,
        field(2)?,
        field(4)?,
        field(6)?,
        field(8)?,
        0,
    )
    .ok()?;
    Some(datetime.to_zoned(TimeZone::UTC).ok()?.timestamp())
}

/// The `notAfter` time of a DER-encoded X.509 certificate.
#[must_use]
pub fn not_after(der: &[u8]) -> Option<Timestamp> {
    let (certificate, _) = expect(der, SEQUENCE)?;
    let (tbs, _) = expect(certificate, SEQUENCE)?;
    let mut rest = tbs;
    if let Some((EXPLICIT_VERSION, _, after)) = element(rest) {
        rest = after;
    }
    let (_, rest) = expect(rest, INTEGER)?;
    let (_, rest) = expect(rest, SEQUENCE)?;
    let (_, rest) = expect(rest, SEQUENCE)?;
    let (validity, _) = expect(rest, SEQUENCE)?;
    let (_, _, after_not_before) = element(validity)?;
    let (tag, text, _) = element(after_not_before)?;
    parse_time(tag, text)
}
