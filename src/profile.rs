//! User profiles: avatar, interests, activity, skills (0–10 with a comment),
//! work and tasks. A profile is signed by its owner and spread over the
//! control topic, so anyone can verify it without a server.
//!
//! From the skills a short **index code** is made, e.g. `Rs8·El7·GS6+2`
//! (top three skills with levels, plus how many more). It is shown next to
//! names; the profile view decodes it. The profile itself is dated and
//! indexed like in general semantics: version₃ · 21.11.2026.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Skill {
    /// catalog code (e.g. "Rs") or free text
    pub id: String,
    /// 0..=10
    pub level: u8,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Profile {
    pub peer: String,
    pub name: String,
    /// small JPEG, base64 (≤ ~40 KB)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub avatar: String,
    #[serde(default)]
    pub interests: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub activity: String,
    #[serde(default)]
    pub skills: Vec<Skill>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub job: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tasks: String,
    /// version index (1, 2, …) and when this version was saved
    #[serde(default)]
    pub rev: u32,
    #[serde(default)]
    pub ts_ms: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sig: String,
}

impl Profile {
    fn signing_bytes(&self) -> Vec<u8> {
        let mut c = self.clone();
        c.sig.clear();
        let mut v = b"cybercord/profile/v1\n".to_vec();
        v.extend(serde_json::to_vec(&c).unwrap_or_default());
        v
    }
    pub fn sign(&mut self, key: &libp2p::identity::Keypair) {
        use base64::Engine;
        self.peer = key.public().to_peer_id().to_base58();
        self.sig.clear();
        if let Ok(s) = key.sign(&self.signing_bytes()) {
            self.sig = base64::engine::general_purpose::STANDARD_NO_PAD.encode(s);
        }
    }
    pub fn verify(&self) -> bool {
        use base64::Engine;
        let (Some(pk), Ok(sig)) = (crate::types::public_key_of(&self.peer), base64::engine::general_purpose::STANDARD_NO_PAD.decode(&self.sig)) else {
            return false;
        };
        self.avatar.len() <= 64 * 1024 && pk.verify(&self.signing_bytes(), &sig)
    }

    /// Skills sorted by level, highest first.
    pub fn top_skills(&self) -> Vec<&Skill> {
        let mut v: Vec<&Skill> = self.skills.iter().collect();
        v.sort_by(|a, b| b.level.cmp(&a.level).then(a.id.cmp(&b.id)));
        v
    }

    /// The compact index code: top three skills with levels, then "+N".
    pub fn code(&self) -> String {
        let top = self.top_skills();
        if top.is_empty() {
            return String::new();
        }
        let s: Vec<String> = top.iter().take(3).map(|k| format!("{}{}", short_code(&k.id), k.level.min(10))).collect();
        let more = if top.len() > 3 { format!("+{}", top.len() - 3) } else { String::new() };
        format!("{}{more}", s.join("·"))
    }
}

/// Catalog code if known, else the first letters of a free-text skill.
pub fn short_code(id: &str) -> String {
    if find(id).is_some() {
        return id.to_string();
    }
    let mut it = id.chars().filter(|c| c.is_alphanumeric()).take(2);
    let first: String = it.next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    format!("{first}{}", it.collect::<String>())
}

/// One entry of the catalog: code, group, Russian and English names.
pub struct Entry {
    pub code: &'static str,
    pub group: &'static str,
    pub ru: &'static str,
    pub en: &'static str,
}

macro_rules! cat {
    ($( $code:literal, $group:literal, $ru:literal, $en:literal; )*) => { &[ $( Entry { code: $code, group: $group, ru: $ru, en: $en }, )* ] };
}

