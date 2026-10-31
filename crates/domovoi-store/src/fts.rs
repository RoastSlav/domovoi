//! Turning user text into a safe FTS5 query.
//!
//! The text is split into runs of letters and digits, the same words the `unicode61`
//! tokenizer produces, and each word is quoted, so no FTS5 operator (`OR`, `NEAR`, `*`,
//! `^`, column filters, quotes) can come from user input. Words are ANDed.

/// The words of `text`: runs of letters and digits, lowercased.
#[must_use]
pub fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// An FTS5 query matching rows that contain every word of `text`, or `None` when `text`
/// has no words (which matches nothing).
#[must_use]
pub fn match_all(text: &str) -> Option<String> {
    let quoted: Vec<String> = words(text).iter().map(|w| format!("\"{w}\"")).collect();
    (!quoted.is_empty()).then(|| quoted.join(" "))
}

/// Like [`match_all`], restricted to one column.
#[must_use]
pub fn match_all_in(column: &str, text: &str) -> Option<String> {
    match_all(text).map(|query| format!("{{{column}}} : ({query})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operators_become_quoted_words() {
        assert_eq!(
            match_all("\"NEAR(\" OR zfs*").as_deref(),
            Some("\"near\" \"or\" \"zfs\"")
        );
        assert_eq!(
            match_all("pool_degraded").as_deref(),
            Some("\"pool\" \"degraded\"")
        );
        assert_eq!(match_all(" ^-:() "), None);
        assert_eq!(
            match_all_in("title", "a").as_deref(),
            Some("{title} : (\"a\")")
        );
    }
}
