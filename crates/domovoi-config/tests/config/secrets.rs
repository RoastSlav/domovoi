use std::io::Write;

use domovoi_config::{MapEnv, SecretSource};
use secrecy::ExposeSecret;

use crate::helpers::{assert_issue, hub_errors, hub_with};

const DISCORD_ENV: &str =
    "[discord]\nbot_token = { env = \"BOT\" }\nchannel_id = 1\nallowed_user_ids = [2]\n";

#[test]
fn inline_secret_is_rejected_with_path() {
    let errors =
        hub_errors("[discord]\nbot_token = \"abc123\"\nchannel_id = 1\nallowed_user_ids = [2]\n");
    assert_issue(
        &errors,
        "discord.bot_token",
        "secrets must use { env = .. } or { file = .. }",
    );
    assert!(errors.iter().all(|e| !e.message.contains("abc123")));
}

#[test]
fn secret_from_env_and_file_resolves() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "hook-from-file").unwrap();
    let path = file.path().to_string_lossy().replace('\\', "/");
    let text = format!("{DISCORD_ENV}webhook_url = {{ file = \"{path}\" }}\n");

    let env = MapEnv::new().with("BOT", "bot-from-env");
    let (config, _) = hub_with(&text, &env).unwrap();
    let discord = config.discord.unwrap();
    assert_eq!(discord.bot_token.value().expose_secret(), "bot-from-env");
    assert_eq!(
        discord.bot_token.source(),
        &SecretSource::Env("BOT".to_owned())
    );
    let hook = discord.webhook_url.unwrap();
    assert_eq!(hook.value().expose_secret(), "hook-from-file");
}

#[test]
fn missing_env_secret_reports_path() {
    let errors = hub_with(DISCORD_ENV, &MapEnv::new()).unwrap_err();
    assert_issue(
        &errors,
        "discord.bot_token",
        "environment variable BOT is not set",
    );
}

#[test]
fn missing_secret_file_reports_path() {
    let text = "[heartbeat]\nurl = { file = \"/nonexistent/domovoi-secret\" }\n";
    let errors = hub_with(text, &MapEnv::new()).unwrap_err();
    assert_issue(&errors, "heartbeat.url", "cannot read secret file");
}

#[test]
fn secret_table_must_name_exactly_one_source() {
    let errors = hub_errors("[heartbeat]\nurl = { env = \"A\", file = \"/b\" }\n");
    assert_issue(&errors, "heartbeat.url", "exactly one");
    let errors = hub_errors("[heartbeat]\nurl = { value = \"x\" }\n");
    assert_issue(&errors, "heartbeat.url", "unknown key `value`");
}

#[cfg(unix)]
#[test]
fn world_readable_secret_file_warns() {
    use std::os::unix::fs::PermissionsExt;

    let mut file = tempfile::NamedTempFile::new().unwrap();
    write!(file, "token").unwrap();
    std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
    let text = format!(
        "[heartbeat]\nurl = {{ file = \"{}\" }}\n",
        file.path().display()
    );
    let (_, warnings) = hub_with(&text, &MapEnv::new()).unwrap();
    assert_eq!(warnings.len(), 1, "{warnings:#?}");
    assert_eq!(warnings[0].path, "heartbeat.url");
    assert!(!warnings[0].is_error());
    assert!(warnings[0].message.contains("chmod 600"));

    std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o600)).unwrap();
    let (_, warnings) = hub_with(&text, &MapEnv::new()).unwrap();
    assert!(warnings.is_empty());
}

#[test]
fn secrets_do_not_appear_in_debug_output() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    write!(file, "file-secret-value").unwrap();
    let path = file.path().to_string_lossy().replace('\\', "/");
    let text = format!(
        "{DISCORD_ENV}webhook_url = {{ file = \"{path}\" }}\n\
         [llm.primary]\nkind = \"anthropic\"\nmodel = \"m\"\napi_key = {{ env = \"KEY\" }}\n\
         [webhooks.alerts]\nkind = \"generic\"\nauth = \"hmac\"\nsecret = {{ env = \"HOOK\" }}\n"
    );
    let env = MapEnv::new()
        .with("BOT", "bot-secret-value")
        .with("KEY", "key-secret-value")
        .with("HOOK", "hook-secret-value");
    let (config, _) = hub_with(&text, &env).unwrap();
    let debug = format!("{config:?}");
    for secret in [
        "bot-secret-value",
        "key-secret-value",
        "hook-secret-value",
        "file-secret-value",
    ] {
        assert!(!debug.contains(secret), "{secret} leaked into Debug output");
    }
    assert!(debug.contains("BOT"));
}
