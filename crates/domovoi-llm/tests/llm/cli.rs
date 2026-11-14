use std::sync::Arc;

use domovoi_core::ports::{
    LlmError, LlmMessage, Part, ProcessError, ProcessExecutor, ProcessOutput, Role, StopReason,
    UsageSource,
};
use domovoi_llm::{
    CliOutput, CliPreset, EXCERPT_CHARS, LlmBuildError, Price, PromptVia, ProviderKind,
    ProviderSettings, build,
};
use domovoi_testkit::FakeProcessExecutor;
use jiff::SignedDuration;

use crate::support::ask;

const CLAUDE: &str = "/usr/local/bin/claude";

fn claude_args() -> Vec<&'static str> {
    vec![
        "-p",
        "--output-format",
        "json",
        "--tools",
        "",
        "--strict-mcp-config",
        "--no-session-persistence",
    ]
}

fn settings(preset: CliPreset) -> ProviderSettings {
    let mut settings = ProviderSettings::new(ProviderKind::Cli);
    settings.cli = Some(preset);
    settings.timeout = SignedDuration::from_secs(90);
    settings
}

fn ok(stdout: &str) -> ProcessOutput {
    ProcessOutput {
        exit_code: Some(0),
        stdout: stdout.to_owned(),
        ..ProcessOutput::default()
    }
}

fn provider(
    fake: &Arc<FakeProcessExecutor>,
    preset: CliPreset,
) -> Arc<dyn domovoi_core::ports::LlmProvider> {
    let executor: Arc<dyn ProcessExecutor> = fake.clone();
    build(&settings(preset), Some(executor)).unwrap()
}

const PRICE: Price = Price {
    input_per_mtok_usd: 2.0,
    output_per_mtok_usd: 10.0,
};

fn priced_provider(
    fake: &Arc<FakeProcessExecutor>,
    preset: CliPreset,
) -> Arc<dyn domovoi_core::ports::LlmProvider> {
    let executor: Arc<dyn ProcessExecutor> = fake.clone();
    let mut settings = settings(preset);
    settings.price = Some(PRICE);
    build(&settings, Some(executor)).unwrap()
}

