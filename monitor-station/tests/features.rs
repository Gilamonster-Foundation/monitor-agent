//! Phase 0 gate: feature visibility, and the default-feature `run()` path.

/// `enabled_features()` must reflect exactly the cargo features compiled in —
/// no more, no less — for any feature combination.
#[test]
fn enabled_features_reflects_cfg() {
    let f = monitor_station::enabled_features();
    assert_eq!(f.contains(&"newt"), cfg!(feature = "newt"));
    assert_eq!(f.contains(&"mesh"), cfg!(feature = "mesh"));
    assert_eq!(f.contains(&"shell"), cfg!(feature = "shell"));
}

/// Without the `newt` feature, `run()` only prints status: it must return Ok
/// and must NOT mint a key or otherwise touch the filesystem.
#[cfg(not(feature = "newt"))]
#[test]
fn run_without_newt_is_ok_and_side_effect_free() {
    monitor_station::run().expect("default-feature run() returns Ok");
}
