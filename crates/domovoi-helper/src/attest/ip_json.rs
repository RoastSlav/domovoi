//! Pure parsers from the JSON of `ip -N -j` to typed values.
//!
//! Only documented keys are read. A rule is accepted only when every key it carries is
//! understood: a key this parser does not know makes the rule unrepresentable instead of being
//! ignored, because a rule that is half understood must never be hashed as if it were whole. A
//! table dump may carry keys the report does not use (`protocol`, `scope`, `prefsrc`, `flags`);
//! those are skipped, but the keys the report needs must be present.
//!
//! With `-N` the kernel's numbers print as strings: a table is `"254"`, a route type is `"1"`
//! (unicast), an `ipproto` is `"ipproto-6"`. A route dump omits `type` for a unicast route and
//! omits `metric` when it is 0. A host route prints a bare address as `dst`, the default route
//! prints `"default"`, and a lookup (`route get -d`) prints the queried address as `dst`.

use core::net::IpAddr;

use domovoi_core::attest::{Cidr, Family, Selector, check_ifname};
use serde_json::{Map, Value};

use super::observe::{IfAddress, Lookup, PolicyRule, ReadError, RouteKind, RuleAction, TableRoute};

fn partial(why: &str) -> ReadError {
    ReadError::Partial(why.to_owned())
}

fn unrepresentable(why: impl Into<String>) -> ReadError {
    ReadError::Unrepresentable(why.into())
}

fn array(text: &str) -> Result<Vec<Map<String, Value>>, ReadError> {
    let value: Value = serde_json::from_str(text).map_err(|_| partial("not valid JSON"))?;
    let Value::Array(items) = value else {
        return Err(partial("not a JSON array"));
    };
    items
        .into_iter()
        .map(|item| match item {
            Value::Object(object) => Ok(object),
            _ => Err(partial("an element is not an object")),
        })
        .collect()
}

/// An unsigned number the kernel prints either as a JSON number or as a string of digits.
fn number(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64(),
        Value::String(s)
            if !s.is_empty()
                && s.bytes().all(|b| b.is_ascii_digit())
                && (s == "0" || !s.starts_with('0')) =>
        {
            s.parse().ok()
        }
        _ => None,
    }
}

fn u32_of(value: &Value, what: &str) -> Result<u32, ReadError> {
    number(value)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| partial(&format!("{what} is not a number")))
}

fn text<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

/// An interface name as bytes. A replacement character means the kernel's name was not UTF-8,
/// which iproute2's JSON cannot carry faithfully.
fn ifname(value: &Value) -> Result<Vec<u8>, ReadError> {
    let name = value
        .as_str()
        .ok_or_else(|| partial("an interface name is not a string"))?;
    if name.contains('\u{fffd}') {
        return Err(unrepresentable("a non-UTF-8 interface name"));
    }
    check_ifname(name.as_bytes()).map_err(|_| unrepresentable("an interface name length"))?;
    Ok(name.as_bytes().to_vec())
}

fn ip(text: &str, what: &str) -> Result<IpAddr, ReadError> {
    text.parse()
        .map_err(|_| partial(&format!("{what} is not an address")))
}

fn hex_u32(text: &str) -> Option<u32> {
    let digits = text.strip_prefix("0x")?;
    if digits.is_empty() || digits.len() > 8 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16).ok()
}

fn prefix_of(object: &Map<String, Value>, address: &str, len_key: &str) -> Result<Cidr, ReadError> {
    let addr = ip(address, "a rule address")?;
    let bits = match object.get(len_key) {
        Some(value) => number(value)
            .and_then(|n| u8::try_from(n).ok())
            .ok_or_else(|| partial("a prefix length is not a number"))?,
        None => Cidr::host(addr).prefix,
    };
    let cidr = Cidr::new(addr, bits).map_err(|_| partial("a prefix length is too large"))?;
    Ok(cidr.network())
}

/// Keys a rule may carry that mean the report cannot express it.
const OTHER_ACTION_KEYS: [&str; 3] = ["action", "goto", "suppress_prefixlen"];

fn range16(
    object: &Map<String, Value>,
    single: &str,
    start: &str,
    end: &str,
    mask: &str,
) -> Result<Option<(u16, u16)>, ReadError> {
    let get16 = |key: &str| -> Result<Option<u16>, ReadError> {
        object
            .get(key)
            .map(|v| {
                number(v)
                    .and_then(|n| u16::try_from(n).ok())
                    .ok_or_else(|| partial("a port is not a number"))
            })
            .transpose()
    };
    if let (Some(a), Some(b)) = (get16(start)?, get16(end)?) {
        return Ok(Some((a, b)));
    }
    let Some(port) = get16(single)? else {
        return Ok(None);
    };
    let full = object
        .get(mask)
        .and_then(Value::as_str)
        .is_none_or(|m| hex_u32(m) == Some(0xffff));
    if full {
        Ok(Some((port, port)))
    } else {
        Err(unrepresentable("a port match with a partial mask"))
    }
}

