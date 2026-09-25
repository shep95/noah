//! The language noah speaks: its interface text and the language shepherd
//! answers in. People pick it with the `language` setting, which lists every
//! language in [`LANGUAGES`]; "system" follows the operating system.

mod catalog;
mod languages;

use gpui::App;
use settings::{RegisterSetting, Settings};

pub use languages::{LANGUAGES, Language};

/// The `language` setting value that follows the operating system.
pub const SYSTEM: &str = "system";

const ENGLISH: &Language = &Language {
    code: "en",
    english_name: "English",
    native_name: "English",
};

#[derive(Clone, Debug, RegisterSetting)]
pub struct LanguageSettings {
    pub language: &'static Language,
}

impl Settings for LanguageSettings {
    fn from_settings(content: &settings::SettingsContent) -> Self {
        let chosen = content
            .language
            .as_ref()
            .map(|code| code.0.as_ref())
            .unwrap_or(SYSTEM);
        let language = if chosen.eq_ignore_ascii_case(SYSTEM) {
            sys_locale::get_locale()
                .and_then(|locale| find_language(&locale))
                .unwrap_or(ENGLISH)
        } else {
            find_language(chosen).unwrap_or(ENGLISH)
        };
        Self { language }
    }
}

/// The language noah is currently shown in.
pub fn current_language(cx: &App) -> &'static Language {
    LanguageSettings::get_global(cx).language
}

/// Finds a language by BCP 47 code, tolerating the forms operating systems
/// report (`pt_BR`, `zh-Hans-CN`, `es-MX.UTF-8`) by falling back to the base
/// language when the exact variant isn't listed.
pub fn find_language(code: &str) -> Option<&'static Language> {
    let code = code
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .replace('_', "-");
    if code.is_empty() {
        return None;
    }
    let exact = LANGUAGES
        .iter()
        .find(|language| language.code.eq_ignore_ascii_case(&code));
    if exact.is_some() {
        return exact;
    }
    let lowercase = code.to_lowercase();
    let traditional_chinese = ["zh-hant", "zh-tw", "zh-hk", "zh-mo"];
    if traditional_chinese
        .iter()
        .any(|prefix| lowercase.starts_with(prefix))
    {
        return find_language("zh-Hant");
    }
    let latin_american_spanish = lowercase.starts_with("es-")
        && !lowercase.starts_with("es-es")
        && lowercase.len() > 3;
    if latin_american_spanish {
        return find_language("es-419");
    }
    let base = lowercase.split('-').next().unwrap_or_default();
    LANGUAGES
        .iter()
        .find(|language| language.code.eq_ignore_ascii_case(base))
}

/// noah's interface text for `english` in the current language. Text without
/// a translation yet is shown in English rather than left blank.
pub fn t<'a>(cx: &App, english: &'a str) -> &'a str {
    translate(current_language(cx).code, english)
}

pub fn translate<'a>(code: &str, english: &'a str) -> &'a str {
    let Some(index) = catalog::KEYS.iter().position(|key| *key == english) else {
        return english;
    };
    translation_table(code)
        .and_then(|table| table.get(index).copied())
        .unwrap_or(english)
}

fn translation_table(code: &str) -> Option<&'static [&'static str; catalog::KEYS.len()]> {
    let find = |wanted: &str| {
        catalog::TRANSLATIONS
            .iter()
            .find(|(table_code, _)| table_code.eq_ignore_ascii_case(wanted))
            .map(|(_, table)| table)
    };
    let base = code.split('-').next().unwrap_or(code);
    // Norwegian ("no") is almost always written as Bokmål.
    let alias = match base {
        "no" | "nn" => Some("nb"),
        "yue" => Some("zh-Hant"),
        _ => None,
    };
    find(code)
        .or_else(|| find(base))
        .or_else(|| alias.and_then(find))
}

/// Whether text in this language runs right to left.
pub fn is_right_to_left(language: &Language) -> bool {
    matches!(
        language.code,
        "ar" | "he" | "fa" | "ur" | "ps" | "sd" | "ug" | "yi" | "dv" | "ks"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_translation_is_complete() {
        for (code, table) in catalog::TRANSLATIONS {
            assert!(find_language(code).is_some(), "{code} isn't a listed language");
            for (index, text) in table.iter().enumerate() {
                assert!(
                    !text.trim().is_empty(),
                    "{code} is missing {:?}",
                    catalog::KEYS[index]
                );
            }
        }
    }

    #[test]
    fn languages_are_listed_once() {
        let mut seen = HashSet::new();
        for language in LANGUAGES {
            assert!(seen.insert(language.code), "{} is listed twice", language.code);
            assert!(!language.native_name.is_empty());
        }
        assert!(LANGUAGES.len() > 180);
    }

    #[test]
    fn operating_system_locales_resolve() {
        let code = |locale| find_language(locale).map(|language| language.code);
        assert_eq!(code("pt_BR.UTF-8"), Some("pt-BR"));
        assert_eq!(code("pt-PT"), Some("pt"));
        assert_eq!(code("zh-Hans-CN"), Some("zh"));
        assert_eq!(code("zh-TW"), Some("zh-Hant"));
        assert_eq!(code("es-MX"), Some("es-419"));
        assert_eq!(code("es-ES"), Some("es"));
        assert_eq!(code("fr-CA"), Some("fr"));
        assert_eq!(code("C"), None);
    }

    #[test]
    fn lookups_fall_back_sensibly() {
        assert_eq!(translate("es", "browser"), "navegador");
        assert_eq!(translate("es-419", "browser"), "navegador");
        assert_eq!(translate("no", "settings"), translate("nb", "settings"));
        assert_eq!(translate("cy", "browser"), "browser", "untranslated → English");
        assert_eq!(translate("es", "not a key"), "not a key");
    }
}
