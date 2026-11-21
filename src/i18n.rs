//! Interface translations. The language follows the operating system
//! automatically and can be changed in Settings. Strings live in `i18n.tsv`.

use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En = 0,
    Ru,
    Uk,
    De,
    Es,
    Fr,
    Zh,
    Ja,
}

pub const ALL: [Lang; 8] = [Lang::En, Lang::Ru, Lang::Uk, Lang::De, Lang::Es, Lang::Fr, Lang::Zh, Lang::Ja];

impl Lang {
    pub fn code(self) -> &'static str {
        ["en", "ru", "uk", "de", "es", "fr", "zh", "ja"][self as usize]
    }
    /// The language's own name, shown in the language menu.
    pub fn native(self) -> &'static str {
        ["English", "Русский", "Українська", "Deutsch", "Español", "Français", "中文", "日本語"][self as usize]
    }
    pub fn from_code(code: &str) -> Option<Lang> {
        let c = code.to_ascii_lowercase();
        let base = c.split(|ch| ch == '-' || ch == '_' || ch == '.').next().unwrap_or("");
        ALL.into_iter().find(|l| l.code() == base)
    }
    #[allow(dead_code)]
    pub fn is_cjk(self) -> bool {
        matches!(self, Lang::Zh | Lang::Ja)
    }
}

/// The operating system's language (falls back to English).
pub fn system() -> Lang {
    sys_locale::get_locale()
        .or_else(|| std::env::var("LANG").ok())
        .and_then(|l| Lang::from_code(&l))
        .unwrap_or(Lang::En)
}

/// "auto"/"" → system language; otherwise the chosen one.
pub fn resolve(setting: &str) -> Lang {
    if setting.is_empty() || setting == "auto" {
        system()
    } else {
        Lang::from_code(setting).unwrap_or_else(system)
    }
}

const TSV: &str = include_str!("i18n.tsv");

fn table() -> &'static HashMap<&'static str, [&'static str; 8]> {
    static T: OnceLock<HashMap<&'static str, [&'static str; 8]>> = OnceLock::new();
    T.get_or_init(|| {
        let mut m = HashMap::new();
        for line in TSV.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let cols: Vec<&'static str> = line.split('\t').collect();
            if cols.len() == 9 {
                let mut row = [""; 8];
                row.copy_from_slice(&cols[1..9]);
                m.insert(cols[0], row);
            }
        }
        m
    })
}

/// Translate a key. Missing translation → English → the key itself.
/// Like [`tr`] for keys built at run time; unknown keys give "".
pub fn tr_dyn(lang: Lang, key: &str) -> &'static str {
    match table().get(key) {
        Some(row) if !row[lang as usize].is_empty() => row[lang as usize],
        Some(row) => row[0],
        None => "",
    }
}

pub fn tr(lang: Lang, key: &'static str) -> &'static str {
    match table().get(key) {
        Some(row) if !row[lang as usize].is_empty() => row[lang as usize],
        Some(row) => row[0],
        None => key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_has_all_languages() {
        for line in TSV.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            assert_eq!(line.split('\t').count(), 9, "row: {line}");
        }
    }

    /// Every `t("key")` used by the interface must exist in the table.
    #[test]
    fn all_ui_keys_are_translated() {
        let sources = [include_str!("app.rs")];
        let mut missing = Vec::new();
        for src in sources {
            let mut rest = src;
            while let Some(i) = rest.find("t(\"") {
                let after = &rest[i + 3..];
                if let Some(end) = after.find('"') {
                    let key = &after[..end];
                    let prev = rest[..i].chars().last().unwrap_or(' ');
                    if !prev.is_alphanumeric() && prev != '_' && !table().contains_key(key) {
                        missing.push(key.to_string());
                    }
                }
                rest = &rest[i + 3..];
            }
        }
        assert!(missing.is_empty(), "untranslated keys: {missing:?}");
    }

    #[test]
    fn locale_parsing() {
        assert_eq!(Lang::from_code("ru_RU.UTF-8"), Some(Lang::Ru));
        assert_eq!(Lang::from_code("zh-CN"), Some(Lang::Zh));
        assert_eq!(Lang::from_code("pt-BR"), None);
        assert_eq!(tr(Lang::Ru, "send"), "Отправить");
    }
}
