use std::sync::Arc;

use domovoi_core::ports::{
    LlmError, LlmMessage, Part, ProcessError, ProcessExecutor, ProcessOutput, Role, StopReason,
};
use domovoi_llm::{
    CliOutput, CliPreset, EXCERPT_CHARS, LlmBuildError, PromptVia, ProviderKind, ProviderSettings,
    build,
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
