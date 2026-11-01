//! Discord ids (64-bit "snowflakes") written as integers in `hub.toml` or as strings by
//! the web UI, where JavaScript numbers cannot hold them exactly.

use serde::de::{self, Deserializer, SeqAccess, Visitor};

struct One;

impl Visitor<'_> for One {
    type Value = u64;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a Discord id, as a number or a string of digits")
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<u64, E> {
        Ok(v)
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<u64, E> {
        u64::try_from(v).map_err(|_| E::custom("a Discord id cannot be negative"))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<u64, E> {
        v.trim()
            .parse()
            .map_err(|_| E::custom("a Discord id is a string of digits"))
    }
}

/// One id.
pub(crate) fn one<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    deserializer.deserialize_any(One)
}

struct Many;

impl<'de> Visitor<'de> for Many {
    type Value = Vec<u64>;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a list of Discord ids")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u64>, A::Error> {
        let mut out = Vec::new();
        while let Some(id) = seq.next_element::<Id>()? {
            out.push(id.0);
        }
        Ok(out)
    }
}

struct Id(u64);

impl<'de> serde::Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        one(deserializer).map(Id)
    }
}

/// A list of ids.
pub(crate) fn many<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u64>, D::Error> {
    deserializer.deserialize_seq(Many)
}
