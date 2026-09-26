//! A compiler for agent instructions. AGENTS.md, CLAUDE.md, .rules, cursor
//! rules and similar files are read together and checked the way code is:
//! rules that contradict each other, rules repeated across files, paths that
//! no longer exist, and files too long for an agent to follow reliably.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Files agents read for instructions, relative to a project root.
pub const INSTRUCTION_FILES: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    ".rules",
    ".cursorrules",
    ".windsurfrules",
    ".clinerules",
    "GEMINI.md",
    ".github/copilot-instructions.md",
    ".noah/memory.md",
    ".noah/preferences.md",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub file: String,
    pub line: usize,
    pub text: String,
    pub polarity: Polarity,
    #[serde(default)]
    pub language: Language,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Polarity {
    Do,
    Dont,
    Neutral,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Finding {
    Contradiction { first: Rule, second: Rule },
    Duplicate { first: Rule, second: Rule },
    MissingPath { rule: Rule, path: String },
    TooLong { file: String, lines: usize },
}

/// The languages instruction files are checked in. noah's interface ships in
/// about twenty languages, and a Spanish or German AGENTS.md deserves the
/// same contradiction check as an English one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    En,
    Es,
    De,
    Fr,
    Pt,
    It,
    Pl,
    Nl,
    Tr,
    Ru,
    Ja,
    Zh,
    Ko,
}

struct Lexicon {
    language: Language,
    /// Words and phrases that make a rule a prohibition. Checked before
    /// `do_words`, so "verwende niemals" is a prohibition even though
    /// "verwende" alone is an instruction.
    dont_words: &'static [&'static str],
    do_words: &'static [&'static str],
    /// Function words: used to tell languages apart and left out when
    /// comparing what two rules are about.
    stop_words: &'static [&'static str],
    /// Languages written without spaces between words, where phrases are
    /// matched anywhere and rules are compared by character pairs.
    unspaced: bool,
}