/// Runs one CLI turn whose JSON output carries `usage`, through a priced provider.
async fn with_usage(usage: &str) -> domovoi_core::ports::LlmResponse {
    let stdout = format!(r#"{{"type":"result","is_error":false,"result":"ok","usage":{usage}}}"#);
    let fake = Arc::new(FakeProcessExecutor::new());
    fake.expect(CLAUDE, &claude_args(), ok(&stdout));
    priced_provider(&fake, CliPreset::claude(CLAUDE))
        .complete(ask("triage"))
        .await
        .unwrap()
}

#[tokio::test]
async fn cli_preset_builds_argv_and_passes_prompt_via_stdin() {
    let fake = Arc::new(FakeProcessExecutor::new());
    fake.expect(CLAUDE, &claude_args(), ok(r#"{"result": "ok"}"#));
    let cli = provider(&fake, CliPreset::claude(CLAUDE));
    assert_eq!(cli.id(), "cli");
    assert!(!cli.supports_tools());

    let mut req = ask("Why is nas down?");
    req.messages.push(LlmMessage {
        role: Role::Assistant,
        parts: vec![Part::Text("Probably the disk.".to_owned())],
    });
    cli.complete(req).await.unwrap();

    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    let spec = &calls[0];
    assert_eq!(spec.program, CLAUDE);
    assert_eq!(spec.args, claude_args());
    assert_eq!(spec.timeout, SignedDuration::from_secs(90));
    assert!(spec.env.is_empty(), "the environment starts empty");
    assert_eq!(
        spec.stdin.as_deref(),
        Some("You triage incidents.\n\nUser:\nWhy is nas down?\n\nAssistant:\nProbably the disk.")
    );

    let custom = CliPreset {
        program: "/opt/llm/run".to_owned(),
        args: vec![
            "--ask".to_owned(),
            "{prompt}".to_owned(),
            "--quiet".to_owned(),
        ],
        prompt_via: PromptVia::Arg,
        output: CliOutput::Text,
        env: [("HOME".to_owned(), "/var/lib/domovoi".to_owned())].into(),
    };
    let prompt = "You triage incidents.\n\nUser:\nhi";
    fake.expect("/opt/llm/run", &["--ask", prompt, "--quiet"], ok("hello\n"));
    let answer = provider(&fake, custom).complete(ask("hi")).await.unwrap();
    assert_eq!(answer.parts, vec![Part::Text("hello".to_owned())]);
    let spec = fake.calls().pop().unwrap();
    assert_eq!(spec.stdin, None);
    assert_eq!(
        spec.env.get("HOME").map(String::as_str),
        Some("/var/lib/domovoi")
    );

    let replaced = CliPreset::claude(CLAUDE).with_args(vec!["--print".to_owned()]);
    assert_eq!(replaced.args, vec!["--print"]);
    assert_eq!(
        CliPreset::claude(CLAUDE).with_args(Vec::new()).args,
        claude_args()
    );

    let executor: Arc<dyn ProcessExecutor> = fake.clone();
    assert_eq!(
        build(&settings(CliPreset::claude("claude")), Some(executor)).err(),
        Some(LlmBuildError::RelativeProgram("claude".to_owned()))
    );
}

#[tokio::test]
async fn cli_json_output_extracts_result_field() {
    let fake = Arc::new(FakeProcessExecutor::new());
    fake.expect(
        CLAUDE,
        &claude_args(),
        ok(r#"{"type":"result","is_error":false,"result":"  {\"summary\":\"disk full\"}\n","usage":{"input_tokens":812,"output_tokens":64},"total_cost_usd":0.01}"#),
    );
    let response = provider(&fake, CliPreset::claude(CLAUDE))
        .complete(ask("triage"))
        .await
        .unwrap();
    assert_eq!(
        response.parts,
        vec![Part::Text(r#"{"summary":"disk full"}"#.to_owned())]
    );
    assert_eq!(response.stop, StopReason::EndTurn);
    assert_eq!(response.usage.input_tokens, 812);
    assert_eq!(response.usage.output_tokens, 64);
    assert_eq!(response.usage.cost_micros, None);

    for (stdout, expected) in [
        (r#"{"type":"result"}"#, "no text field `result`"),
        ("Not JSON at all", "not JSON"),
    ] {
        let fake = Arc::new(FakeProcessExecutor::new());
        fake.expect(CLAUDE, &claude_args(), ok(stdout));
        let err = provider(&fake, CliPreset::claude(CLAUDE))
            .complete(ask("triage"))
            .await
            .unwrap_err();
        assert!(
            matches!(&err, LlmError::InvalidResponse(m) if m.contains(expected)),
            "{err:?}"
        );
    }
}

#[tokio::test]
async fn cli_failure_reports_redacted_stderr() {
    let stderr = format!("Error: not logged in\u{1b}[0m\n{}", "x".repeat(2000));
    let fake = Arc::new(FakeProcessExecutor::new());
    fake.expect(
        CLAUDE,
        &claude_args(),
        ProcessOutput {
            exit_code: Some(1),
            stderr: stderr.clone(),
            ..ProcessOutput::default()
        },
    );
    let err = provider(&fake, CliPreset::claude(CLAUDE))
        .complete(ask("triage"))
        .await
        .unwrap_err();
    let LlmError::Unavailable(message) = &err else {
        panic!("{err:?}");
    };
    assert!(message.contains("exit code 1"), "{message}");
    assert!(message.contains("Error: not logged in"), "{message}");
    assert!(
        !message.contains('\u{1b}'),
        "control characters are removed"
    );
    let (_, excerpt) = message.split_once(": ").unwrap();
    assert_eq!(excerpt.chars().count(), EXCERPT_CHARS);
    assert!(excerpt.ends_with('…'));

    let fake = Arc::new(FakeProcessExecutor::new());
    fake.expect(
        CLAUDE,
        &claude_args(),
        ProcessOutput {
            timed_out: true,
            ..ProcessOutput::default()
        },
    );
    assert!(matches!(
        provider(&fake, CliPreset::claude(CLAUDE)).complete(ask("triage")).await,
        Err(LlmError::Unavailable(m)) if m.contains("timed out")
    ));

    let fake = Arc::new(FakeProcessExecutor::new());
    fake.expect_result(CLAUDE, &claude_args(), Err(ProcessError::NotFound));
    assert!(matches!(
        provider(&fake, CliPreset::claude(CLAUDE)).complete(ask("triage")).await,
        Err(LlmError::Unavailable(m)) if m.contains("program not found")
    ));
}

/// Two explicit zeros are a complete report, not a missing one.
#[tokio::test]
async fn cli_complete_usage_with_zeros_is_reported() {
    let response = with_usage(r#"{"input_tokens":0,"output_tokens":0}"#).await;
    assert_eq!(response.usage.source, UsageSource::Reported);
    assert_eq!(response.usage.input_tokens, 0);
    assert_eq!(response.usage.output_tokens, 0);
    assert_eq!(response.usage.cost_micros, Some(0));
}

/// Only the input total: keep it, estimate the answer.
#[tokio::test]
async fn cli_input_only_usage_estimates_the_output() {
    let response = with_usage(r#"{"input_tokens":100}"#).await;
    assert_eq!(response.usage.source, UsageSource::Estimated);
    assert_eq!(response.usage.input_tokens, 100);
    assert!(
        response.usage.output_tokens > 0,
        "the answer is estimated: {:?}",
        response.usage
    );
    assert_eq!(
        response.usage.cost_micros,
        Some(100 * 2 + u64::from(response.usage.output_tokens) * 10)
    );
}

/// Only the output total: keep it, estimate the prompt.
#[tokio::test]
async fn cli_output_only_usage_estimates_the_input() {
    let response = with_usage(r#"{"output_tokens":20}"#).await;
    assert_eq!(response.usage.source, UsageSource::Estimated);
    assert_eq!(response.usage.output_tokens, 20);
    assert!(
        response.usage.input_tokens > 0,
        "the prompt is estimated: {:?}",
        response.usage
    );
    assert_eq!(
        response.usage.cost_micros,
        Some(u64::from(response.usage.input_tokens) * 2 + 20 * 10)
    );
}

/// Empty, null, malformed or wrongly typed usage is not a reported zero: both totals come
/// from the prompt and the answer.
#[tokio::test]
async fn cli_unusable_usage_is_estimated_not_reported_zero() {
    for usage in [r#"{}"#, "null", r#""oops""#, r#"{"input_tokens":"many"}"#] {
        let response = with_usage(usage).await;
        assert_eq!(
            response.usage.source,
            UsageSource::Estimated,
            "usage {usage}"
        );
        assert!(response.usage.input_tokens > 0, "usage {usage}");
        assert!(response.usage.output_tokens > 0, "usage {usage}");
        assert!(
            response.usage.cost_micros.is_some_and(|cost| cost > 0),
            "usage {usage}: {:?}",
            response.usage
        );
    }
}

/// Plain-text CLI output has no totals at all, so both are estimated and priced.
#[tokio::test]
async fn cli_text_output_is_estimated_and_priced() {
    let custom = CliPreset {
        program: "/opt/llm/run".to_owned(),
        args: vec!["--ask".to_owned(), "{prompt}".to_owned()],
        prompt_via: PromptVia::Arg,
        output: CliOutput::Text,
        env: Default::default(),
    };
    let fake = Arc::new(FakeProcessExecutor::new());
    let prompt = "You triage incidents.\n\nUser:\ntriage";
    fake.expect("/opt/llm/run", &["--ask", prompt], ok("All good.\n"));
    let response = priced_provider(&fake, custom)
        .complete(ask("triage"))
        .await
        .unwrap();
    assert_eq!(response.usage.source, UsageSource::Estimated);
    assert!(response.usage.input_tokens > 0);
    assert!(response.usage.output_tokens > 0);
    assert!(response.usage.cost_micros.is_some_and(|cost| cost > 0));
}

/// One priced CLI turn whose stdout is exactly `stdout`.
async fn run_stdout(stdout: &str) -> domovoi_core::ports::LlmResponse {
    let fake = Arc::new(FakeProcessExecutor::new());
    fake.expect(CLAUDE, &claude_args(), ok(stdout));
    priced_provider(&fake, CliPreset::claude(CLAUDE))
        .complete(ask("triage"))
        .await
        .unwrap()
}

/// An empty plain-text answer has no output to count, so it charges the authorised
/// allowance instead of zero.
#[tokio::test]
async fn cli_empty_text_answer_charges_the_allowance() {
    let custom = CliPreset {
        program: "/opt/llm/run".to_owned(),
        args: vec!["--ask".to_owned(), "{prompt}".to_owned()],
        prompt_via: PromptVia::Arg,
        output: CliOutput::Text,
        env: Default::default(),
    };
    let fake = Arc::new(FakeProcessExecutor::new());
    let prompt = "You triage incidents.\n\nUser:\ntriage";
    fake.expect("/opt/llm/run", &["--ask", prompt], ok("\n"));
    let response = priced_provider(&fake, custom)
        .complete(ask("triage"))
        .await
        .unwrap();
    assert_eq!(response.usage.source, UsageSource::Unknown);
    assert!(response.usage.input_tokens > 0);
    assert_eq!(
        response.usage.output_tokens, 512,
        "the request's allowance is charged, not zero"
    );
    assert_eq!(
        response.usage.cost_micros,
        Some(u64::from(response.usage.input_tokens) * 2 + 512 * 10)
    );
}

/// The same holds for an empty JSON result with no usage metadata.
#[tokio::test]
async fn cli_empty_json_answer_charges_the_allowance() {
    let response = run_stdout(r#"{"type":"result","result":""}"#).await;
    assert_eq!(response.usage.source, UsageSource::Unknown);
    assert!(response.usage.input_tokens > 0);
    assert_eq!(response.usage.output_tokens, 512);
    assert_eq!(
        response.usage.cost_micros,
        Some(u64::from(response.usage.input_tokens) * 2 + 512 * 10)
    );
}

/// Input-only usage on an empty answer keeps the reported input and still charges the
/// allowance for the unknown output.
#[tokio::test]
async fn cli_empty_json_with_input_usage_keeps_input() {
    let response =
        run_stdout(r#"{"type":"result","result":"","usage":{"input_tokens":100}}"#).await;
    assert_eq!(response.usage.source, UsageSource::Unknown);
    assert_eq!(response.usage.input_tokens, 100);
    assert_eq!(response.usage.output_tokens, 512);
    assert_eq!(response.usage.cost_micros, Some(100 * 2 + 512 * 10));
}

/// A reported output of zero is authoritative, even when the answer is empty.
#[tokio::test]
async fn cli_explicit_zero_output_is_authoritative() {
    let response = run_stdout(
        r#"{"type":"result","result":"","usage":{"input_tokens":100,"output_tokens":0}}"#,
    )
    .await;
    assert_eq!(response.usage.source, UsageSource::Reported);
    assert_eq!(response.usage.input_tokens, 100);
    assert_eq!(response.usage.output_tokens, 0, "a reported zero is kept");
    assert_eq!(response.usage.cost_micros, Some(100 * 2));
}

/// A non-empty answer with no usage is still estimated from the answer, not charged the
/// allowance.
#[tokio::test]
async fn cli_non_empty_answer_without_usage_is_estimated() {
    let response = run_stdout(r#"{"type":"result","result":"All good."}"#).await;
    assert_eq!(response.usage.source, UsageSource::Estimated);
    assert!(response.usage.input_tokens > 0);
    assert!(
        response.usage.output_tokens > 0 && response.usage.output_tokens < 512,
        "the visible answer is estimated, not the allowance: {:?}",
        response.usage
    );
    assert!(response.usage.cost_micros.is_some_and(|cost| cost > 0));
}
