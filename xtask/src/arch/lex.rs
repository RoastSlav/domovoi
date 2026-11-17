//! A small Rust lexer for the line rules in [`super::scan`] (INV-1, R2, R3, R4).
//!
//! It does one job: say which characters are comments, which are inside string or character
//! literals, and which are code. It produces two views of the text with the same length and
//! the same line breaks as the original, so a line number in a view is a line number in the
//! file:
//!
//! - `code`: comments and the insides of literals are replaced by spaces. Rules that look for
//!   a call or a path read this, so a mention in prose or in a string is not code.
//! - `literals`: comments are replaced by spaces, literals are kept. Rules that look for a
//!   string (`"sh"`, `plan:v2`) read this.
//!
//! Handled: `//` comments, nested `/* */` comments, `"..."` strings with escapes (they may
//! span lines), raw strings `r"..."` and `r#"..."#` with any number of hashes, byte and C
//! strings (`b"..."`, `br#"..."#`, `c"..."`), character and byte-character literals, and
//! lifetimes, which are not literals. Whitespace and newlines are always kept.

/// The two views of one source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lexed {
    /// Comments and literal contents blanked.
    pub code: String,
    /// Comments blanked, literals kept.
    pub literals: String,
}

/// Blank comments and literal contents in `text`. See the module documentation.
pub fn lex(text: &str) -> Lexed {
    let chars: Vec<char> = text.chars().collect();
    let mut code = String::with_capacity(text.len());
    let mut literals = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            let end = chars[i..]
                .iter()
                .position(|&ch| ch == '\n')
                .map_or(chars.len(), |p| i + p);
            blank(&chars[i..end], &mut code, &mut literals);
            i = end;
        } else if c == '/' && next == Some('*') {
            let end = block_comment_end(&chars, i);
            blank(&chars[i..end], &mut code, &mut literals);
            i = end;
        } else if let Some((open, end)) = raw_string(&chars, i) {
            literal(&chars[i..end], open, &mut code, &mut literals);
            i = end;
        } else if let Some((open, end)) = quoted_string(&chars, i) {
            literal(&chars[i..end], open, &mut code, &mut literals);
            i = end;
        } else if let Some((open, end)) = char_literal(&chars, i) {
            literal(&chars[i..end], open, &mut code, &mut literals);
            i = end;
        } else {
            code.push(c);
            literals.push(c);
            i += 1;
        }
    }
    Lexed { code, literals }
}

/// Replace every character but line breaks with a space, in both views.
fn blank(span: &[char], code: &mut String, literals: &mut String) {
    for &ch in span {
        let out = if ch == '\n' { '\n' } else { ' ' };
        code.push(out);
        literals.push(out);
    }
}

/// Keep a literal in `literals`; in `code` keep its opening and closing delimiters (the first
/// `open` characters and the last character) and blank what is between them.
fn literal(span: &[char], open: usize, code: &mut String, literals: &mut String) {
    for (index, &ch) in span.iter().enumerate() {
        literals.push(ch);
        let delimiter = index < open || index + 1 == span.len();
        code.push(if delimiter || ch == '\n' { ch } else { ' ' });
    }
}

/// The index after the block comment that starts at `start`, counting nesting. An unclosed
/// comment runs to the end of the text.
fn block_comment_end(chars: &[char], start: usize) -> usize {
    let mut depth = 0usize;
    let mut i = start;
    while i < chars.len() {
        match (chars[i], chars.get(i + 1)) {
            ('/', Some('*')) => {
                depth += 1;
                i += 2;
            }
            ('*', Some('/')) => {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    return i;
                }
            }
            _ => i += 1,
        }
    }
    chars.len()
}

/// Whether the character before `i` continues an identifier, so a prefix letter at `i` is
/// the tail of a longer name rather than a literal prefix.
fn after_identifier(chars: &[char], i: usize) -> bool {
    i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_')
}

/// A raw string at `i` (`r"`, `r#"`, `br"`, `cr#"`): the number of opening characters and the
/// index after the closing delimiter.
fn raw_string(chars: &[char], i: usize) -> Option<(usize, usize)> {
    if after_identifier(chars, i) {
        return None;
    }
    let mut j = i;
    if matches!(chars.get(j), Some('b' | 'c')) {
        j += 1;
    }
    if chars.get(j) != Some(&'r') {
        return None;
    }
    j += 1;
    let hashes = chars[j..].iter().take_while(|&&ch| ch == '#').count();
    j += hashes;
    if chars.get(j) != Some(&'"') {
        return None;
    }
    let open = j + 1 - i;
    let mut k = j + 1;
    while k < chars.len() {
        if chars[k] == '"' && chars[k + 1..].iter().take_while(|&&ch| ch == '#').count() >= hashes {
            return Some((open, k + 1 + hashes));
        }
        k += 1;
    }
    Some((open, chars.len()))
}

/// A `"..."`, `b"..."` or `c"..."` string at `i`: the number of opening characters and the
/// index after the closing quote.
fn quoted_string(chars: &[char], i: usize) -> Option<(usize, usize)> {
    let mut j = i;
    if matches!(chars.get(j), Some('b' | 'c')) && !after_identifier(chars, i) {
        j += 1;
    }
    if chars.get(j) != Some(&'"') {
        return None;
    }
    let open = j + 1 - i;
    let mut k = j + 1;
    while k < chars.len() {
        match chars[k] {
            '\\' => k += 2,
            '"' => return Some((open, k + 1)),
            _ => k += 1,
        }
    }
    Some((open, chars.len()))
}

/// A `'x'`, `'\n'` or `b'x'` literal at `i`. A `'` that does not close after one character or
/// one escape is a lifetime or label and is code.
fn char_literal(chars: &[char], i: usize) -> Option<(usize, usize)> {
    let mut j = i;
    if chars.get(j) == Some(&'b') && !after_identifier(chars, i) {
        j += 1;
    }
    if chars.get(j) != Some(&'\'') {
        return None;
    }
    let open = j + 1 - i;
    match chars.get(j + 1)? {
        '\\' => {
            let close = chars[j + 2..].iter().position(|&ch| ch == '\'')?;
            let end = j + 2 + close + 1;
            // An escape is short; a far quote belongs to something else.
            (end - j <= 12).then_some((open, end))
        }
        '\'' | '\n' => None,
        _ => (chars.get(j + 2) == Some(&'\'')).then_some((open, j + 3)),
    }
}

#[cfg(test)]
#[path = "lex_tests.rs"]
mod tests;
