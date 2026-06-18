//! Read-only object-capability identity for the caster station.
//!
//! The station must be *structurally incapable* of mutating the farm it
//! watches. We inherit the Gilamonster capability lattice
//! ([`agent_mesh_protocol`]) — the same signed, attenuation-only machinery
//! `newt-identity` wraps — and mint a key as follows:
//!
//! ```text
//!   UserKey (root of trust, ~/.caster/identity.pem)
//!     └── AgentKey::issue(⊤)                  = session root (full authority)
//!           └── session.delegate(ReadOnly)    = operating key (read-only)
//! ```
//!
//! The operating key's caveats are *signed into its cert* and provably `⊑` the
//! root. [`AgentKey::delegate`] refuses to mint a child that amplifies, and
//! [`agent_mesh_protocol::CertChain::verify`] re-checks attenuation at every
//! link — so a compromised holder of the operating key (but not the root
//! `UserKey`) can only ever *narrow* its authority, never widen it.
//!
//! This module is empty unless the `newt` feature is enabled.

#[cfg(feature = "newt")]
pub use agent_mesh_protocol::{AgentKey, AgentMetadata, Caveats, CountBound, MeshError, Scope};

#[cfg(feature = "newt")]
mod imp {
    use std::path::{Path, PathBuf};

    use agent_mesh_protocol::{AgentKey, AgentMetadata, Caveats, Scope, UserKey};
    use anyhow::{anyhow, Result};

    /// The station's read-only authority: it may **read** the systems it
    /// watches, but holds no write / exec / network authority. Tools run under
    /// this leash (e.g. an embedded brush shell, Phase 5) are therefore
    /// deny-by-default on every mutating axis.
    #[must_use]
    pub fn read_only_caveats() -> Caveats {
        Caveats {
            fs_write: Scope::none(),
            exec: Scope::none(),
            net: Scope::none(),
            // fs_read / max_calls / valid_for_generation stay at ⊤ (read-only,
            // not no-op): the station can still observe the farm and its config.
            ..Caveats::top()
        }
    }

    /// Mint the station's read-only operating key, rooted at the per-user key
    /// at `key_path` (generated on first run, reused thereafter).
    ///
    /// The returned [`AgentKey`] carries exactly [`read_only_caveats`], signed
    /// and provably `⊑` the full-authority session root.
    ///
    /// # Errors
    ///
    /// Returns an error if the user key cannot be loaded/generated, or — which
    /// would be a programming error here — if the read-only attenuation is
    /// somehow not `⊑` the `⊤` session root.
    pub fn read_only_operating_key(key_path: &Path) -> Result<AgentKey> {
        let user = load_or_generate(key_path)?;
        let session_root =
            AgentKey::issue(&user, station_meta("caster-session-root", Caveats::top()));
        session_root
            .delegate(station_meta("caster-readonly", read_only_caveats()))
            .map_err(|e| anyhow!("read-only attenuation rejected: {e}"))
    }

    /// Verify `key`'s cert chain and return the enforcement-side [`Caveats`] it
    /// carries.
    ///
    /// # Errors
    ///
    /// Returns an error if the cert chain fails verification (bad signature or
    /// an amplifying link).
    pub fn key_caveats(key: &AgentKey) -> Result<Caveats> {
        key.cert()
            .verify()
            .map_err(|e| anyhow!("operating-key cert failed verification: {e}"))?;
        Ok(key.cert().metadata.caveats.clone())
    }

    /// The station's per-user key path: `~/.caster/identity.pem`
    /// (`%USERPROFILE%\.caster\identity.pem` on Windows).
    ///
    /// # Errors
    ///
    /// Returns an error if neither `USERPROFILE` nor `HOME` is set.
    pub fn station_key_path() -> Result<PathBuf> {
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .ok_or_else(|| {
                anyhow!("no home directory (USERPROFILE/HOME) to locate the station key")
            })?;
        Ok(PathBuf::from(home).join(".caster").join("identity.pem"))
    }

    /// Load the per-user root key, generating and persisting a fresh one on
    /// first run. `UserKey::save` writes the key with restrictive permissions
    /// and creates the parent directory.
    fn load_or_generate(path: &Path) -> Result<UserKey> {
        if path.exists() {
            UserKey::load(path).map_err(|e| anyhow!("loading the station user key: {e}"))
        } else {
            let key = UserKey::generate();
            key.save(path)
                .map_err(|e| anyhow!("saving the station user key: {e}"))?;
            Ok(key)
        }
    }

    /// Signed metadata for a station key. `issued_at` is a *claim* in a signed
    /// cert (never a coordination primitive), so wall-clock is appropriate.
    fn station_meta(role: &str, caveats: Caveats) -> AgentMetadata {
        AgentMetadata {
            role: role.to_string(),
            host: "caster".to_string(),
            capabilities: Vec::new(),
            issued_at: chrono::Utc::now().to_rfc3339(),
            expires_at: None,
            caveats,
        }
    }
}

#[cfg(feature = "newt")]
pub use imp::{key_caveats, read_only_caveats, read_only_operating_key, station_key_path};
