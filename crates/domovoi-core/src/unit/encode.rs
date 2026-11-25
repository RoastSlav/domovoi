//! Length-framed encoding: every value is written `name:len:value`, so no two inputs share
//! an encoding and nothing is concatenated unframed.

use std::collections::BTreeSet;

/// A builder for one framed text, domain tag first.
pub struct Frames {
    out: Vec<u8>,
}

impl Frames {
    /// Starts a text with `domain` as its first line.
    #[must_use]
    pub fn new(domain: &str) -> Self {
        let mut out = domain.as_bytes().to_vec();
        out.push(b'\n');
        Self { out }
    }

    /// One framed value.
    pub fn value(&mut self, name: &str, value: &str) {
        self.out
            .extend_from_slice(format!("{name}:{}:", value.len()).as_bytes());
        self.out.extend_from_slice(value.as_bytes());
        self.out.push(b'\n');
    }

    /// A count line, written before the elements of a list.
    pub fn count(&mut self, name: &str, count: usize) {
        self.value(name, &count.to_string());
    }

    /// A list: its count, then each element framed, in the given order.
    pub fn list(&mut self, name: &str, items: &[String]) {
        self.count(name, items.len());
        for item in items {
            self.value("item", item);
        }
    }

    /// A set: its count, then each element framed in sorted order.
    pub fn set(&mut self, name: &str, items: &BTreeSet<String>) {
        self.count(name, items.len());
        for item in items {
            self.value("item", item);
        }
    }

    /// The finished bytes.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.out
    }
}
