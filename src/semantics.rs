//! A small, local helper for extensional (non-Aristotelian) conversation.
//!
//! It looks for wordings that general semantics warns about — allness,
//! the "is" of identity about a person, mind reading, either-or, jumping to
//! conclusions, loaded evaluations, demands — and for markers of dogmatic,
//! cult-like talk. It then sorts sentences by levels of abstraction in the
//! spirit of Korzybski's structural differential: description (closer to the
//! facts), inference, evaluation.
//!
//! It is a heuristic word list, not an oracle: it is shown as a hint, never
//! used to block or hide anything. The analysis is also a map, not the
//! territory.

use crate::words;

/// What a finding is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// "always", "never", "everyone": a generalisation.
    Allness,
    /// "you are an X": a person identified with a label.
    Identity,
    /// "you did it on purpose", "you just want…": guessing someone's mind.
    MindReading,
    /// "either … or", "with us or against us": two-valued thinking.
    EitherOr,
    /// "obviously", "everyone knows": a conclusion presented as a fact.
    Jump,
    /// "terrible", "nonsense", "the best": a loaded evaluation.
    Evaluation,
    /// "you must": a demand on another person.
    Demand,
    /// insult or swearing (see [`crate::words`]).
    Insult,
    /// dogmatic / cult-like markers: "the only truth", "don't question".
    Dogma,
}

impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Kind::Allness => "allness",
            Kind::Identity => "identity",
            Kind::MindReading => "mind",
            Kind::EitherOr => "eitheror",
            Kind::Jump => "jump",
            Kind::Evaluation => "eval",
            Kind::Demand => "demand",
            Kind::Insult => "insult",
            Kind::Dogma => "dogma",
        }
    }
    /// Kinds that heat a conversation up.
    pub fn heats(self) -> bool {
        matches!(self, Kind::Identity | Kind::MindReading | Kind::Insult | Kind::EitherOr | Kind::Demand | Kind::Evaluation)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub kind: Kind,
    /// the words that matched, as written
    pub text: String,
}

/// Level of abstraction of one sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// facts: numbers, dates, quotes, what was seen or said
    Description,
    /// conclusions, guesses, generalisations
    Inference,
    /// judgements, labels, insults
    Evaluation,
    /// none of the markers: could be anything (questions, greetings…)
    Unclear,
}

// ---- word lists (lower case, "ё" written as "е") ----

