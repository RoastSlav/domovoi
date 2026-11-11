use domovoi_hub::csp::{BASE_POLICY, hash_source, inline_script_hashes, policy};
use sha2::{Digest, Sha256};

const PAGE: &str = r#"<!doctype html>
<html><head>
<!-- <script>commented()</script> -->
<script>
  var t = 1;
</script>
<script type="module" crossorigin src="/assets/app.js"></script>
<SCRIPT SRC='/a.js'></SCRIPT>
<script data-x="a>b">other()</script>
</head><body><div id="root"></div></body></html>"#;

fn sha256_base64(text: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(Sha256::digest(text.as_bytes()))
}

#[test]
fn only_inline_scripts_are_hashed_over_their_exact_text() {
    let hashes = inline_script_hashes(PAGE);
    assert_eq!(
        hashes,
        vec![
            format!(
                "'sha256-{}'",
                sha256_base64(
                    "
  var t = 1;
"
                )
            ),
            format!("'sha256-{}'", sha256_base64("other()")),
        ]
    );
}

#[test]
fn a_known_script_has_its_published_hash() {
    assert_eq!(
        hash_source(""),
        "'sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU='"
    );
}

#[test]
fn the_inline_theme_script_matches_the_hash_in_the_policy() {
    let hashes = inline_script_hashes(PAGE);
    let csp = policy(&hashes);
    assert!(csp.starts_with(BASE_POLICY));
    assert!(csp.contains(&format!("script-src 'self' {} {}", hashes[0], hashes[1])));
    assert!(!csp.contains("unsafe-eval"));
    assert!(!csp.contains("script-src 'unsafe-inline'"));
}

#[test]
fn a_page_without_inline_scripts_allows_only_the_hub_itself() {
    let hashes = inline_script_hashes(r#"<script type="module" src="/a.js"></script>"#);
    assert!(hashes.is_empty());
    assert_eq!(policy(&hashes), format!("{BASE_POLICY}; script-src 'self'"));
}

#[test]
fn an_unterminated_tag_yields_no_hash() {
    assert!(inline_script_hashes("<script ").is_empty());
}
