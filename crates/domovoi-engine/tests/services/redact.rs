use domovoi_core::ports::Redactor;
use domovoi_engine::PatternRedactor;
use secrecy::SecretString;

#[test]
fn pattern_redactor_removes_secrets_and_credential_shapes() {
    let redactor = PatternRedactor::new([
        SecretString::from("short".to_owned()),
        SecretString::from("short-and-longer".to_owned()),
        SecretString::from(String::new()),
    ]);
    let samples = [
        "Authorization: Bearer eyJabc.def/ghi+jk==",
        "key sk-abcdefghijklmnopqrstuvwxyz012345",
        "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
        &format!("github_pat_{}", "a".repeat(60)),
        "aws AKIAABCDEFGHIJKLMNOP",
        "slack xoxb-1234567890-abcdef",
        "hook https://discord.com/api/webhooks/123/abc-def",
        "jwt eyJhbGciOi.eyJzdWIiOi.c2lnbmF0dXJl",
        "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\n-----END RSA PRIVATE KEY-----",
    ];
    for sample in samples {
        let out = redactor.redact(sample);
        assert!(out.contains("[REDACTED]"), "{sample} -> {out}");
        assert_eq!(redactor.redact(&out), out, "idempotent for {sample}");
    }
    assert_eq!(
        redactor.redact("value short-and-longer and short"),
        "value [REDACTED] and [REDACTED]",
        "longest secret first"
    );
    assert_eq!(
        redactor.redact("password = hunter2, API_KEY: abc123 token=t0k"),
        "password = [REDACTED] API_KEY: [REDACTED] token=[REDACTED]"
    );
    assert_eq!(
        redactor.redact("nothing to hide here"),
        "nothing to hide here"
    );
}
