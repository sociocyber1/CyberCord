//! "Words matter": a tiny, local recogniser for insults and swearing in the
//! user's *own* outgoing messages.
//!
//! Nothing is blocked, hidden, rate-limited or reported: the message is sent
//! exactly as written. The only effect is that the sender is offered a short
//! document about general semantics (Korzybski) and the cost of escalation.
//! The counter lives in the local settings file and never leaves the machine.
//!
//! Matching is deliberately simple: word-start stems with an explicit list of
//! allowed Russian prefixes (so "страхуется", "вебинар" or "употребляя" do
//! not match), exact forms for ambiguous words, `ё→е`, Latin look-alikes in
//! Cyrillic words, and collapsed letter repeats ("сууука"). It is a hint for
//! oneself, not an enforcement tool, so it does not try to beat obfuscation.

/// How a pattern is matched against one normalised word.
enum Pat {
    /// the word starts with the stem
    Stem(&'static str),
    /// the whole word equals this form
    Exact(&'static str),
    /// one of the prefixes, then the stem, at the start of the word
    Pre(&'static [&'static str], &'static str),
    /// substring anywhere (for scripts without spaces: Chinese, Japanese)
    Sub(&'static str),
}
use Pat::*;

// Russian verbal/adverbial prefixes that are safe in front of the strongest roots.
const P_EB: &[&str] = &["", "у", "за", "на", "по", "до", "вы", "от", "отъ", "про", "под", "подъ", "при", "раз", "разъ", "съ", "изъ", "объ", "недо", "пере", "долбо", "долба"];
const P_HUY: &[&str] = &["", "на", "по", "за", "до", "о", "ни", "от", "с", "пере", "раз"];
const P_PIZD: &[&str] = &["", "с", "у", "за", "на", "по", "до", "вы", "от", "про", "под", "рас", "рос"];

const PATTERNS: &[Pat] = &[
    // --- Russian / Ukrainian: swearing roots
    Pre(P_HUY, "хуй"),
    Pre(P_HUY, "хуе"),
    Pre(P_HUY, "хуя"),
    Pre(P_HUY, "хуи"),
    Pre(P_PIZD, "пизд"),
    Pre(P_EB, "еба"),
    Pre(P_EB, "ебл"),
    Pre(P_EB, "ебу"),
    Pre(P_EB, "ебн"),
    Pre(P_EB, "ебт"),
    Pre(P_EB, "еби"),
    Pre(P_EB, "ебо"),
    Exact("бля"),
    Stem("бляд"),
    Stem("блят"),
    Exact("сука"),
    Exact("суки"),
    Exact("суку"),
    Exact("суке"),
    Exact("сукой"),
    Stem("сучк"),
    Stem("сучар"),
    Stem("муда"),
    Stem("муди"),
    Stem("мудо"),
    // --- Russian / Ukrainian: insults aimed at a person
    Stem("долбо"),
    Stem("дебил"),
    Stem("идиот"),
    Stem("кретин"),
    Stem("придур"),
    Stem("ублюд"),
    Stem("урод"),
    Stem("мраз"),
    Stem("гнид"),
    Exact("тварь"),
    Exact("твари"),
    Exact("чмо"),
    Stem("чмошн"),
    Exact("лох"),
    Stem("лошар"),
    Stem("пидор"),
    Stem("пидар"),
    Stem("педик"),
    Stem("шлюх"),
    Stem("гандон"),
    Stem("гондон"),
    Stem("залуп"),
    Stem("говнюк"),
    Stem("говноед"),
    Stem("скотин"),
    Stem("ничтожеств"),
    Exact("дура"),
    Stem("дурак"),
    Stem("тупиц"),
    Stem("тупорыл"),
    Stem("курв"),
    Stem("падлюк"),
    Exact("падла"),
    // --- English
    Stem("fuck"),
    Stem("motherf"),
    Stem("ashol"), // asshole (letter repeats are collapsed)
    Stem("bastard"),
    Stem("bitch"),
    Stem("cunt"),
    Exact("dick"),
    Stem("dickhead"),
    Stem("idiot"),
    Stem("moron"),
    Stem("retard"),
    Exact("scum"),
    Stem("slut"),
    Stem("whor"),
    Stem("wanker"),
    Stem("twat"),
    Exact("prick"),
    Stem("dumbas"),
    Stem("jackas"),
    // --- German
    Stem("arschloch"),
    Stem("wichser"),
    Stem("fotze"),
    Stem("hurensohn"),
    Stem("vollidiot"),
    Stem("schlampe"),
    // --- Spanish
    Exact("puta"),
    Stem("cabron"),
    Stem("cabrón"),
    Stem("gilipol"),
    Stem("pendej"),
    Stem("imbecil"),
    Stem("imbécil"),
    // --- French
    Stem("conard"), // connard (collapsed)
    Stem("conas"),  // connasse
    Stem("salope"),
    Stem("encul"),
    Exact("con"),
    Stem("imbécile"),
    // --- Chinese / Japanese (no spaces: substring)
    Sub("傻逼"),
    Sub("操你"),
    Sub("他妈的"),
    Sub("王八蛋"),
    Sub("混蛋"),
    Sub("贱人"),
    Sub("くそ野郎"),
    Sub("クソ野郎"),
    Sub("死ね"),
    Sub("バカ野郎"),
    Sub("ばかやろう"),
];

/// Lower-case, `ё→е`, Latin look-alikes inside Cyrillic words, repeats collapsed.
fn norm_word(w: &str) -> String {
    let lower = w.to_lowercase();
    let cyr = lower.chars().any(|c| ('а'..='я').contains(&c) || c == 'ё' || c == 'і' || c == 'ї' || c == 'є');
    let mut out = String::with_capacity(lower.len());
    let mut last = '\0';
    for c in lower.chars() {
        let c = match c {
            'ё' => 'е',
            'a' if cyr => 'а',
            'e' if cyr => 'е',
            'o' if cyr => 'о',
            'p' if cyr => 'р',
            'c' if cyr => 'с',
            'x' if cyr => 'х',
            'y' if cyr => 'у',
            'k' if cyr => 'к',
            'm' if cyr => 'м',
            't' if cyr => 'т',
            'i' if cyr => 'и',
            'u' if cyr => 'и',
            '0' if cyr => 'о',
            '3' if cyr => 'з',
            _ => c,
        };
        if c != last {
            out.push(c);
        }
        last = c;
    }
    out
}

fn collapse(s: &str) -> String {
    let mut out = String::new();
    let mut last = '\0';
    for c in s.chars() {
        if c != last {
            out.push(c);
        }
        last = c;
    }
    out
}

fn word_hits(w: &str) -> bool {
    PATTERNS.iter().any(|p| match p {
        Stem(s) => w.starts_with(&collapse(s)),
        Exact(s) => w == collapse(s),
        Pre(pres, root) => pres.iter().any(|pre| {
            let full = collapse(&format!("{pre}{root}"));
            w.starts_with(&full)
        }),
        Sub(_) => false,
    })
}

/// True if the message contains at least one insult or swear word.
pub fn is_insult(text: &str) -> bool {
    // skip links, code and @mentions/peer ids: they are not words someone said
    let lower = text.to_lowercase();
    if PATTERNS.iter().any(|p| matches!(p, Sub(s) if lower.contains(s))) {
        return true;
    }
    lower
        .split_whitespace()
        .filter(|t| !t.contains("://") && !t.starts_with('@') && !t.starts_with('`'))
        .flat_map(|t| t.split(|c: char| !(c.is_alphanumeric())))
        .filter(|w| !w.is_empty())
        .any(|w| word_hits(&norm_word(w)))
}

#[cfg(test)]
mod tests {
    use super::is_insult;

    #[test]
    fn recognises_insults_and_swearing() {
        for s in [
            "ты идиот",
            "Ну ты и ДЕБИЛ!!!",
            "сууука",
            "иди нахуй",
            "это пиздец какой-то",
            "заебал уже",
            "блять, опять",
            "долбоёб",
            "ты урод",
            "xуйня", // Latin x
            "you are an idiot",
            "f*** you, fucking moron",
            "what an asshole",
            "du Arschloch",
            "eres un gilipollas",
            "quel connard",
            "你这个傻逼",
        ] {
            assert!(is_insult(s), "{s}");
        }
    }

    #[test]
    fn leaves_ordinary_words_alone() {
        for s in [
            "Машина страхуется на год",
            "завтра вебинар по Rust",
            "Себастьян пришёл",
            "употребляя термины точно",
            "бляха-муха, забыл",
            "сук на дереве",
            "купи хлеба",
            "небо голубое",
            "скипидар",
            "колебания струны",
            "Scunthorpe United",
            "classic assessment",
            "Dickens wrote novels",
            "https://example.com/fuck-path",
            "мудрость приходит с опытом",
            "конференция и контроль",
            "погреба полны",
        ] {
            assert!(!is_insult(s), "{s}");
        }
    }
}