fn selector_of(object: &Map<String, Value>) -> Result<Selector, ReadError> {
    let mut selector = Selector::default();
    if let Some(src) = text(object, "src").filter(|s| *s != "all") {
        selector.from = Some(prefix_of(object, src, "srclen")?);
    }
    if let Some(dst) = text(object, "dst").filter(|s| *s != "all") {
        selector.to = Some(prefix_of(object, dst, "dstlen")?);
    }
    selector.iif = object.get("iif").map(ifname).transpose()?;
    selector.oif = object.get("oif").map(ifname).transpose()?;
    if let Some(mark) = object.get("fwmark") {
        let value = mark
            .as_str()
            .and_then(hex_u32)
            .ok_or_else(|| partial("fwmark is not hex"))?;
        let mask = match object.get("fwmask") {
            Some(m) => m
                .as_str()
                .and_then(hex_u32)
                .ok_or_else(|| partial("fwmask is not hex"))?,
            None => u32::MAX,
        };
        selector.fwmark = Some((value, mask));
    }
    if let Some(proto) = object.get("ipproto") {
        let digits = proto.as_str().map_or_else(
            || number(proto).map(|n| n.to_string()),
            |s| Some(s.strip_prefix("ipproto-").unwrap_or(s).to_owned()),
        );
        selector.ipproto = digits
            .and_then(|d| d.parse::<u8>().ok())
            .map(Some)
            .ok_or_else(|| unrepresentable("an ipproto that is not a number"))?;
    }
    selector.sport = range16(object, "sport", "sport_start", "sport_end", "sport_mask")?;
    selector.dport = range16(object, "dport", "dport_start", "dport_end", "dport_mask")?;
    if let (Some(a), Some(b)) = (object.get("uid_start"), object.get("uid_end")) {
        selector.uidrange = Some((u32_of(a, "uid_start")?, u32_of(b, "uid_end")?));
    }
    selector.not = object.contains_key("not");
    Ok(selector)
}

const RULE_KEYS: [&str; 21] = [
    "priority",
    "src",
    "srclen",
    "dst",
    "dstlen",
    "iif",
    "oif",
    "fwmark",
    "fwmask",
    "ipproto",
    "sport",
    "sport_start",
    "sport_end",
    "sport_mask",
    "dport",
    "dport_start",
    "dport_end",
    "dport_mask",
    "uid_start",
    "uid_end",
    "not",
];

/// Parses the output of `ip -N -j -4 rule show` or `-6`.
///
/// # Errors
///
/// [`ReadError::Partial`] for malformed JSON or a missing `priority` or `table`;
/// [`ReadError::Unrepresentable`] for a key this parser does not know or a value that cannot be
/// expressed. A rule whose action is not a lookup is returned with
/// [`RuleAction::Other`], for the caller to refuse.
pub fn parse_rules(json: &str, family: Family) -> Result<Vec<PolicyRule>, ReadError> {
    array(json)?
        .into_iter()
        .map(|object| {
            let priority = u32_of(
                object
                    .get("priority")
                    .ok_or_else(|| partial("a rule has no priority"))?,
                "priority",
            )?;
            if let Some(key) = OTHER_ACTION_KEYS.iter().find(|k| object.contains_key(**k)) {
                return Ok(PolicyRule {
                    family,
                    priority,
                    selector: Selector::default(),
                    action: RuleAction::Other((*key).to_owned()),
                    table: 0,
                });
            }
            if let Some(key) = object
                .keys()
                .find(|k| !RULE_KEYS.contains(&k.as_str()) && k.as_str() != "table")
            {
                return Err(unrepresentable(format!("rule key {key}")));
            }
            let table = u32_of(
                object
                    .get("table")
                    .ok_or_else(|| partial("a rule has no table"))?,
                "table",
            )?;
            Ok(PolicyRule {
                family,
                priority,
                selector: selector_of(&object)?,
                action: RuleAction::Lookup,
                table,
            })
        })
        .collect()
}

