use monitor_core::alert::{Alert, AlertDispatcher, Severity};

/// Speaks alert messages using the platform's best available TTS engine.
///
/// Detection order (runtime, not compile-time):
/// - Linux: piper → espeak-ng → spd-say
/// - macOS: say
/// - Windows: PowerShell SpeechSynthesizer
///
/// Set `engine = "auto"` in config to use detection, or pin to a specific engine.
pub struct VoiceDispatcher {
    engine: VoiceEngine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceEngine {
    Auto,
    Piper,
    EspeakNg,
    SpdSay,
    Say,
    PowerShell,
    Disabled,
}

impl VoiceEngine {
    pub fn from_config_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "piper" => Self::Piper,
            "espeak-ng" | "espeak" => Self::EspeakNg,
            "spd-say" => Self::SpdSay,
            "say" => Self::Say,
            "powershell" => Self::PowerShell,
            "disabled" | "off" | "false" => Self::Disabled,
            _ => Self::Auto,
        }
    }

    /// Detect the best available engine on this platform.
    pub fn detect() -> Self {
        #[cfg(target_os = "macos")]
        {
            if which("say") {
                Self::Say
            } else {
                Self::Disabled
            }
        }

        #[cfg(target_os = "windows")]
        {
            Self::PowerShell
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            if which("piper") {
                return Self::Piper;
            }
            if which("espeak-ng") {
                return Self::EspeakNg;
            }
            if which("spd-say") {
                return Self::SpdSay;
            }
            Self::Disabled
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn which(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// PowerShell that speaks via System.Speech (SAPI). The phrase is handed to it
/// through the `MONITOR_VOICE_TEXT` env var rather than interpolated into the
/// script, so untrusted alert text — an apostrophe, or a crafted payload —
/// cannot break or inject into the command.
const PS_VOICE_SCRIPT: &str = "Add-Type -AssemblyName System.Speech; \
     $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
     $s.Speak($env:MONITOR_VOICE_TEXT)";

/// Env var carrying the phrase for [`PS_VOICE_SCRIPT`].
const PS_VOICE_TEXT_ENV: &str = "MONITOR_VOICE_TEXT";

impl VoiceDispatcher {
    pub fn new(engine_str: &str) -> Self {
        let engine = if engine_str == "auto" || engine_str.is_empty() {
            VoiceEngine::detect()
        } else {
            VoiceEngine::from_config_str(engine_str)
        };
        tracing::debug!("voice dispatcher using engine: {engine:?}");
        Self { engine }
    }

    fn phrase_for(&self, alert: &Alert) -> String {
        match alert.severity {
            Severity::Critical => format!("WARNING. {}", alert.message),
            Severity::Warn => format!("Heads up. {}", alert.message),
            Severity::Info => alert.message.clone(),
        }
    }

    async fn speak(&self, text: &str) -> anyhow::Result<()> {
        let text = text.to_owned();
        match &self.engine {
            VoiceEngine::Disabled | VoiceEngine::Auto => {}
            VoiceEngine::Say => {
                tokio::process::Command::new("say")
                    .arg(&text)
                    .status()
                    .await?;
            }
            VoiceEngine::EspeakNg => {
                tokio::process::Command::new("espeak-ng")
                    .args(["-s", "140", &text])
                    .status()
                    .await?;
            }
            VoiceEngine::SpdSay => {
                tokio::process::Command::new("spd-say")
                    .arg(&text)
                    .status()
                    .await?;
            }
            VoiceEngine::Piper => {
                // piper reads text from stdin, writes wav to stdout; pipe through aplay.
                use tokio::io::AsyncWriteExt;
                let mut child = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg("piper --output_raw | aplay -r 22050 -f S16_LE -c 1")
                    .stdin(std::process::Stdio::piped())
                    .spawn()?;
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(text.as_bytes()).await?;
                }
                child.wait().await?;
            }
            VoiceEngine::PowerShell => {
                tokio::process::Command::new("powershell")
                    .args(["-NoProfile", "-Command", PS_VOICE_SCRIPT])
                    .env(PS_VOICE_TEXT_ENV, &text)
                    .status()
                    .await?;
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl AlertDispatcher for VoiceDispatcher {
    fn name(&self) -> &str {
        "voice"
    }

    async fn fire(&self, alert: &Alert) -> anyhow::Result<()> {
        let phrase = self.phrase_for(alert);
        self.speak(&phrase).await
    }

    async fn resolve(&self, _alert: &Alert) -> anyhow::Result<()> {
        // Resolved alerts do not get a spoken notification by default —
        // unnecessary noise for a system returning to normal.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_from_str_roundtrip() {
        assert_eq!(VoiceEngine::from_config_str("piper"), VoiceEngine::Piper);
        assert_eq!(
            VoiceEngine::from_config_str("espeak-ng"),
            VoiceEngine::EspeakNg
        );
        assert_eq!(VoiceEngine::from_config_str("say"), VoiceEngine::Say);
        assert_eq!(
            VoiceEngine::from_config_str("disabled"),
            VoiceEngine::Disabled
        );
        assert_eq!(VoiceEngine::from_config_str("auto"), VoiceEngine::Auto);
    }

    #[tokio::test]
    async fn voice_dispatcher_disabled_is_no_op() {
        use monitor_core::alert::{Alert, AlertId, AlertState, Severity};
        use monitor_core::metrics::MetricPath;
        use uuid::Uuid;
        let d = VoiceDispatcher {
            engine: VoiceEngine::Disabled,
        };
        let alert = Alert {
            id: AlertId::for_rule("gnuc", "test"),
            uuid: Uuid::new_v4(),
            rule_name: "test".into(),
            target: "gnuc".into(),
            metric: MetricPath::new("cpu.percent"),
            value: 90.0,
            severity: Severity::Critical,
            state: AlertState::Firing,
            message: "test message".into(),
            fired_at_secs: None,
            resolved_at_secs: None,
            cooldown_until_secs: None,
            fired_instant: None,
            cooldown_until_instant: None,
        };
        // Should not fail or hang.
        d.fire(&alert).await.unwrap();
        d.resolve(&alert).await.unwrap();
    }

    #[test]
    fn powershell_voice_passes_text_via_env_not_interpolation() {
        // Regression: the alert phrase must reach SAPI through an env var, never
        // string-interpolated into the script. Otherwise an apostrophe in a
        // message breaks the command, and a crafted message could inject
        // arbitrary PowerShell.
        assert!(
            PS_VOICE_SCRIPT.contains(&format!("$env:{PS_VOICE_TEXT_ENV}")),
            "script must read the phrase from the env var"
        );
        assert!(
            !PS_VOICE_SCRIPT.contains("Speak('"),
            "script must not interpolate the phrase into a quoted literal"
        );
    }

    /// Speaks aloud via SAPI, so it is ignored in the normal gate — run with
    /// `cargo test -p monitor-alert -- --ignored` on a Windows host with audio.
    /// Proves an apostrophe in the message no longer breaks the command.
    #[cfg(target_os = "windows")]
    #[tokio::test]
    #[ignore = "speaks aloud via SAPI; run explicitly with --ignored"]
    async fn powershell_fire_handles_apostrophe() {
        use monitor_core::alert::{Alert, AlertId, AlertState, Severity};
        use monitor_core::metrics::MetricPath;
        use uuid::Uuid;
        let d = VoiceDispatcher::new("powershell");
        let alert = Alert {
            id: AlertId::for_rule("nuc01", "quote-test"),
            uuid: Uuid::new_v4(),
            rule_name: "quote-test".into(),
            target: "nuc01".into(),
            metric: MetricPath::new("disk.used_pct"),
            value: 95.0,
            severity: Severity::Warn,
            state: AlertState::Firing,
            message: "nuc01 disk's almost full".into(),
            fired_at_secs: None,
            resolved_at_secs: None,
            cooldown_until_secs: None,
            fired_instant: None,
            cooldown_until_instant: None,
        };
        d.fire(&alert).await.unwrap();
    }
}
