//! Splitting long text into Discord-sized messages.

/// The longest message Discord accepts, in characters.
pub const MAX_MESSAGE: usize = 2000;

/// `text` in parts of at most `max` characters, split between lines. A single line longer
/// than `max` is cut at character boundaries. Empty input gives no parts.
#[must_use]
pub fn split_message(text: &str, max: usize) -> Vec<String> {
    let max = max.max(1);
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut current_len = 0;
    for line in text.trim_end().split('\n') {
        let line_len = line.chars().count();
        let needed = if current.is_empty() {
            line_len
        } else {
            current_len + 1 + line_len
        };
        if needed <= max {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
            current_len = needed;
            continue;
        }
        if !current.is_empty() {
            parts.push(std::mem::take(&mut current));
        }
        if line_len <= max {
            current = line.to_owned();
            current_len = line_len;
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        let mut chunks = chars
            .chunks(max)
            .map(|c| c.iter().collect::<String>())
            .peekable();
        while let Some(chunk) = chunks.next() {
            if chunks.peek().is_some() {
                parts.push(chunk);
            } else {
                current_len = chunk.chars().count();
                current = chunk;
            }
        }
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_replies_are_split_at_2000_chars_on_line_boundaries() {
        let line = "x".repeat(99);
        let text = vec![line.as_str(); 50].join("\n");
        assert_eq!(text.chars().count(), 50 * 99 + 49);
        let parts = split_message(&text, MAX_MESSAGE);
        assert_eq!(parts.len(), 3);
        for part in &parts {
            assert!(part.chars().count() <= MAX_MESSAGE);
            assert!(part.split('\n').all(|l| l == line), "no line is cut");
        }
        assert_eq!(parts.join("\n"), text, "nothing is lost");
        assert_eq!(
            parts[0].split('\n').count(),
            20,
            "20 lines of 100 fill 1999 chars"
        );
    }

    #[test]
    fn short_text_is_one_part_and_empty_is_none() {
        assert_eq!(split_message("hello\nworld", MAX_MESSAGE), ["hello\nworld"]);
        assert!(split_message("", MAX_MESSAGE).is_empty());
        assert!(split_message("  \n", MAX_MESSAGE).is_empty());
    }

    #[test]
    fn an_overlong_line_is_cut_at_characters() {
        let text = format!("head\n{}\ntail", "é".repeat(4500));
        let parts = split_message(&text, MAX_MESSAGE);
        assert!(parts.iter().all(|p| p.chars().count() <= MAX_MESSAGE));
        assert_eq!(parts.concat().replace('\n', ""), text.replace('\n', ""));
        assert_eq!(parts[0], "head");
        assert!(parts.last().unwrap().ends_with("tail"));
    }
}