const LEXICONS: &[Lexicon] = &[
    Lexicon {
        language: Language::En,
        dont_words: &[
            "never", "don't", "do not", "avoid", "must not", "mustn't", "no longer", "stop",
            "without", "should not", "shouldn't", "cannot", "can't",
        ],
        do_words: &["always", "must", "use", "prefer", "should", "ensure", "do", "make sure"],
        stop_words: &[
            "a", "an", "the", "to", "of", "in", "on", "for", "and", "or", "is", "are", "be", "it",
            "this", "that", "with", "when", "you", "your", "we", "our", "any", "all", "as", "by",
            "at", "from", "not", "no", "longer", "instead", "please", "make", "sure",
        ],
        unspaced: false,
    },
    Lexicon {
        language: Language::Es,
        dont_words: &["nunca", "jamás", "no", "evita", "evite", "eviten", "evitar", "prohibido", "sin"],
        do_words: &[
            "siempre", "debe", "debes", "deben", "usa", "usar", "use", "utiliza", "utilice",
            "utilizar", "prefiere", "preferir", "asegúrate", "asegúrese", "hay que",
        ],
        stop_words: &[
            "el", "la", "los", "las", "un", "una", "de", "del", "en", "y", "o", "que", "es", "son",
            "para", "por", "con", "al", "se", "lo", "su", "sus", "tu", "tus", "este", "esta",
            "cuando", "como", "hay",
        ],
        unspaced: false,
    },
    Lexicon {
        language: Language::De,
        dont_words: &[
            "nie", "niemals", "nicht", "kein", "keine", "keinen", "vermeide", "vermeiden",
            "verboten", "ohne",
        ],
        do_words: &[
            "immer", "stets", "muss", "müssen", "verwende", "verwenden", "benutze", "benutzen",
            "nutze", "nutzen", "bevorzuge", "bevorzugen", "sollte", "sollten", "stelle sicher",
            "stellen sie sicher",
        ],
        stop_words: &[
            "der", "die", "das", "den", "dem", "des", "ein", "eine", "einen", "und", "oder", "ist",
            "sind", "für", "mit", "von", "zu", "zur", "zum", "im", "in", "auf", "bei", "wenn",
            "als", "du", "sie", "wir", "es", "nur", "sicher", "stelle", "stellen",
        ],
        unspaced: false,
    },
    Lexicon {
        language: Language::Fr,
        dont_words: &["jamais", "ne", "pas", "évite", "évitez", "éviter", "interdit", "sans"],
        do_words: &[
            "toujours", "doit", "doivent", "dois", "utilise", "utilisez", "utiliser", "préfère",
            "préférez", "préférer", "assurez-vous", "assure-toi", "il faut",
        ],
        stop_words: &[
            "le", "la", "les", "un", "une", "des", "de", "du", "en", "et", "ou", "que", "qui",
            "est", "sont", "pour", "par", "avec", "au", "aux", "ce", "cette", "dans", "sur",
            "vous", "votre", "vos", "quand", "comme", "il", "faut", "l'", "d'", "n'",
        ],
        unspaced: false,
    },
    Lexicon {
        language: Language::Pt,
        dont_words: &["nunca", "jamais", "não", "evite", "evitar", "proibido", "sem"],
        do_words: &[
            "sempre", "deve", "devem", "use", "usar", "utilize", "utilizar", "prefira",
            "preferir", "garanta", "certifique-se",
        ],
        stop_words: &[
            "o", "a", "os", "as", "um", "uma", "de", "do", "da", "dos", "das", "em", "no", "na",
            "nos", "nas", "e", "ou", "que", "é", "são", "para", "por", "com", "se", "seu", "sua",
            "este", "esta", "quando", "como",
        ],
        unspaced: false,
    },
    Lexicon {
        language: Language::It,
        dont_words: &["mai", "non", "evita", "evitare", "evitate", "vietato", "senza"],
        do_words: &[
            "sempre", "deve", "devi", "devono", "usa", "usare", "utilizza", "utilizzare",
            "preferisci", "preferire", "assicurati",
        ],
        stop_words: &[
            "il", "lo", "la", "i", "gli", "le", "un", "una", "di", "del", "della", "dei", "in",
            "nei", "e", "o", "che", "è", "sono", "per", "con", "al", "si", "su", "quando", "come",
            "questo", "questa",
        ],
        unspaced: false,
    },
    Lexicon {
        language: Language::Pl,
        dont_words: &["nigdy", "nie", "unikaj", "unikać", "zabronione", "bez"],
        do_words: &[
            "zawsze", "musi", "musisz", "muszą", "używaj", "używać", "stosuj", "preferuj",
            "należy", "powinien", "powinno", "upewnij",
        ],
        stop_words: &[
            "i", "w", "z", "na", "do", "się", "to", "jest", "są", "że", "o", "a", "od", "po",
            "dla", "przy", "jak", "gdy", "lub", "czy", "ten", "ta", "we", "ze",
        ],
        unspaced: false,
    },
    Lexicon {
        language: Language::Nl,
        dont_words: &["nooit", "niet", "geen", "vermijd", "vermijden", "verboden", "zonder"],
        do_words: &[
            "altijd", "moet", "moeten", "gebruik", "gebruiken", "gebruikt", "voorkeur",
            "zorg ervoor", "zorg",
        ],
        stop_words: &[
            "de", "het", "een", "en", "of", "is", "zijn", "voor", "met", "van", "te", "in", "op",
            "bij", "als", "je", "jij", "we", "wij", "dat", "die", "dit", "wanneer", "ervoor",
        ],
        unspaced: false,
    },
    Lexicon {
        language: Language::Tr,
        dont_words: &[
            "asla", "hiçbir zaman", "değil", "yasak", "yasaktır", "kaçının", "kaçın", "sakın",
            "kullanma", "kullanmayın", "yapma", "yapmayın", "etme", "etmeyin", "olmadan",
        ],
        do_words: &[
            "her zaman", "daima", "mutlaka", "kullanın", "kullan", "tercih", "gerekir",
            "gerekli", "emin olun",
        ],
        stop_words: &[
            "ve", "veya", "bir", "bu", "şu", "için", "ile", "da", "de", "ki", "mi", "ne", "o",
            "olarak", "gibi", "her", "çok", "daha", "en", "ise", "zaman", "emin", "olun",
        ],
        unspaced: false,
    },
    Lexicon {
        language: Language::Ru,
        dont_words: &[
            "никогда", "не", "нельзя", "избегай", "избегайте", "избегать", "запрещено", "без",
        ],
        do_words: &[
            "всегда", "должен", "должна", "должны", "используй", "используйте", "использовать",
            "предпочитай", "предпочитайте", "следует", "обязательно", "убедитесь",
        ],
        stop_words: &[
            "и", "в", "во", "на", "с", "со", "по", "к", "у", "о", "из", "за", "для", "что", "это",
            "как", "а", "но", "или", "если", "то", "же", "при", "от", "до", "ты", "вы", "мы",
        ],
        unspaced: false,
    },
    Lexicon {
        language: Language::Ja,
        dont_words: &[
            "してはいけない", "ないでください", "使用しない", "使わない", "しないで", "しない",
            "するな", "いけません", "禁止", "決して", "避け", "べからず",
        ],
        do_words: &[
            "してください", "すること", "使用する", "ください", "常に", "必ず", "いつも", "使う",
            "べき", "推奨",
        ],
        stop_words: &[],
        unspaced: true,
    },
    Lexicon {
        language: Language::Zh,
        dont_words: &[
            "永远不要", "不要", "禁止", "不得", "不能", "绝不", "絕不", "避免", "不应", "不應",
            "不可", "严禁", "嚴禁", "不准", "别", "勿",
        ],
        do_words: &[
            "总是", "總是", "始终", "始終", "一直", "必须", "必須", "务必", "務必", "使用", "应该",
            "應該", "应当", "應當", "确保", "確保", "优先", "優先", "请", "請", "要",
        ],
        stop_words: &[],
        unspaced: true,
    },
    Lexicon {
        language: Language::Ko,
        dont_words: &[
            "하지 마", "하지 말", "하지 않", "사용하지", "마세요", "말 것", "말아야", "금지",
            "절대", "피하", "피해", "않도록",
        ],
        do_words: &["항상", "반드시", "사용하", "해야", "하세요", "권장", "확인하", "늘"],
        stop_words: &[],
        unspaced: true,
    },
];

