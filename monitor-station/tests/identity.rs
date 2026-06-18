//! Phase 1 gate: the station's read-only object-capability key.
//!
//! These tests only compile/run under `--features newt`:
//!   cargo test -p monitor-station --features newt
#![cfg(feature = "newt")]

use monitor_station::identity::{
    key_caveats, read_only_caveats, read_only_operating_key, AgentMetadata, Caveats,
};
use tempfile::TempDir;

#[test]
fn station_key_carries_exactly_read_only_authority() {
    let dir = TempDir::new().unwrap();
    let op = read_only_operating_key(&dir.path().join("identity.pem")).unwrap();
    assert_eq!(key_caveats(&op).unwrap(), read_only_caveats());
}

#[test]
fn station_key_cert_verifies_end_to_end() {
    let dir = TempDir::new().unwrap();
    let op = read_only_operating_key(&dir.path().join("identity.pem")).unwrap();
    op.cert()
        .verify()
        .expect("read-only operating key cert verifies");
}

/// The headline object-capability property: a read-only operating key cannot
/// delegate wider authority than it holds. Minting a child with `⊤` (full)
/// caveats must be refused as `CaveatAmplification`.
#[test]
fn station_key_cannot_widen() {
    let dir = TempDir::new().unwrap();
    let op = read_only_operating_key(&dir.path().join("identity.pem")).unwrap();
    let amplifying = AgentMetadata {
        role: "evil-child".to_string(),
        host: "caster".to_string(),
        capabilities: Vec::new(),
        issued_at: "2026-01-01T00:00:00Z".to_string(),
        expires_at: None,
        caveats: Caveats::top(),
    };
    assert!(
        op.delegate(amplifying).is_err(),
        "a read-only key must not be able to delegate full authority"
    );
}

/// The per-user root key is generated once and reused, so the station's
/// identity is stable across restarts.
#[test]
fn user_key_is_reused_across_mints() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("identity.pem");
    let first = read_only_operating_key(&path).unwrap();
    assert!(path.exists(), "first mint creates the user key");
    let second = read_only_operating_key(&path).unwrap();
    assert_eq!(
        first.cert().user_fingerprint(),
        second.cert().user_fingerprint(),
        "second mint must reuse the persisted user key, not regenerate it"
    );
}