const ALLNESS: &[&str] = &["всегда", "никогда", "никто", "никому", "ничто", "каждый раз", "вечно", "постоянно", "все без исключения", "always", "never", "nobody", "everyone", "everybody", "every time", "no one"];
const PEOPLE: &[&str] = &["ты", "вы", "он", "она", "они", "you", "he", "she", "they", "you're", "he's", "she's", "they're"];
const LABELS: &[&str] = &[
    "дурак", "дура", "лжец", "лжецы", "врун", "вруны", "тупой", "тупая", "тупые", "ленивый", "ленивая", "ленивые", "эгоист", "эгоистка", "эгоисты", "токсичный", "токсичная", "неудачник", "неудачница", "неудачники", "бесполезный", "бесполезная", "бесполезные", "плохой", "плохая", "плохие", "ужасный", "ужасная", "ужасные", "жалкий", "жалкая", "психованный", "больной", "больная", "ненормальный", "ненормальная", "предатель", "предатели", "враг", "враги", "фанатик", "фанатики",
    "stupid", "liar", "liars", "lazy", "selfish", "toxic", "loser", "losers", "useless", "pathetic", "crazy", "insane", "traitor", "enemy", "dumb", "worthless",
];
const LINKS: &[&str] = &["—", "-", "–", "есть", "просто", "же", "всего лишь", "такой", "такая", "такие", "настоящий", "настоящая", "a", "an", "are", "is", "just", "such", "so"];
const MIND: &[&str] = &[
    "ты специально", "вы специально", "он специально", "она специально", "ты нарочно", "вы нарочно", "ты хочешь", "вы хотите", "ты просто хочешь", "вы просто хотите", "на самом деле ты", "на самом деле вы", "ты думаешь, что", "вы думаете, что", "я знаю, что ты", "я знаю, что вы", "тебе плевать", "вам плевать", "тебе все равно", "вам все равно",
    "you did it on purpose", "on purpose", "you just want", "you only want", "deep down you", "i know you", "you don't care", "you think that",
];
const EITHER: &[&str] = &["кто не с нами", "третьего не дано", "или ты", "либо ты", "либо вы", "или вы", "with us or against us", "either you", "there is no middle"];
const JUMP: &[&str] = &["очевидно", "понятно же", "ясно же", "ясное дело", "все знают", "всем известно", "это факт", "без сомнения", "несомненно", "obviously", "clearly", "everyone knows", "it's a fact", "no doubt"];
const EVAL: &[&str] = &[
    "ужасно", "ужасный", "ужасная", "отвратительно", "отвратительный", "кошмар", "позор", "позорище", "бред", "чушь", "ерунда", "глупость", "глупо", "абсурд", "невыносимо", "катастрофа", "худший", "худшая", "лучший в мире", "идеально", "мерзко", "мерзкий", "дичь",
    "terrible", "awful", "disgusting", "nonsense", "rubbish", "garbage", "ridiculous", "absurd", "worst", "horrible", "pathetic",
];
const DEMAND: &[&str] = &["ты должен", "ты должна", "вы должны", "ты обязан", "ты обязана", "вы обязаны", "немедленно", "you must", "you have to", "you should"];
const DOGMA: &[&str] = &[
    "единственная истина", "единственно верн", "абсолютная истина", "истина в последней инстанции", "только мы знаем", "только мы", "мы избранные", "избранные", "не задавай вопросов", "не задавайте вопросов", "не сомневайся", "не сомневайтесь", "сомнение — это", "сомнения от лукавого", "учитель сказал", "учитель всегда прав", "лидер всегда прав", "непогрешим", "все остальные заблуждаются", "все остальные ошибаются", "тайное знание", "просто поверь", "просто верь", "верь мне", "доказательства не нужны", "без доказательств", "не обсуждается", "порви с семьей", "отрекись", "спасение только", "только так и никак иначе", "кто сомневается",
    "the only truth", "absolute truth", "only we know", "chosen ones", "don't question", "do not question", "never question", "stop doubting", "the leader is always right", "the teacher is always right", "everyone else is wrong", "secret knowledge", "just believe", "just trust me", "no need for evidence", "not up for discussion", "cut ties with your family",
];
const INFER: &[&str] = &["значит", "следовательно", "наверное", "видимо", "похоже", "кажется", "думаю", "по-моему", "скорее всего", "вероятно", "therefore", "probably", "seems", "i think", "i guess", "apparently", "likely"];
const OBSERVE: &[&str] = &["видел", "видела", "слышал", "слышала", "сказал", "сказала", "написал", "написала", "измерил", "посчитал", "в протоколе", "по логам", "saw", "heard", "said", "wrote", "measured", "according to", "logs show"];

fn norm(s: &str) -> String {
    s.to_lowercase().replace('ё', "е")
}

/// Positions where a phrase occurs as whole words.
fn find_phrase(hay: &str, needle: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = hay[from..].find(needle) {
        let at = from + i;
        let end = at + needle.len();
        let before_ok = hay[..at].chars().next_back().map_or(true, |c| !c.is_alphanumeric());
        // a stem ending (e.g. "единственно верн") may continue inside the word
        let after_ok = needle.ends_with(|c: char| !c.is_alphanumeric()) || hay[end..].chars().next().map_or(true, |c| !c.is_alphanumeric()) || needle.ends_with("верн");
        if before_ok && after_ok {
            return Some(at);
        }
        from = at + needle.len().max(1);
    }
    None
}

fn tokens(s: &str) -> Vec<&str> {
    s.split(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | '!' | '?' | ';' | ':' | '(' | ')' | '"' | '«' | '»'))
        .filter(|t| !t.is_empty())
        .collect()
}

