use domovoi_proto::{CHALLENGE_CONTEXT, Nonce, challenge_message};

use crate::samples::agent_id;

#[test]
fn challenge_message_layout_is_stable() {
    let nonce = Nonce(core::array::from_fn(|i| i as u8));
    let bytes = challenge_message(&nonce, agent_id(), "tls:abc");

    let mut expected = b"domovoi-agent-auth-v1\0".to_vec();
    expected.extend(0u8..32);
    expected.extend([
        0x01, 0x90, 0x00, 0x00, 0x00, 0x00, 0x70, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0xa1,
    ]);
    expected.extend(b"tls:abc");

    assert_eq!(CHALLENGE_CONTEXT, b"domovoi-agent-auth-v1\0");
    assert_eq!(bytes, expected);
    assert_eq!(bytes.len(), 22 + 32 + 16 + 7);
}
