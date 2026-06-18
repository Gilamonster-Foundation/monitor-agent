//! caster station — Phase 0/1 scaffold.
//!
//! The caster station is the inherit-and-extend descendant of `newt-agent`
//! that runs the lab-wide monitoring view on the Windows "caster" box. This
//! crate is the integration point where the station is wired together; the
//! shared monitoring engine continues to live in `monitor-core` /
//! `monitor-collect` / `monitor-alert`.
//!
//! * **Phase 0** — cargo feature gates [`newt`], `mesh`, `shell`, all OFF by
//!   default, so `cargo build --workspace` stays green and ecosystem-clean.
//! * **Phase 1** — a read-only object-capability operating key inherited from
//!   the Gilamonster capability lattice (`--features newt`). See [`identity`].
//!
//! [`newt`]: identity

pub mod identity;

/// The cargo features compiled into this build (Phase 0 visibility).
///
/// Returns the names of the optional station capabilities enabled at build
/// time, in a stable order. Empty for a default build.
#[must_use]
pub fn enabled_features() -> Vec<&'static str> {
    vec![
        #[cfg(feature = "newt")]
        "newt",
        #[cfg(feature = "mesh")]
        "mesh",
        #[cfg(feature = "shell")]
        "shell",
    ]
}

/// Print station status.
///
/// Under `--features newt`, mint and report the station's read-only operating
/// key; otherwise note that the object-capability layer is disabled. A default
/// (no-feature) build is side-effect free — it never touches the filesystem.
///
/// # Errors
///
/// Returns an error if (under `newt`) the per-user key cannot be loaded or
/// generated, or the read-only attenuation is rejected.
pub fn run() -> anyhow::Result<()> {
    println!("caster station — scaffold (Phase 0/1)");
    let feats = enabled_features();
    let summary = if feats.is_empty() {
        "<none>".to_string()
    } else {
        feats.join(", ")
    };
    println!("features: {summary}");

    #[cfg(feature = "newt")]
    {
        let key_path = identity::station_key_path()?;
        let op = identity::read_only_operating_key(&key_path)?;
        let caveats = identity::key_caveats(&op)?;
        println!("read-only operating key minted; enforced caveats: {caveats:?}");
    }
    #[cfg(not(feature = "newt"))]
    println!(
        "read-only identity disabled — build with `--features newt` to inherit the ocap layer"
    );

    Ok(())
}
