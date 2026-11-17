//! Tests for [`FakeAgentLink`]: presence first, then the scripted handler.

use super::*;

#[tokio::test]
async fn fake_agent_link_checks_presence_then_handlers() {
    let link = FakeAgentLink::new();
    let host = HostId::new();
    let at = Timestamp::constant(1_700_000_000, 0);
    let call = ReadToolCall::HostOverview;
    assert_eq!(
        link.run_tool(host, call.clone(), at).await,
        Err(LinkError::NotConnected)
    );

    link.set_presence(
        host,
        Presence::Connected {
            since: at,
            version: "0.1.0".to_owned(),
        },
    );
    assert!(matches!(
        link.run_tool(host, call.clone(), at).await,
        Err(LinkError::Rejected(_))
    ));

    link.on_tool(|_, _| {
        Ok(ToolOutput {
            text: "up 3 days".to_owned(),
            truncated: false,
        })
    });
    let out = link.run_tool(host, call, at).await.unwrap();
    assert_eq!(out.text, "up 3 days");
    assert_eq!(link.requests().len(), 3);
    assert!(link.skills(host).is_empty());
}
