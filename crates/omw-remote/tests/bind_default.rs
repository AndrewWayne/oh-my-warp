//! Contract test for the daemon default-listen address (threat-model I-15,
//! PRD §4.2): loopback by default, other-interface exposure only on an
//! explicit operator override.

use omw_remote::{resolve_bind_addr, DEFAULT_LISTEN_ADDR};

#[test]
fn default_listen_is_loopback() {
    let addr = resolve_bind_addr(None).expect("default parses");
    assert!(
        addr.ip().is_loopback(),
        "default bind must be loopback (I-15); got {addr}"
    );
    assert_eq!(addr.port(), 8787);
    // The advertised constant must itself be loopback.
    let konst: std::net::SocketAddr = DEFAULT_LISTEN_ADDR.parse().expect("const parses");
    assert!(konst.ip().is_loopback());
}

#[test]
fn empty_or_whitespace_override_falls_back_to_loopback() {
    assert!(resolve_bind_addr(Some("")).unwrap().ip().is_loopback());
    assert!(resolve_bind_addr(Some("   ")).unwrap().ip().is_loopback());
}

#[test]
fn explicit_override_is_honored() {
    // The opt-in path: an operator can expose on all interfaces or a
    // specific tailnet IP.
    let all = resolve_bind_addr(Some("0.0.0.0:8787")).expect("parses");
    assert!(all.ip().is_unspecified(), "0.0.0.0 opt-in honored");
    assert_eq!(all.port(), 8787);

    let tailnet = resolve_bind_addr(Some("100.64.0.5:9000")).expect("parses");
    assert_eq!(tailnet.to_string(), "100.64.0.5:9000");
}

#[test]
fn invalid_override_errors() {
    assert!(resolve_bind_addr(Some("not-an-addr")).is_err());
    assert!(resolve_bind_addr(Some("127.0.0.1")).is_err()); // missing port
}
