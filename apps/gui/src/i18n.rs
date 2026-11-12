//! Existing message catalogs are shared by the desktop app and macOS Installer.
use std::{collections::BTreeMap, sync::OnceLock};

pub const LANGUAGES: [(&str, &str); 7] = [
    ("en", "English"),
    ("ru", "Русский"),
    ("es", "Español"),
    ("de", "Deutsch"),
    ("fr", "Français"),
    ("ja", "日本語"),
    ("pt-BR", "Português (Brasil)"),
];

pub fn catalogs() -> &'static BTreeMap<&'static str, BTreeMap<String, String>> {
    static CATALOGS: OnceLock<BTreeMap<&str, BTreeMap<String, String>>> = OnceLock::new();
    CATALOGS.get_or_init(|| {
        [
            ("en", include_str!("../locales/en.json")),
            ("ru", include_str!("../locales/ru.json")),
            ("es", include_str!("../locales/es.json")),
            ("de", include_str!("../locales/de.json")),
            ("fr", include_str!("../locales/fr.json")),
            ("ja", include_str!("../locales/ja.json")),
            ("pt-BR", include_str!("../locales/pt-BR.json")),
        ]
        .into_iter()
        .map(|(locale, source)| {
            (
                locale,
                serde_json::from_str(source).expect("bundled translation catalog"),
            )
        })
        .collect()
    })
}

pub fn valid_choice(choice: &str) -> bool {
    choice == "system" || LANGUAGES.iter().any(|(code, _)| *code == choice)
}

pub fn resolve<'a>(choice: &'a str, preferred: &[impl AsRef<str>]) -> &'a str {
    if choice != "system" && valid_choice(choice) {
        return choice;
    }
    for tag in preferred {
        let tag = tag
            .as_ref()
            .split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_lowercase();
        if tag == "pt" {
            return "pt-BR";
        }
        if let Some((code, _)) = LANGUAGES.iter().find(|(code, _)| *code == tag) {
            return code;
        }
    }
    "en"
}

pub fn text(locale: &str, key: &str) -> String {
    catalogs()
        .get(locale)
        .and_then(|catalog| catalog.get(key))
        .or_else(|| catalogs()["en"].get(key))
        .cloned()
        .unwrap_or_else(|| key.into())
}

pub fn reason(locale: &str, original: &str) -> String {
    let key = match original {
        "Intel SMC BCLM firmware threshold, 50..100%; device write compatibility is unverified" => {
            "reasonIntelBclm"
        }
        "no compatible charge-control interface detected" => "reasonNone",
        "SMC binary gate; daemon enforces hysteresis while awake; firmware may deny writes" => {
            "reasonSmc"
        }
        "daemon temporarily releases gate until full" => "reasonSmcTopUp",
        "SMC CH0I; daemon restores adapter at target"
        | "SMC adapter gate; daemon restores adapter at target" => "reasonSmcDischarge",
        "kernel end threshold; writes require device permissions" | "kernel end threshold" => {
            "reasonKernelEnd"
        }
        "kernel start threshold" => "reasonKernelStart",
        "temporary 100% threshold, restored by daemon" => "reasonKernelTopUp",
        "not exposed by generic threshold backend" => "reasonGeneric",
        "Lenovo WMI conservation mode, fixed 80%; device write compatibility is unverified" => {
            "reasonLenovo"
        }
        "scoped system-sleep assertion while charging or discharging on AC; holds closed-lid sleep until the limit" => {
            "preventSleepHelp"
        }
        "battery temperature sensor and SMC charging gate" => "heatHelp",
        "MagSafe ACLC LED override; requires compatible connected adapter" => "ledMode",
        "not exposed by Intel BCLM" | "not exposed by this backend" => "reasonGeneric",
        _ => return original.into(),
    };
    text(locale, key)
}

pub fn battery_state(locale: &str, original: &str) -> String {
    text(
        locale,
        match original.to_lowercase().as_str() {
            "charging" => "charging",
            "discharging" => "discharging",
            "full" => "full",
            "not charging" | "not-charging" => "notCharging",
            "low" => "low",
            "critical" => "critical",
            _ => "unknown",
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tokens(value: &str) -> Vec<&str> {
        value
            .split('{')
            .skip(1)
            .filter_map(|s| s.split_once('}').map(|(key, _)| key))
            .collect()
    }
    #[test]
    fn all_catalogs_have_complete_nonempty_messages_and_interpolation() {
        let reference = &catalogs()["en"];
        for catalog in catalogs().values() {
            assert_eq!(
                catalog.keys().collect::<Vec<_>>(),
                reference.keys().collect::<Vec<_>>()
            );
            for (key, value) in catalog {
                assert!(!value.trim().is_empty(), "{key}");
                assert_eq!(tokens(value), tokens(&reference[key]), "{key}");
            }
        }
    }
    #[test]
    fn ordered_system_languages_and_explicit_choice_are_respected() {
        for (input, expected) in [
            ("ru-RU", "ru"),
            ("es-MX", "es"),
            ("de-AT", "de"),
            ("fr-CA", "fr"),
            ("ja-JP", "ja"),
            ("pt-PT", "pt-BR"),
            ("en-GB", "en"),
            ("zh-CN", "en"),
        ] {
            assert_eq!(resolve("system", &[input]), expected);
        }
        assert_eq!(resolve("system", &["zh-CN", "fr-FR", "de-DE"]), "fr");
        assert_eq!(resolve("de", &["ru-RU"]), "de");
        assert_eq!(resolve("invalid", &["ja"]), "ja");
        assert!(!valid_choice("../en"));
        assert_eq!(reason("ja", "permission denied"), "permission denied");
        assert_eq!(
            battery_state("ru", "Not-charging"),
            text("ru", "notCharging")
        );
    }
}
