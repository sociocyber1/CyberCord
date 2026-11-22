//! The SocioCyberneering city map: districts → branches (sciences, projects,
//! designs, CAD tools …) → text and voice channels. Anyone can subscribe to
//! a branch; its channels then appear in the side panel. Channels are plain
//! gossip topics (`city.<branch>/<channel>`), so no server is involved.

pub struct District {
    pub slug: &'static str,
    pub ru: &'static str,
    pub en: &'static str,
    /// GNOME accent (r, g, b)
    pub color: (u8, u8, u8),
}

pub struct Branch {
    pub slug: &'static str,
    pub district: &'static str,
    pub ru: &'static str,
    pub en: &'static str,
    /// what the branch is for — shown on entering and when subscribing
    pub goal_ru: &'static str,
    pub goal_en: &'static str,
}

/// Text channels every branch has, and its voice room.
pub const TEXT: &[(&str, &str, &str)] = &[("chat", "общий", "chat"), ("projects", "проекты", "projects"), ("help", "помощь", "help")];
pub const VOICE: (&str, &str, &str) = ("room", "голосовая", "voice room");

pub const DISTRICTS: &[District] = &[
    District { slug: "science", ru: "Науки", en: "Sciences", color: (0x1c, 0x71, 0xd8) },
    District { slug: "projects", ru: "Проекты", en: "Projects", color: (0x26, 0xa2, 0x69) },
    District { slug: "design", ru: "Дизайн", en: "Design", color: (0xc0, 0x61, 0xcb) },
    District { slug: "cad", ru: "CAD и инструменты", en: "CAD & tools", color: (0xe6, 0x61, 0x00) },
    District { slug: "eng", ru: "Инженерия", en: "Engineering", color: (0x98, 0x6a, 0x44) },
    District { slug: "habitat", ru: "Город и среда", en: "City & habitat", color: (0x2e, 0xc2, 0x7e) },
    District { slug: "gs", ru: "Общая семантика", en: "General semantics", color: (0x81, 0x3d, 0x9c) },
    District { slug: "sce", ru: "SocioCyberneering", en: "SocioCyberneering", color: (0x35, 0x84, 0xe4) },
];

macro_rules! br {
    ($( $slug:literal, $d:literal, $ru:literal, $en:literal, $gru:literal, $gen:literal; )*) => {
        &[ $( Branch { slug: $slug, district: $d, ru: $ru, en: $en, goal_ru: $gru, goal_en: $gen }, )* ]
    };
}