/// Unambiguous English words still recognized in files written in another
/// language, where technical rules are often left in English. Short words
/// like "do" or "use" are left out: they mean other things elsewhere ("do"
/// is "to" in Polish).
const ENGLISH_FALLBACK_DONT: &[&str] = &["never", "don't", "do not", "must not", "avoid"];
const ENGLISH_FALLBACK_DO: &[&str] = &["always", "must", "should", "prefer", "ensure"];

fn lexicon(language: Language) -> &'static Lexicon {
    LEXICONS
        .iter()
        .find(|lexicon| lexicon.language == language)
        .unwrap_or(&LEXICONS[0])
}

fn is_japanese_kana(character: char) -> bool {
    matches!(character, '\u{3040}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9F}')
}

fn is_hangul(character: char) -> bool {
    matches!(character, '\u{AC00}'..='\u{D7AF}' | '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}')
}

fn is_han(character: char) -> bool {
    matches!(character, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}')
}

fn is_unspaced_script(character: char) -> bool {
    is_japanese_kana(character) || is_hangul(character) || is_han(character)
}

/// Guesses the language of an instruction file: by script for Japanese,
/// Chinese, Korean and Russian, and otherwise by which language's function
/// words it uses most. Falls back to English.
pub fn detect_language(text: &str) -> Language {
    let (mut kana, mut hangul, mut han, mut cyrillic, mut letters) = (0usize, 0usize, 0usize, 0usize, 0usize);
    for character in text.chars().filter(|character| character.is_alphabetic()) {
        letters += 1;
        if is_japanese_kana(character) {
            kana += 1;
        } else if is_hangul(character) {
            hangul += 1;
        } else if is_han(character) {
            han += 1;
        } else if matches!(character, '\u{0400}'..='\u{04FF}') {
            cyrillic += 1;
        }
    }
    let significant = |count: usize| count * 10 >= letters.max(1);
    if significant(kana) || (kana > 0 && significant(kana + han)) {
        return Language::Ja;
    }
    if significant(hangul) {
        return Language::Ko;
    }
    if significant(han) {
        return Language::Zh;
    }
    if significant(cyrillic) {
        return Language::Ru;
    }

    let lowercase = text.to_lowercase();
    let words: Vec<&str> = lowercase
        .split(|character: char| !(character.is_alphanumeric() || character == '\''))
        .filter(|word| !word.is_empty())
        .collect();
    let score = |lexicon: &Lexicon| -> usize {
        words
            .iter()
            .filter(|word| {
                lexicon.stop_words.contains(word)
                    || lexicon.dont_words.contains(word)
                    || lexicon.do_words.contains(word)
            })
            .count()
    };
    let english = score(lexicon(Language::En));
    LEXICONS
        .iter()
        .filter(|lexicon| !lexicon.unspaced && lexicon.language != Language::Ru)
        .map(|lexicon| (lexicon.language, score(lexicon)))
        // A clear margin over English is needed: English words such as "a",
        // "in" and "do" also appear in other languages' lists.
        .filter(|(language, score)| *language == Language::En || *score > english + english / 5)
        .max_by_key(|(_, score)| *score)
        .map(|(language, _)| language)
        .unwrap_or(Language::En)
}