/// All findings in a message, at most one per phrase.
pub fn scan(text: &str) -> Vec<Finding> {
    let n = norm(text);
    let mut out: Vec<Finding> = Vec::new();
    let mut push = |kind: Kind, t: &str| {
        if !out.iter().any(|f| f.kind == kind && f.text == t) {
            out.push(Finding { kind, text: t.to_string() });
        }
    };
    for (list, kind) in [(DOGMA, Kind::Dogma), (MIND, Kind::MindReading), (EITHER, Kind::EitherOr), (JUMP, Kind::Jump), (DEMAND, Kind::Demand), (ALLNESS, Kind::Allness), (EVAL, Kind::Evaluation)] {
        for p in list {
            if find_phrase(&n, p).is_some() {
                push(kind, p);
            }
        }
    }
    // either … or inside one sentence: "или … или", "либо … либо"
    for s in sentences(&n) {
        for w in ["или", "либо", "either"] {
            let toks = tokens(&s);
            let count = toks.iter().filter(|t| **t == w).count();
            if count >= 2 || (w == "either" && toks.contains(&"or")) {
                push(Kind::EitherOr, &format!("{w} … {}", if w == "either" { "or" } else { w }));
            }
        }
        // identity: person + (link words) + label, within a few words
        let toks = tokens(&s);
        for (i, t) in toks.iter().enumerate() {
            if PEOPLE.contains(t) {
                for j in i + 1..(i + 4).min(toks.len()) {
                    let w = toks[j];
                    if LABELS.contains(&w) || words::is_insult(w) {
                        push(Kind::Identity, &format!("{t} … {w}"));
                        break;
                    }
                    if !LINKS.contains(&w) && !w.chars().all(|c| !c.is_alphanumeric()) {
                        break;
                    }
                }
            }
        }
    }
    if words::is_insult(text) {
        push(Kind::Insult, "");
    }
    out
}

/// Split into sentences (keeps the text of each, trimmed).
pub fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        cur.push(c);
        if matches!(c, '.' | '!' | '?' | '\n' | '…') {
            let t = cur.trim().to_string();
            if t.chars().any(|c| c.is_alphanumeric()) {
                out.push(t);
            }
            cur.clear();
        }
    }
    let t = cur.trim().to_string();
    if t.chars().any(|c| c.is_alphanumeric()) {
        out.push(t);
    }
    out
}

/// One sentence with its level and findings.
#[derive(Debug, Clone)]
pub struct Line {
    pub text: String,
    pub level: Level,
    pub findings: Vec<Finding>,
}

pub fn levels(text: &str) -> Vec<Line> {
    sentences(text)
        .into_iter()
        .map(|s| {
            let f = scan(&s);
            let n = norm(&s);
            let has = |k: Kind| f.iter().any(|x| x.kind == k);
            let level = if has(Kind::Insult) || has(Kind::Identity) || has(Kind::Evaluation) {
                Level::Evaluation
            } else if has(Kind::MindReading) || has(Kind::Allness) || has(Kind::Jump) || has(Kind::EitherOr) || has(Kind::Dogma) || INFER.iter().any(|p| find_phrase(&n, p).is_some()) {
                Level::Inference
            } else if s.chars().any(|c| c.is_ascii_digit()) || s.contains('«') || s.contains('"') || s.contains("://") || OBSERVE.iter().any(|p| find_phrase(&n, p).is_some()) {
                Level::Description
            } else {
                Level::Unclear
            };
            Line { text: s, level, findings: f }
        })
        .collect()
}

/// How heated the recent part of a conversation looks: number of messages
/// with heating findings and number of distinct authors among them.
pub fn heat<'a>(recent: impl Iterator<Item = (&'a str, &'a str)>) -> (usize, usize) {
    let mut msgs = 0;
    let mut authors: Vec<&str> = Vec::new();
    for (author, body) in recent {
        if scan(body).iter().any(|f| f.kind.heats()) {
            msgs += 1;
            if !authors.contains(&author) {
                authors.push(author);
            }
        }
    }
    (msgs, authors.len())
}

// ---- extensional devices for the composer ----

const SUB: [char; 10] = ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈', '₉'];
const SUP: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];

/// "Иван" → "Иван₁", "Иван₁" → "Иван₂" (indexing: Иван₁ ≠ Иван₂).
pub fn index(word: &str) -> String {
    let base: String = word.trim_end_matches(|c| SUB.contains(&c)).to_string();
    let digits: String = word[base.len()..].chars().map(|c| char::from_digit(SUB.iter().position(|s| *s == c).unwrap_or(0) as u32, 10).unwrap_or('0')).collect();
    let n: u32 = digits.parse().unwrap_or(0) + 1;
    format!("{base}{}", n.to_string().chars().map(|c| SUB[c.to_digit(10).unwrap_or(0) as usize]).collect::<String>())
}

/// Split "Иван₁₂" into ("Иван", Some(12)).
pub fn split_index(word: &str) -> (&str, Option<u32>) {
    let base = word.trim_end_matches(|c| SUB.contains(&c));
    let digits: String = word[base.len()..].chars().filter_map(|c| SUB.iter().position(|s| *s == c)).map(|d| char::from_digit(d as u32, 10).unwrap_or('0')).collect();
    (base, digits.parse().ok())
}