pub const BRANCHES: &[Branch] = br![
    // sciences
    "physics", "science", "Физика", "Physics", "Законы природы: от механики до квантов. Разбираем задачи, статьи и эксперименты.", "Laws of nature from mechanics to quanta: problems, papers, experiments.";
    "math", "science", "Математика", "Mathematics", "Учимся и помогаем: от школьной математики до доказательств и численных методов.", "Learn and help: from school maths to proofs and numerical methods.";
    "chemistry", "science", "Химия", "Chemistry", "Вещества, реакции, синтез — в том числе для Atom-Generator.", "Substances, reactions, synthesis — Atom-Generator included.";
    "biology", "science", "Биология", "Biology", "Жизнь, клетки, экосистемы, биотехнологии.", "Life, cells, ecosystems, biotechnology.";
    "astronomy", "science", "Астрономия", "Astronomy", "Звёзды, планеты, нуклеосинтез: откуда берутся атомы.", "Stars, planets, nucleosynthesis: where atoms come from.";
    "geology", "science", "Геология и материалы", "Geology & materials", "Минералы, ресурсы планеты, материалы из минералов.", "Minerals, the planet's resources, materials from minerals.";
    "neuro", "science", "Нейронаука и психология", "Neuroscience & psychology", "Как работает нервная система и поведение — по фактам и исследованиям.", "How the nervous system and behaviour work — by facts and research.";
    "papers", "science", "Научные статьи", "Papers", "Ищем, читаем и обсуждаем статьи: ссылка, вывод, что проверено.", "Find, read and discuss papers: link, claim, what was tested.";
    // projects
    "atom-generator", "projects", "Atom-Generator", "Atom-Generator", "Машина, собирающая еду и материалы из простых веществ. С чего начать — в документе сервера.", "A machine assembling food and materials from simple substances.";
    "cybercord", "projects", "CyberCord", "CyberCord", "Развитие этого мессенджера: идеи, ошибки, протокол, переводы.", "Developing this messenger: ideas, bugs, protocol, translations.";
    "hydroponics", "projects", "Гидропоника и аквапоника", "Hydroponics & aquaponics", "Выращивание без почвы, датчики, автоматика, урожай.", "Soil-free growing, sensors, automation, harvests.";
    "energy", "projects", "Энергия", "Energy", "Солнце, ветер, накопители: энергия в изобилии для всех.", "Sun, wind, storage: abundant energy for everyone.";
    "open-hardware", "projects", "Открытое железо", "Open hardware", "Устройства с открытыми схемами и прошивками.", "Devices with open schematics and firmware.";
    "education", "projects", "Образование", "Education", "Курсы, учебники, наставничество — знания открыты для всех.", "Courses, books, mentoring — knowledge open to all.";
    "translations", "projects", "Переводы", "Translations", "Переводим книги и материалы (общая семантика, Проект Венера) на разные языки.", "Translating books and materials into many languages.";
    // design
    "ui-design", "design", "UI и UX", "UI & UX", "Интерфейсы, удобство, доступность.", "Interfaces, usability, accessibility.";
    "industrial", "design", "Промышленный дизайн", "Industrial design", "Предметы и машины, спроектированные для людей и долговечности.", "Objects and machines designed for people and durability.";
    "graphics", "design", "Графика и бренд", "Graphics & brand", "Иллюстрации, иконки, стиль SocioCyberneering.", "Illustrations, icons, the SocioCyberneering style.";
    "motion", "design", "3D и анимация", "3D & animation", "Визуализации, анимации, ролики проекта.", "Visualisations, animations, project videos.";
    "architecture-design", "design", "Архитектурный дизайн", "Architectural design", "Здания и пространства: эскизы, модели, разборы.", "Buildings and spaces: sketches, models, reviews.";
    // CAD & tools
    "freecad", "cad", "FreeCAD", "FreeCAD", "Параметрическое 3D-моделирование: вопросы, модели, приёмы.", "Parametric 3D modelling: questions, models, tips.";
    "kicad", "cad", "KiCad", "KiCad", "Схемы и печатные платы: ревью, библиотеки, производство.", "Schematics and PCBs: reviews, libraries, manufacturing.";
    "blender", "cad", "Blender", "Blender", "3D-графика, рендер, анимация, визуализация городов.", "3D graphics, rendering, animation, city visualisation.";
    "openscad", "cad", "OpenSCAD", "OpenSCAD", "Модели кодом: параметры, библиотеки, 3D-печать.", "Models as code: parameters, libraries, 3D printing.";
    "latex", "cad", "LaTeX", "LaTeX", "Документы, книги, формулы и схемы TikZ.", "Documents, books, formulas and TikZ drawings.";
    "simulation", "cad", "Симуляции", "Simulations", "Моделирование систем, городов, физики; инструменты и результаты.", "Modelling systems, cities, physics; tools and results.";
    // engineering
    "electronics", "eng", "Электроника", "Electronics", "Схемы, пайка, измерения, силовая электроника.", "Circuits, soldering, measurement, power electronics.";
    "embedded", "eng", "Встраиваемые системы", "Embedded systems", "Микроконтроллеры, Orange Pi, Rust на железе.", "Microcontrollers, Orange Pi, Rust on hardware.";
    "robotics", "eng", "Робототехника", "Robotics", "Механика, приводы, управление, автоматизация труда.", "Mechanics, actuators, control, automating labour.";
    "fabrication", "eng", "Изготовление", "Fabrication", "3D-печать, ЧПУ, мастерские.", "3D printing, CNC, workshops.";
    "networks", "eng", "Сети и P2P", "Networks & P2P", "Связь без центров: протоколы, mesh, безопасность.", "Communication without centres: protocols, mesh, security.";
    "ai", "eng", "ИИ", "AI", "Модели, инструменты, применение ИИ в проектах — с проверкой фактов.", "Models, tools, AI in projects — with fact checking.";
    // habitat
    "circular-city", "habitat", "Круговой город", "Circular city", "Проектируем город по научному методу: зоны, транспорт, ресурсы.", "Designing a city by the scientific method: zones, transport, resources.";
    "transport", "habitat", "Транспорт", "Transport", "Перемещение людей и грузов без пробок и выхлопа.", "Moving people and goods without traffic jams or exhaust.";
    "food", "habitat", "Еда и агро", "Food & agriculture", "Как накормить всех: агро, ферментация, вертикальные фермы.", "Feeding everyone: farming, fermentation, vertical farms.";
    "water", "habitat", "Вода", "Water", "Очистка, опреснение, круговорот воды в городе.", "Purification, desalination, the city's water cycle.";
    "ecology", "habitat", "Экология", "Ecology", "Замкнутые циклы, отходы как ресурс, восстановление природы.", "Closed loops, waste as a resource, restoring nature.";
    // general semantics
    "gs-basics", "gs", "Основы общей семантики", "GS basics", "Карта — не территория, связывание времени, структурный дифференциал.", "Map is not territory, time-binding, the structural differential.";
    "gs-practice", "gs", "Практика", "Practice", "Экстенсиональные приёмы в жизни: индексы, даты, пауза, проверка фактов.", "Extensional devices in life: indices, dates, pause, fact checking.";
    "gs-reading", "gs", "Чтение Коржибски", "Reading Korzybski", "Читаем «Науку и здравомыслие», «Зрелость человечества» и семинары.", "Reading Science and Sanity, Manhood of Humanity and the seminars.";
    "gs-books", "gs", "Книги и переводы", "Books & translations", "Учебники, пособия, переводы по общей семантике.", "Textbooks, manuals, translations on general semantics.";
    // SocioCyberneering
    "rbe", "sce", "Ресурсо-ориентированная экономика", "Resource-based economy", "Мир без политики, нищеты и войн: как распределять ресурсы научно.", "A world without politics, poverty and war: allocating resources scientifically.";
    "venus-ideas", "sce", "Идеи Проекта Венера", "Venus Project ideas", "Идеи Жака Фреско (до 2017 года): разбор, проверка, развитие.", "Jacque Fresco's ideas (until 2017): study, test, develop.";
    "sociocybernetics", "sce", "Социокибернетика", "Sociocybernetics", "Общество как система с обратной связью: модели и эксперименты.", "Society as a feedback system: models and experiments.";
    "cooperation", "sce", "Кооперация", "Cooperation", "Ищем людей в проекты по всему SocioCyberneering: кто что умеет и что нужно.", "Finding people for projects across SocioCyberneering: skills and needs.";
    "events", "sce", "События", "Events", "Встречи, созвоны, хакатоны и общие сборки.", "Meetings, calls, hackathons and builds together.";
];

