use domovoi_proto::{PROTOCOL_VERSION, ProtocolVersion, compatible};

fn v(major: u16, minor: u16) -> ProtocolVersion {
    ProtocolVersion { major, minor }
}

#[test]
fn exact_rule_matrix() {
    // (agent, hub, accepted): the same major and the same minor, nothing else.
    let cases = [
        (v(1, 1), v(1, 1), true),
        (v(1, 0), v(1, 1), false),
        (v(1, 2), v(1, 1), false),
        (v(2, 1), v(1, 1), false),
        (v(1, 1), v(2, 1), false),
        (v(1, u16::MAX), v(1, u16::MAX), true),
    ];
    for (agent, hub, accepted) in cases {
        assert_eq!(compatible(agent, hub), accepted, "agent {agent}, hub {hub}");
    }
}

#[test]
fn current_version_is_one_one() {
    assert_eq!(PROTOCOL_VERSION.to_string(), "1.1");
}