/// 100 skills and interests to pick from, grouped like districts of a city.
pub const CATALOG: &[Entry] = cat![
    // science
    "Ma", "science", "Математика", "Mathematics";
    "Ph", "science", "Физика", "Physics";
    "Ch", "science", "Химия", "Chemistry";
    "Bi", "science", "Биология", "Biology";
    "As", "science", "Астрономия", "Astronomy";
    "Gl", "science", "Геология", "Geology";
    "Ec", "science", "Экология", "Ecology";
    "Me", "science", "Медицина", "Medicine";
    "Nu", "science", "Нутрициология", "Nutrition science";
    "Ps", "science", "Психология", "Psychology";
    "Ne", "science", "Нейронаука", "Neuroscience";
    "St", "science", "Статистика", "Statistics";
    "Ln", "science", "Лингвистика", "Linguistics";
    "Hi", "science", "История науки", "History of science";
    "Ms", "science", "Материаловедение", "Materials science";
    "Nn", "science", "Нанотехнологии", "Nanotechnology";
    "Gn", "science", "Генетика", "Genetics";
    "Op", "science", "Оптика", "Optics";
    // general semantics & SCE ideas
    "GS", "gs", "Общая семантика", "General semantics";
    "Sd", "gs", "Структурный дифференциал", "Structural differential";
    "Ep", "gs", "E-Prime / точный язык", "E-Prime / precise language";
    "Ex", "gs", "Экстенсиональные приёмы", "Extensional devices";
    "Tb", "gs", "Связывание времени", "Time-binding";
    "Sm", "gs", "Научный метод", "Scientific method";
    "Ct", "gs", "Критическое мышление", "Critical thinking";
    "Pb", "gs", "Решение проблем", "Problem solving";
    "Sc", "sce", "Социокибернетика", "Sociocybernetics";
    "Cy", "sce", "Кибернетика", "Cybernetics";
    "Sy", "sce", "Системное мышление", "Systems thinking";
    "Rb", "sce", "Ресурсо-ориентированная экономика", "Resource-based economy";
    "Vp", "sce", "Идеи Проекта Венера", "Venus Project ideas";
    "Cc", "sce", "Круговые города", "Circular cities";
    "Su", "sce", "Устойчивое развитие", "Sustainability";
    "Ag", "sce", "Atom-Generator", "Atom-Generator";
    // engineering & making
    "En", "eng", "Инженерия", "Engineering";
    "El", "eng", "Электроника", "Electronics";
    "Pc", "eng", "Платы (KiCad)", "PCB design (KiCad)";
    "Em", "eng", "Встраиваемые системы", "Embedded systems";
    "Ro", "eng", "Робототехника", "Robotics";
    "Au", "eng", "Автоматизация", "Automation";
    "Io", "eng", "Датчики и IoT", "Sensors & IoT";
    "Pw", "eng", "Силовая электроника", "Power electronics";
    "Me2", "eng", "Механика", "Mechanics";
    "Ca", "eng", "CAD / 3D-моделирование", "CAD / 3D modelling";
    "Fb", "eng", "3D-печать и станки", "3D printing & CNC";
    "Hy", "eng", "Гидропоника / аквапоника", "Hydroponics / aquaponics";
    "Ey", "eng", "Энергетика", "Energy systems";
    "So", "eng", "Солнечная энергия", "Solar power";
    "Ar", "eng", "Архитектура", "Architecture";
    "Ur", "eng", "Урбанистика", "Urban planning";
    "Cn", "eng", "Строительство", "Construction";
    "Tr", "eng", "Транспорт", "Transport";
    // computing
    "Rs", "code", "Rust", "Rust";
    "Py", "code", "Python", "Python";
    "Js", "code", "JavaScript / TypeScript", "JavaScript / TypeScript";
    "Cp", "code", "C / C++", "C / C++";
    "Go", "code", "Go", "Go";
    "Wb", "code", "Веб-разработка", "Web development";
    "Fe", "code", "Фронтенд / UI", "Front-end / UI";
    "Bk", "code", "Бэкенд", "Back-end";
    "Db", "code", "Базы данных", "Databases";
    "Lx", "code", "Linux / администрирование", "Linux / sysadmin";
    "Nw", "code", "Сети / P2P", "Networking / P2P";
    "Se", "code", "Безопасность", "Security";
    "Ai", "code", "ИИ / машинное обучение", "AI / machine learning";
    "Da", "code", "Анализ данных", "Data analysis";
    "Si", "code", "Симуляции", "Simulations";
    "Gm", "code", "Разработка игр", "Game development";
    "Mo", "code", "Мобильная разработка", "Mobile development";
    "Dv", "code", "DevOps / CI", "DevOps / CI";
    "Ltx", "code", "LaTeX / документация", "LaTeX / documentation";
    "Os", "code", "Open source", "Open source";
    // design & art
    "Ds", "art", "Дизайн", "Design";
    "Ux", "art", "UX / интерфейсы", "UX / interfaces";
    "Gr", "art", "Графика / иллюстрация", "Graphics / illustration";
    "Ty", "art", "Типографика", "Typography";
    "An", "art", "Анимация", "Animation";
    "Vi", "art", "Видео / монтаж", "Video / editing";
    "Ft", "art", "Фотография", "Photography";
    "Mu", "art", "Музыка", "Music";
    "Sn", "art", "Звук", "Sound";
    "Wr", "art", "Тексты / литература", "Writing / literature";
    "3d", "art", "3D-графика", "3D graphics";
    "Ga", "art", "Генеративное искусство", "Generative art";
    // people & organisation
    "Ed", "people", "Преподавание", "Teaching";
    "Tl", "people", "Переводы", "Translation";
    "Pm", "people", "Управление проектами", "Project management";
    "Co", "people", "Коммуникация / медиация", "Communication / mediation";
    "Cm", "people", "Сообщества", "Community building";
    "Re", "people", "Исследования", "Research";
    "Sa", "people", "Научпоп / объяснять", "Science outreach";
    "Lg", "people", "Право", "Law";
    "Fi", "people", "Финансы / учёт", "Finance / accounting";
    "Lo", "people", "Логистика", "Logistics";
    "Hc", "people", "Здоровье и спорт", "Health & sport";
    "Ck", "people", "Кулинария", "Cooking";
    "Ga2", "people", "Садоводство", "Gardening";
    "Ws", "people", "Мастерская / ремесло", "Workshop / crafts";
    "Mt", "people", "Наставничество", "Mentoring";
    "Ev", "people", "Мероприятия", "Events";
];