pub fn branch(slug: &str) -> Option<&'static Branch> {
    BRANCHES.iter().find(|b| b.slug == slug)
}

pub fn district(slug: &str) -> Option<&'static District> {
    DISTRICTS.iter().find(|d| d.slug == slug)
}

/// The guild part of a branch's channel ids.
pub fn guild(branch: &str) -> String {
    format!("city.{branch}")
}

/// Ready-made messages offered in every branch (and some per district).
pub fn ready_messages(branch: &Branch, ru: bool) -> Vec<String> {
    let name = if ru { branch.ru } else { branch.en };
    let mut v = if ru {
        vec![
            format!("Ищу соратников в проект ({name}): цель — …, нужно — …, срок — …"),
            "Вопрос: … Что уже пробовал: … Факты (версия, дата, файлы): …".to_string(),
            "Делюсь результатом: … (дата, версия, что проверено, где файлы)".to_string(),
        ]
    } else {
        vec![
            format!("Looking for people for a project ({name}): goal — …, needed — …, timeline — …"),
            "Question: … What I tried: … Facts (version, date, files): …".to_string(),
            "Sharing a result: … (date, version, what was tested, where the files are)".to_string(),
        ]
    };
    if branch.district == "cad" {
        v.push(if ru { format!("Нужна помощь с {name}: версия …, что хочу сделать …, файл модели приложу") } else { format!("Need help with {name}: version …, what I want to do …, model file attached") });
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_branch_has_a_district_and_unique_slug() {
        let mut slugs: Vec<&str> = BRANCHES.iter().map(|b| b.slug).collect();
        slugs.sort();
        slugs.dedup();
        assert_eq!(slugs.len(), BRANCHES.len());
        assert!(BRANCHES.iter().all(|b| district(b.district).is_some()));
        assert!(BRANCHES.len() >= 40);
    }
}