fn contains_phrase(text: &str, phrase: &str, unspaced: bool) -> bool {
    if unspaced || phrase.chars().any(is_unspaced_script) {
        text.contains(phrase)
    } else {
        contains_word(text, phrase)
    }
}

/// Turkish forms most prohibitions and obligations with verb suffixes
/// ("kullanmamalı", "kullanmalı") rather than separate words.
fn turkish_suffix_polarity(text: &str) -> Option<Polarity> {
    let words = text.split(|character: char| !character.is_alphanumeric());
    let mut polarity = None;
    for word in words {
        if ["mayın", "meyin", "mamalı", "memeli", "mamalısın", "memelisin"]
            .iter()
            .any(|suffix| word.ends_with(suffix))
        {
            return Some(Polarity::Dont);
        }
        if ["malı", "meli", "malısın", "melisin", "malıdır", "melidir"]
            .iter()
            .any(|suffix| word.ends_with(suffix))
        {
            polarity = Some(Polarity::Do);
        }
    }
    polarity
}

fn polarity(text: &str, language: Language) -> Polarity {
    let lowercase = text.to_lowercase();
    let lexicon = lexicon(language);
    let english_dont: &[&str] = if language == Language::En { &[] } else { ENGLISH_FALLBACK_DONT };
    let english_do: &[&str] = if language == Language::En { &[] } else { ENGLISH_FALLBACK_DO };
    let has_any = |words: &[&str]| {
        words
            .iter()
            .any(|word| contains_phrase(&lowercase, word, lexicon.unspaced))
    };
    let turkish = (language == Language::Tr)
        .then(|| turkish_suffix_polarity(&lowercase))
        .flatten();
    if has_any(lexicon.dont_words) || has_any(english_dont) || turkish == Some(Polarity::Dont) {
        Polarity::Dont
    } else if has_any(lexicon.do_words) || has_any(english_do) || turkish == Some(Polarity::Do) {
        Polarity::Do
    } else {
        Polarity::Neutral
    }
}

/// Pulls rules (bullets and imperative sentences) out of an instruction file.
pub fn extract_rules(file: &str, text: &str) -> Vec<Rule> {
    let language = detect_language(text);
    let unspaced = lexicon(language).unspaced;
    let mut in_code = false;
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                in_code = !in_code;
                return None;
            }
            if in_code || trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let body = trimmed
                .trim_start_matches(['-', '*', '+', '・', '•'])
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')')
                .trim();
            let long_enough = if unspaced {
                body.chars().filter(|character| character.is_alphanumeric()).count() >= 6
            } else {
                body.split_whitespace().count() >= 3
            };
            if !long_enough {
                return None;
            }
            Some(Rule {
                file: file.to_string(),
                line: index + 1,
                text: body.to_string(),
                polarity: polarity(body, language),
                language,
            })
        })
        .collect()
}

