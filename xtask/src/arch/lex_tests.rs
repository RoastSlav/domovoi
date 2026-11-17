//! The lexer blanks comments and literal contents without moving a single line break.

use super::*;

fn code(text: &str) -> String {
    lex(text).code
}

fn literals(text: &str) -> String {
    lex(text).literals
}

#[test]
fn both_views_keep_the_length_and_the_line_breaks() {
    let text = "let a = \"x\ny\"; /* c\n*/ let b = r#\"q\"#; // end\nlet c = 'z';\n";
    for view in [code(text), literals(text)] {
        assert_eq!(view.chars().count(), text.chars().count());
        assert_eq!(view.matches('\n').count(), text.matches('\n').count());
    }
}

#[test]
fn line_comments_are_blanked() {
    let out = code("let a = 1; // HumanApproval::new(x)\nlet b = 2;\n");
    assert!(!out.contains("HumanApproval"));
    assert!(out.contains("let a = 1;") && out.contains("let b = 2;"));
}

#[test]
fn doc_comments_are_comments() {
    assert!(!code("/// uses std::fs\n//! and std::net\nfn f() {}\n").contains("std::"));
}

#[test]
fn code_after_a_block_comment_on_the_same_line_survives() {
    let out = code("/* explanation */ let approval = HumanApproval::new(user);\n");
    assert!(out.contains("HumanApproval::new(user)"));
    assert!(!out.contains("explanation"));
}

#[test]
fn a_leading_star_is_code() {
    let out = code("    *slot = HumanApproval::new(user);\n");
    assert!(out.contains("*slot = HumanApproval::new(user);"));
}

#[test]
fn block_comments_nest_and_span_lines() {
    let out =
        code("/* a /* HumanApproval::new */ still comment\n more */ let x = 1;\nlet y = 2;\n");
    assert!(!out.contains("HumanApproval") && !out.contains("still comment"));
    assert!(out.contains("let x = 1;") && out.contains("let y = 2;"));
}

#[test]
fn comment_markers_inside_strings_are_not_comments() {
    let text = "let url = \"http://host/*path*/\"; let a = HumanApproval::new(u);\n";
    let out = literals(text);
    assert!(out.contains("http://host/*path*/"));
    assert!(out.contains("HumanApproval::new(u)"));
}

#[test]
fn string_contents_are_blank_in_code_and_kept_in_literals() {
    let text = "let s = \"HumanApproval::new(\";\n";
    assert!(!code(text).contains("HumanApproval"));
    assert!(literals(text).contains("HumanApproval::new("));
    assert!(code(text).contains("let s = \""), "the quotes stay");
}

#[test]
fn escaped_quotes_and_backslashes_do_not_end_a_string() {
    let text = "let s = \"a\\\"b // not a comment\"; let t = \"c\\\\\"; let real = 1;\n";
    let out = literals(text);
    assert!(out.contains("not a comment"));
    assert!(out.contains("let real = 1;"));
    assert!(code(text).contains("let real = 1;"));
}

#[test]
fn raw_strings_with_any_number_of_hashes() {
    let text = "let a = r\"// x\"; let b = r#\"/* \" y\"#; let c = r###\"\"## z\"###; let d = 1;\n";
    let out = literals(text);
    assert!(out.contains("// x") && out.contains("/* \" y") && out.contains("\"## z"));
    assert!(out.contains("let d = 1;"));
    assert!(!code(text).contains(" y") && !code(text).contains(" z"));
}

#[test]
fn byte_and_c_strings() {
    let text = "let a = b\"// x\"; let b = br#\"/* y\"#; let c = c\"z\"; let d = 1;\n";
    let out = literals(text);
    assert!(out.contains("// x") && out.contains("/* y") && out.contains("let d = 1;"));
    assert!(!code(text).contains(" x") && !code(text).contains(" y"));
}

#[test]
fn a_prefix_letter_inside_a_name_is_not_a_literal_prefix() {
    let text = "let bar = 1; let cr = 2; let xb = \"s\"; let near = 3;\n";
    assert_eq!(literals(text), text);
}

#[test]
fn character_literals_and_lifetimes() {
    let text = "let q = '\"'; let s = '\\''; let b = b'/'; let n = '\\n'; fn f<'a>(x: &'a str) {} let r = 1;\n";
    let out = code(text);
    assert!(
        out.contains("fn f<'a>(x: &'a str)"),
        "lifetimes are code: {out}"
    );
    assert!(out.contains("let r = 1;"));
    // The quote character literal did not start a string that swallows the rest.
    assert!(literals(text).contains("let r = 1;"));
}

#[test]
fn a_slash_in_a_char_literal_is_not_a_comment() {
    let out = code("let c = '/'; let d = '*'; let live = HumanApproval::new(u);\n");
    assert!(out.contains("HumanApproval::new(u)"));
}

#[test]
fn multi_line_strings_keep_their_line_numbers() {
    let text = "let s = \"line one\nHumanApproval::new(x)\nline three\";\nlet after = 1;\n";
    let out = code(text);
    assert_eq!(out.lines().count(), text.lines().count());
    assert!(!out.contains("HumanApproval"));
    assert_eq!(out.lines().nth(3), Some("let after = 1;"));
}

#[test]
fn unterminated_input_does_not_panic() {
    for text in ["/* open", "\"open", "r#\"open", "'", "b'", "'\\", "r", "r#"] {
        let _ = lex(text);
    }
}