impl Profile {
    /// Does this person do / know what the query asks? (Specialist search: a skill
    /// code like "El", a skill name in Russian or English, interests, work, tasks.)
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.chars().count() < 2 {
            return false;
        }
        let skill = |id: &str| -> bool {
            id.to_lowercase() == q || find(id).map(|e| e.ru.to_lowercase().contains(&q) || e.en.to_lowercase().contains(&q)).unwrap_or(false)
        };
        self.skills.iter().any(|s| skill(&s.id) || s.note.to_lowercase().contains(&q))
            || self.interests.iter().any(|i| skill(i) || i.to_lowercase().contains(&q))
            || (q.chars().count() >= 3 && (self.job.to_lowercase().contains(&q) || self.tasks.to_lowercase().contains(&q) || self.activity.to_lowercase().contains(&q)))
    }
}

pub fn find(code: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|e| e.code == code)
}

/// Display name of a skill/interest in Russian or English.
pub fn name(code: &str, ru: bool) -> String {
    match find(code) {
        Some(e) => (if ru { e.ru } else { e.en }).to_string(),
        None => code.to_string(),
    }
}

pub const GROUPS: &[(&str, &str, &str)] = &[
    ("science", "Науки", "Sciences"),
    ("gs", "Общая семантика", "General semantics"),
    ("sce", "SocioCyberneering", "SocioCyberneering"),
    ("eng", "Инженерия", "Engineering"),
    ("code", "Программирование", "Computing"),
    ("art", "Дизайн и искусство", "Design & art"),
    ("people", "Люди и организация", "People & organisation"),
];

// ---- storage of known profiles ----

fn path() -> std::path::PathBuf {
    crate::store::data_dir().join("profiles.json")
}

pub fn load_all() -> Vec<Profile> {
    std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save_all(list: &[Profile]) {
    if let Ok(b) = serde_json::to_vec(list) {
        let p = path();
        let tmp = p.with_extension("tmp");
        if std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_100_unique_codes() {
        assert_eq!(CATALOG.len(), 100);
        let mut codes: Vec<&str> = CATALOG.iter().map(|e| e.code).collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), 100);
    }

    #[test]
    fn code_and_signature() {
        let k = libp2p::identity::Keypair::generate_ed25519();
        let mut p = Profile { name: "Alex Merser".into(), skills: vec![
            Skill { id: "GS".into(), level: 6, note: String::new() },
            Skill { id: "Rs".into(), level: 8, note: "async, egui".into() },
            Skill { id: "El".into(), level: 7, note: String::new() },
            Skill { id: "Pc".into(), level: 5, note: String::new() },
            Skill { id: "свой навык".into(), level: 4, note: String::new() },
        ], rev: 3, ..Default::default() };
        assert_eq!(p.code(), "Rs8·El7·GS6+2");
        p.sign(&k);
        assert!(p.verify());
        p.job = "changed".into();
        assert!(!p.verify());
        assert_eq!(short_code("свой навык"), "Св");
    }
}