fn contains_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(start, _)| {
        let before = text[..start].chars().last();
        let after = text[start + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// What a rule is about: its words without function words and without the
/// words that only say whether to do it. Text in scripts written without
/// spaces is compared by pairs of characters, since there are no words to
/// split on.
fn content_words(text: &str, language: Language) -> BTreeSet<String> {
    let english = lexicon(Language::En);
    let lexicon = lexicon(language);
    let mut lowercase = text.to_lowercase();
    if lexicon.unspaced {
        let mut phrases: Vec<&str> = lexicon
            .dont_words
            .iter()
            .chain(lexicon.do_words)
            .copied()
            .collect();
        phrases.sort_by_key(|phrase| std::cmp::Reverse(phrase.chars().count()));
        for phrase in phrases {
            lowercase = lowercase.replace(phrase, " ");
        }
    }
    let is_modality_or_stop = |word: &str| {
        [lexicon, english].iter().any(|lexicon| {
            lexicon.stop_words.contains(&word)
                || lexicon
                    .dont_words
                    .iter()
                    .chain(lexicon.do_words)
                    .any(|phrase| phrase.split_whitespace().any(|part| part == word))
        })
    };
    let mut words = BTreeSet::new();
    let add_word = |word: &str, words: &mut BTreeSet<String>| {
        let word = word.trim_matches('.').trim_matches('\'');
        if word.chars().count() > 1 && !is_modality_or_stop(word) {
            words.insert(word.to_string());
        }
    };
    let add_run = |run: &[char], words: &mut BTreeSet<String>| {
        if run.len() == 1 {
            words.insert(run.iter().collect());
        }
        for pair in run.windows(2) {
            words.insert(pair.iter().collect());
        }
    };
    for token in lowercase.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '\'' || c == '.')) {
        let mut spaced = String::new();
        let mut run: Vec<char> = Vec::new();
        for character in token.chars() {
            if is_unspaced_script(character) {
                add_word(&spaced, &mut words);
                spaced.clear();
                run.push(character);
            } else {
                add_run(&run, &mut words);
                run.clear();
                spaced.push(character);
            }
        }
        add_word(&spaced, &mut words);
        add_run(&run, &mut words);
    }
    words
}

fn similarity(first: &Rule, second: &Rule) -> f32 {
    let first = content_words(&first.text, first.language);
    let second = content_words(&second.text, second.language);
    if first.is_empty() || second.is_empty() {
        return 0.0;
    }
    let shared = first.intersection(&second).count() as f32;
    shared / first.union(&second).count() as f32
}

fn backticked_paths(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .filter(|span| {
            let has_extension = span.rsplit_once('.').is_some_and(|(stem, extension)| {
                !stem.is_empty()
                    && (1..=5).contains(&extension.len())
                    && extension.chars().all(|c| c.is_ascii_alphanumeric())
            });
            (span.contains('/') || has_extension)
                && !span.contains(' ')
                && !span.contains('(')
                && !span.starts_with('-')
                && !span.contains("://")
                && span.chars().any(|c| c.is_alphabetic())
        })
        .map(str::to_string)
        .collect()
}

/// Checks instruction files together. `path_exists` resolves paths the rules
/// mention against the project.
pub fn check(files: &[(String, String)], path_exists: impl Fn(&str) -> bool) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut rules = Vec::new();
    for (file, text) in files {
        let lines = text.lines().count();
        if lines > 400 {
            findings.push(Finding::TooLong {
                file: file.clone(),
                lines,
            });
        }
        rules.extend(extract_rules(file, text));
    }
    for (index, first) in rules.iter().enumerate() {
        for second in &rules[index + 1..] {
            let score = similarity(first, second);
            let opposite = matches!(
                (first.polarity, second.polarity),
                (Polarity::Do, Polarity::Dont) | (Polarity::Dont, Polarity::Do)
            );
            if opposite && score >= 0.5 {
                findings.push(Finding::Contradiction {
                    first: first.clone(),
                    second: second.clone(),
                });
            } else if !opposite && score >= 0.85 && first.file != second.file {
                findings.push(Finding::Duplicate {
                    first: first.clone(),
                    second: second.clone(),
                });
            }
        }
        for path in backticked_paths(&first.text) {
            if path.contains('*') || path.contains('<') || path.contains('{') {
                continue;
            }
            if !path_exists(&path) {
                findings.push(Finding::MissingPath {
                    rule: first.clone(),
                    path,
                });
            }
        }
    }
    findings
}

