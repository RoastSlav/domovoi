use domovoi_proto::{PROTOCOL_VERSION, ProtocolVersion, compatible};

fn v(major: u16, minor: u16) -> ProtocolVersion {
    ProtocolVersion { major, minor }
}

#[test]
fn compatibility_matrix() {
    // (agent, hub, accepted)
    let cases = [
        (v(1, 0), v(1, 0), true),
        (v(1, 3), v(1, 4), true),
        (v(1, 2), v(1, 4), false),
        (v(1, 5), v(1, 4), false),
        (v(2, 0), v(1, 0), false),
        (v(1, 0), v(2, 0), false),
        (v(1, u16::MAX), v(1, u16::MAX), true),
    ];
    for (agent, hub, accepted) in cases {
        assert_eq!(compatible(agent, hub), accepted, "agent {agent}, hub {hub}");
    }
}

#[test]
fn current_version_is_one_zero() {
    assert_eq!(PROTOCOL_VERSION.to_string(), "1.0");
}