fn kind_of(object: &Map<String, Value>) -> RouteKind {
    match object.get("type") {
        None => RouteKind::Unicast,
        Some(value) => {
            let unicast = value.as_str() == Some("unicast") || number(value) == Some(1);
            if unicast {
                RouteKind::Unicast
            } else {
                RouteKind::Other(match value {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
            }
        }
    }
}

fn dst_of(text: &str, family: Family) -> Result<Cidr, ReadError> {
    if text == "default" {
        let any = match family {
            Family::Inet => IpAddr::from([0, 0, 0, 0]),
            Family::Inet6 => IpAddr::from([0_u16; 8]),
        };
        return Cidr::new(any, 0).map_err(|_| partial("default prefix"));
    }
    match text.split_once('/') {
        Some((addr, bits)) => {
            let bits = bits
                .parse::<u8>()
                .map_err(|_| partial("a route prefix length is not a number"))?;
            Cidr::new(ip(addr, "a route destination")?, bits)
                .map(|c| c.network())
                .map_err(|_| partial("a route prefix length is too large"))
        }
        None => Ok(Cidr::host(ip(text, "a route destination")?)),
    }
}

fn gateway_of(object: &Map<String, Value>) -> Result<Option<IpAddr>, ReadError> {
    text(object, "gateway")
        .map(|g| ip(g, "a gateway"))
        .transpose()
}

fn route_of(
    object: &Map<String, Value>,
    family: Family,
    table: u32,
) -> Result<TableRoute, ReadError> {
    let dst = dst_of(
        text(object, "dst").ok_or_else(|| partial("a route has no dst"))?,
        family,
    )?;
    let multipath = object.contains_key("nexthops") || object.contains_key("nhid");
    let kind = kind_of(object);
    let ifname = match object.get("dev") {
        Some(dev) => ifname(dev)?,
        None if multipath || kind != RouteKind::Unicast => Vec::new(),
        None => return Err(partial("a unicast route has no dev")),
    };
    Ok(TableRoute {
        family,
        table,
        dst,
        gateway: gateway_of(object)?,
        ifname,
        metric: object
            .get("metric")
            .map(|m| u32_of(m, "metric"))
            .transpose()?
            .unwrap_or(0),
        kind,
        multipath,
    })
}

/// Parses the output of `ip -N -j -4|-6 route show table <table>`.
///
/// # Errors
///
/// [`ReadError::Partial`] for malformed JSON or a missing `dst` or `dev`;
/// [`ReadError::Unrepresentable`] for a non-UTF-8 or over-long interface name.
pub fn parse_routes(json: &str, family: Family, table: u32) -> Result<Vec<TableRoute>, ReadError> {
    array(json)?
        .iter()
        .map(|object| route_of(object, family, table))
        .collect()
}

/// Parses the output of `ip -N -j -d -4|-6 route get <address>`.
///
/// The output must hold exactly one route and name its type, table and source address.
///
/// # Errors
///
/// [`ReadError::Partial`] for malformed JSON or a missing key.
pub fn parse_lookup(json: &str, endpoint: IpAddr) -> Result<Lookup, ReadError> {
    let mut items = array(json)?;
    let (Some(object), None) = (items.pop(), items.pop()) else {
        return Err(partial("a lookup must print exactly one route"));
    };
    if !object.contains_key("type") {
        return Err(partial("a lookup prints no type"));
    }
    let table = u32_of(
        object
            .get("table")
            .ok_or_else(|| partial("a lookup prints no table"))?,
        "table",
    )?;
    let src = ip(
        text(&object, "prefsrc").ok_or_else(|| partial("a lookup prints no source address"))?,
        "the source address",
    )?;
    let route = route_of(&object, Family::of(endpoint), table)?;
    Ok(Lookup {
        endpoint,
        table,
        gateway: route.gateway,
        ifname: route.ifname,
        src,
        metric: route.metric,
        kind: route.kind,
        multipath: route.multipath,
    })
}

/// Parses the output of `ip -N -j addr show`.
///
/// # Errors
///
/// [`ReadError::Partial`] for malformed JSON or a missing key;
/// [`ReadError::Unrepresentable`] for a non-UTF-8 interface name.
pub fn parse_addresses(json: &str) -> Result<Vec<IfAddress>, ReadError> {
    let mut out = Vec::new();
    for link in array(json)? {
        let name = ifname(
            link.get("ifname")
                .ok_or_else(|| partial("a link has no ifname"))?,
        )?;
        let infos = link
            .get("addr_info")
            .and_then(Value::as_array)
            .ok_or_else(|| partial("a link has no addr_info"))?;
        for info in infos {
            let info = info.as_object().ok_or_else(|| partial("addr_info entry"))?;
            let addr = ip(
                text(info, "local").ok_or_else(|| partial("an address has no local"))?,
                "an interface address",
            )?;
            let bits = info
                .get("prefixlen")
                .and_then(number)
                .and_then(|n| u8::try_from(n).ok())
                .ok_or_else(|| partial("an address has no prefixlen"))?;
            out.push(IfAddress {
                addr: Cidr::new(addr, bits).map_err(|_| partial("an address prefix length"))?,
                ifname: name.clone(),
            });
        }
    }
    Ok(out)
}