/// "Иван" + 3 → "Иван₃".
pub fn with_index(base: &str, n: u32) -> String {
    format!("{base}{}", n.to_string().chars().map(|c| SUB[c.to_digit(10).unwrap_or(0) as usize]).collect::<String>())
}

/// The index to give `word` next: if it has one, the following number;
/// otherwise one more than the largest index of the same word in `context`
/// (the draft and the recent messages), so Иван₁ in the chat makes Иван₂.
pub fn next_index(word: &str, context: &str) -> u32 {
    let (base, own) = split_index(word);
    if let Some(n) = own {
        return n + 1;
    }
    let low = base.to_lowercase();
    let mut max = 0;
    for tok in context.split(|c: char| !(c.is_alphanumeric() || SUB.contains(&c) || c == '-')) {
        let (b, n) = split_index(tok);
        if let Some(n) = n {
            if b.to_lowercase() == low {
                max = max.max(n);
            }
        }
    }
    max + 1
}

/// Append a date in superscript (dating: Иван²⁰²⁶ ≠ Иван²⁰¹⁶): "²¹·¹¹·²⁰²⁶".
pub fn date(word: &str, day: u32, month: u32, year: i32) -> String {
    let sup = |s: String| s.chars().map(|c| c.to_digit(10).map(|d| SUP[d as usize]).unwrap_or(c)).collect::<String>();
    format!("{word}{}·{}·{}", sup(format!("{day:02}")), sup(format!("{month:02}")), sup(year.to_string()))
}

/// Safety quotes: «слово».
pub fn quote(word: &str) -> String {
    format!("«{word}»")
}

/// Hyphen: "пространство время" → "пространство-время".
pub fn hyphen(words: &str) -> String {
    words.split_whitespace().collect::<Vec<_>>().join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(s: &str) -> Vec<Kind> {
        scan(s).into_iter().map(|f| f.kind).collect()
    }

    #[test]
    fn finds_risky_wordings() {
        assert!(kinds("Ты всегда опаздываешь").contains(&Kind::Allness));
        assert!(kinds("ты — лжец").contains(&Kind::Identity));
        assert!(kinds("Вы просто ленивые").contains(&Kind::Identity));
        assert!(kinds("you are a liar").contains(&Kind::Identity));
        assert!(kinds("Ты специально это сделал").contains(&Kind::MindReading));
        assert!(kinds("Или ты с нами, или против").contains(&Kind::EitherOr));
        assert!(kinds("Очевидно, он виноват").contains(&Kind::Jump));
        assert!(kinds("Это бред").contains(&Kind::Evaluation));
        assert!(kinds("Не задавай вопросов, учитель всегда прав").contains(&Kind::Dogma));
        assert!(kinds("This is the only truth, don't question it").contains(&Kind::Dogma));
    }

    #[test]
    fn quiet_on_plain_facts() {
        for s in ["Встреча 21.11 в 18:00 у Ивана", "Сервер упал в 14:02, лог приложил", "Привет! Как дела?", "Я видел, что сборка прошла", "Каждый вторник у нас созвон"] {
            assert!(scan(s).is_empty(), "{s}: {:?}", scan(s));
        }
    }

    #[test]
    fn levels_of_abstraction() {
        let l = levels("Сборка упала в 14:02. Наверное, виноват кэш. Это ужасно!");
        assert_eq!(l.iter().map(|x| x.level).collect::<Vec<_>>(), vec![Level::Description, Level::Inference, Level::Evaluation]);
    }

    #[test]
    fn devices() {
        assert_eq!(index("Иван"), "Иван₁");
        assert_eq!(index("Иван₉"), "Иван₁₀");
        assert_eq!(next_index("Иван", "вчера Иван₁ сказал, а Иван₃ нет"), 4);
        assert_eq!(next_index("иван", "привет"), 1);
        assert_eq!(next_index("Иван₂", "Иван₇"), 3);
        assert_eq!(split_index("Иван₁₂"), ("Иван", Some(12)));
        assert_eq!(with_index("Иван", 5), "Иван₅");
        assert_eq!(date("Иван", 21, 11, 2026), "Иван²¹·¹¹·²⁰²⁶");
        assert_eq!(quote("ум"), "«ум»");
        assert_eq!(hyphen("пространство время"), "пространство-время");
    }
}