pub fn report(findings: &[Finding], files_checked: usize, rules_found: usize) -> String {
    let mut out = format!(
        "# agent instructions check\n\n{files_checked} file(s), {rules_found} rule(s), {} finding(s)\n\n",
        findings.len()
    );
    for finding in findings {
        match finding {
            Finding::Contradiction { first, second } => out.push_str(&format!(
                "- contradiction: {}:{} \"{}\" vs {}:{} \"{}\"\n",
                first.file, first.line, first.text, second.file, second.line, second.text
            )),
            Finding::Duplicate { first, second } => out.push_str(&format!(
                "- duplicate: {}:{} repeats {}:{} \"{}\"\n",
                second.file, second.line, first.file, first.line, first.text
            )),
            Finding::MissingPath { rule, path } => out.push_str(&format!(
                "- stale path: {}:{} mentions `{path}`, which doesn't exist\n",
                rule.file, rule.line
            )),
            Finding::TooLong { file, lines } => out.push_str(&format!(
                "- too long: {file} has {lines} lines; agents follow short, specific rules more reliably\n"
            )),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_contradictions_duplicates_and_stale_paths() {
        let files = vec![
            (
                "AGENTS.md".to_string(),
                "# rules\n- Always use tabs for indentation in Rust files.\n- Run tests with `script/test` before committing.\n```\nnever run this\n```\n".to_string(),
            ),
            (
                ".rules".to_string(),
                "- Never use tabs for indentation in Rust files.\n- Run tests with `script/test` before committing.\n".to_string(),
            ),
        ];
        let findings = check(&files, |path| path != "script/test");
        let kinds: Vec<&str> = findings
            .iter()
            .map(|finding| match finding {
                Finding::Contradiction { .. } => "contradiction",
                Finding::Duplicate { .. } => "duplicate",
                Finding::MissingPath { .. } => "missing",
                Finding::TooLong { .. } => "long",
            })
            .collect();
        assert_eq!(kinds, ["contradiction", "duplicate", "missing", "missing"]);
    }

    fn kinds(findings: &[Finding]) -> Vec<&'static str> {
        findings
            .iter()
            .map(|finding| match finding {
                Finding::Contradiction { .. } => "contradiction",
                Finding::Duplicate { .. } => "duplicate",
                Finding::MissingPath { .. } => "missing",
                Finding::TooLong { .. } => "long",
            })
            .collect()
    }

    #[test]
    fn detects_languages() {
        for (text, language) in [
            ("- Always run the tests before you commit and keep the diff small.", Language::En),
            ("- Siempre ejecuta las pruebas antes de hacer commit y mantén el cambio pequeño.", Language::Es),
            ("- Führe immer die Tests aus, bevor du committest, und halte den Diff klein.", Language::De),
            ("- Exécutez toujours les tests avant de valider et gardez le diff petit.", Language::Fr),
            ("- Sempre execute os testes antes de fazer commit e mantenha o diff pequeno.", Language::Pt),
            ("- Esegui sempre i test prima di fare commit e mantieni il diff piccolo.", Language::It),
            ("- Zawsze uruchamiaj testy przed commitem i utrzymuj małe zmiany w kodzie.", Language::Pl),
            ("- Voer altijd de tests uit voor je commit en houd de diff klein.", Language::Nl),
            ("- Commit etmeden önce her zaman testleri çalıştır ve bu değişikliği küçük tut.", Language::Tr),
            ("- Всегда запускай тесты перед коммитом и делай маленькие изменения.", Language::Ru),
            ("- コミットする前に必ずテストを実行してください。", Language::Ja),
            ("- 提交之前必须运行测试，并保持改动很小。", Language::Zh),
            ("- 커밋하기 전에 항상 테스트를 실행하세요.", Language::Ko),
        ] {
            assert_eq!(detect_language(text), language, "{text}");
        }
    }

    #[test]
    fn finds_contradictions_in_every_language() {
        for (language, first, second) in [
            (
                Language::Es,
                "- Siempre usa tabulaciones para la indentación en archivos Rust.",
                "- Nunca uses tabulaciones para la indentación en archivos Rust.",
            ),
            (
                Language::De,
                "- Verwende immer Tabs für die Einrückung in Rust-Dateien.",
                "- Verwende niemals Tabs für die Einrückung in Rust-Dateien.",
            ),
            (
                Language::Fr,
                "- Utilisez toujours des tabulations pour l'indentation des fichiers Rust.",
                "- N'utilisez jamais de tabulations pour l'indentation des fichiers Rust.",
            ),
            (
                Language::Pt,
                "- Sempre use tabulações para a indentação em arquivos Rust.",
                "- Nunca use tabulações para a indentação em arquivos Rust.",
            ),
            (
                Language::It,
                "- Usa sempre le tabulazioni per l'indentazione nei file Rust.",
                "- Non usare mai le tabulazioni per l'indentazione nei file Rust.",
            ),
            (
                Language::Pl,
                "- Zawsze używaj tabulatorów do wcięć w plikach Rust.",
                "- Nigdy nie używaj tabulatorów do wcięć w plikach Rust.",
            ),
            (
                Language::Nl,
                "- Gebruik altijd tabs voor inspringen in Rust-bestanden.",
                "- Gebruik nooit tabs voor inspringen in Rust-bestanden.",
            ),
            (
                Language::Tr,
                "- Rust dosyalarında girinti için her zaman sekme kullanın.",
                "- Rust dosyalarında girinti için asla sekme kullanmayın.",
            ),
            (
                Language::Ru,
                "- Всегда используй табы для отступов в файлах Rust.",
                "- Никогда не используй табы для отступов в файлах Rust.",
            ),
            (
                Language::Ja,
                "- Rustファイルでは常にタブでインデントしてください。",
                "- Rustファイルではタブでインデントしないでください。",
            ),
            (
                Language::Zh,
                "- 始终使用制表符缩进 Rust 文件。",
                "- 不要使用制表符缩进 Rust 文件。",
            ),
            (
                Language::Ko,
                "- Rust 파일에는 항상 탭으로 들여쓰기를 하세요.",
                "- Rust 파일에는 절대 탭으로 들여쓰기를 하지 마세요.",
            ),
        ] {
            let files = vec![
                ("AGENTS.md".to_string(), format!("{first}\n")),
                (".rules".to_string(), format!("{second}\n")),
            ];
            let rules = extract_rules("AGENTS.md", first);
            assert_eq!(rules.len(), 1, "{first}");
            assert_eq!(rules[0].language, language, "{first}");
            assert_eq!(rules[0].polarity, Polarity::Do, "{first}");
            let rules = extract_rules(".rules", second);
            assert_eq!(rules[0].polarity, Polarity::Dont, "{second}");
            assert_eq!(kinds(&check(&files, |_| true)), ["contradiction"], "{language:?}");
        }
    }

    #[test]
    fn unrelated_rules_in_other_languages_are_fine() {
        let files = vec![(
            "AGENTS.md".to_string(),
            "- Nunca subas secretos al repositorio.\n- Siempre escribe pruebas para los analizadores nuevos.\n".to_string(),
        ), (
            "CLAUDE.md".to_string(),
            "- 不要把密钥提交到仓库。\n- 始终为新的解析器编写测试。\n".to_string(),
        )];
        assert!(check(&files, |_| true).is_empty());
    }

    #[test]
    fn english_rules_inside_other_languages_still_count() {
        let rules = extract_rules(
            "AGENTS.md",
            "- Die Tests laufen mit cargo test im Ordner der Crate.\n- Never commit the generated files in the target folder.\n",
        );
        assert_eq!(rules[0].language, Language::De);
        assert_eq!(rules[1].polarity, Polarity::Dont);
    }

    #[test]
    fn unrelated_rules_are_fine() {
        let files = vec![(
            "AGENTS.md".to_string(),
            "- Never commit secrets to the repository.\n- Always write tests for new parsers.\n".to_string(),
        )];
        assert!(check(&files, |_| true).is_empty());
    }
}
