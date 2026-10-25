//! Per-user GUI startup. The privileged charging service has its own lifecycle.
use fastframe_autostart::{Autostart, Start, State};
use std::collections::HashMap;

fn registration() -> Autostart {
    Autostart::new("batterybridge-gui", "BatteryBridge")
}

/// Called on a worker. Read OS state rather than storing another enabled flag.
pub fn query(enabled: Option<bool>) -> Result<State, String> {
    let startup = registration();
    match enabled {
        None => Ok(startup.state()),
        Some(enabled) => startup
            .set(enabled.then_some(Start::Hidden))
            .map_err(|error| error.to_string()),
    }
}

#[cfg(target_os = "macos")]
pub fn disable_for_uninstall() -> Result<(), String> {
    let startup = registration();
    if startup.state().start().is_some() {
        startup.set(None).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Cold tray startup precedes eframe's CreationContext. Use its existing file
/// and format, without moving preferences or maintaining a second language copy.
pub fn saved_language() -> Option<String> {
    let path = eframe::storage_dir(super::APP_ID)?.join("app.ron");
    if std::fs::metadata(&path).ok()?.len() > 1024 * 1024 {
        return None;
    }
    language_from_storage(&std::fs::read_to_string(path).ok()?)
}

fn language_from_storage(text: &str) -> Option<String> {
    let mut values: HashMap<String, String> = ron::from_str(text).ok()?;
    values
        .remove("language")
        .filter(|s| super::i18n::valid_choice(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_start_reads_the_existing_language_and_ignores_invalid_preferences() {
        assert_eq!(
            language_from_storage(r#"{"language": "ru", "other": "value",}"#),
            Some("ru".into())
        );
        assert_eq!(
            language_from_storage(r#"{"language": "system",}"#),
            Some("system".into())
        );
        for text in [
            "broken",
            "{}",
            r#"{"language": "xx"}"#,
            r#"{"language": 12}"#,
        ] {
            assert_eq!(language_from_storage(text), None);
        }
    }

    #[test]
    fn packaged_macos_agent_matches_the_registration_contract() {
        let actual =
            include_str!("../../../packaging/launchd/io.github.batterybridge.autostart.plist");
        let expected = fastframe_autostart::macos_agent_plist(
            super::super::APP_ID,
            fastframe_autostart::START_HIDDEN,
        );
        assert_eq!(
            actual.split_whitespace().collect::<Vec<_>>(),
            expected.split_whitespace().collect::<Vec<_>>()
        );
    }
}
