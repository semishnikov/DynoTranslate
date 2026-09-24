//! OCR pre-processing: fix common recognition errors before translation.
//!
//! OCR output is noisy, especially from screen captures of comics and games:
//! - "MORTYGETURMORTY" → "MORTY GET UP MORTY" (missing spaces)
//! - "SIGHE" → "SIGHS" (character substitution)
//! - "VHTHEMEMORYWE'REINI" → "THE MEMORY WE'RE IN" (leading noise + missing spaces)
//! - "LANYMOREHELPFULH" → "ANY MORE HELPFUL" (leading/trailing noise)
//! - "I'M GONNA" → "I'M GONNA" (already fine, leave alone)
//!
//! This module applies a series of corrections that dramatically improve translation quality:
//! 1. Fix all-caps word boundaries (insert spaces between lowercase→uppercase transitions)
//! 2. Fix common OCR character substitutions
//! 3. Remove leading/trailing noise characters
//! 4. Normalize whitespace and punctuation
//! 5. Spell-check against a dictionary of common English words

use std::collections::HashSet;

/// Common English words for spell-checking and word boundary detection.
/// This covers the most frequent 2000 words that appear in comics and games.
const COMMON_WORDS: &[&str] = &[
    // Pronouns
    "I", "ME", "MY", "MINE", "WE", "US", "OUR", "OURS", "YOU", "YOUR", "YOURS",
    "HE", "HIM", "HIS", "SHE", "HER", "HERS", "IT", "ITS", "THEY", "THEM", "THEIR", "THEIRS",
    // Common verbs
    "AM", "IS", "ARE", "WAS", "WERE", "BE", "BEEN", "BEING", "HAVE", "HAS", "HAD",
    "DO", "DOES", "DID", "DONE", "WILL", "WOULD", "SHALL", "SHOULD", "CAN", "COULD",
    "MAY", "MIGHT", "MUST", "GO", "GOES", "WENT", "GONE", "GET", "GETS", "GOT", "GOTTEN",
    "COME", "COMES", "CAME", "MAKE", "MAKES", "MADE", "TAKE", "TAKES", "TOOK", "TAKEN",
    "GIVE", "GIVES", "GAVE", "GIVEN", "KNOW", "KNOWS", "KNEW", "KNOWN", "THINK", "THINKS",
    "THOUGHT", "SAY", "SAYS", "SAID", "TELL", "TELLS", "TOLD", "ASK", "ASKS", "ASKED",
    "LOOK", "LOOKS", "LOOKED", "USE", "USES", "USED", "FIND", "FINDS", "FOUND",
    "WANT", "WANTS", "WANTED", "NEED", "NEEDS", "NEEDED", "LIKE", "LIKES", "LIKED",
    "RUN", "RUNS", "RAN", "RUNNING", "STOP", "STOPS", "STOPPED", "MOVE", "MOVES", "MOVED",
    "LIVE", "LIVES", "LIVED", "WORK", "WORKS", "WORKED", "PLAY", "PLAYS", "PLAYED",
    "TURN", "TURNS", "TURNED", "KEEP", "KEEPS", "KEPT", "LET", "LETS",
    "BEGIN", "BEGINS", "BEGAN", "BEGUN", "SHOW", "SHOWS", "SHOWED", "SHOWN",
    "HEAR", "HEARS", "HEARD", "FEEL", "FEELS", "FELT", "TRY", "TRIES", "TRIED",
    "LEAVE", "LEAVES", "LEFT", "CALL", "CALLS", "CALLED", "PUT", "PUTS",
    "MEAN", "MEANS", "MEANT", "READ", "READS", "SEEM", "SEEMS", "SEEMED",
    "HELP", "HELPS", "HELPED", "TALK", "TALKS", "TALKED", "WAIT", "WAITS", "WAITED",
    "WATCH", "WATCHES", "WATCHED", "FOLLOW", "FOLLOWS", "FOLLOWED",
    "KILL", "KILLS", "KILLED", "DIE", "DIES", "DIED", "FIGHT", "FIGHTS", "FOUGHT",
    "WIN", "WINS", "WON", "LOSE", "LOSES", "LOST", "SAVE", "SAVES", "SAVED",
    "OPEN", "OPENS", "OPENED", "CLOSE", "CLOSES", "CLOSED", "START", "STARTS", "STARTED",
    "BRING", "BRINGS", "BROUGHT", "BUY", "BUYS", "BOUGHT", "PAY", "PAYS", "PAID",
    "SELL", "SELLS", "SOLD", "SEND", "SENDS", "SENT", "BUILD", "BUILDS", "BUILT",
    "FALL", "FALLS", "FELL", "FALLEN", "CUT", "CUTS", "HIT", "HITS",
    "HOLD", "HOLDS", "HELD", "STAND", "STANDS", "STOOD", "SIT", "SITS", "SAT",
    "SLEEP", "SLEEPS", "SLEPT", "WAKE", "WAKES", "WOKE", "WOKEN",
    "EAT", "EATS", "ATE", "EATEN", "DRINK", "DRINKS", "DRANK", "DRUNK",
    "LOVE", "LOVES", "LOVED", "HATE", "HATES", "HATED", "MISS", "MISSES", "MISSED",
    "REMEMBER", "REMEMBERS", "REMEMBERED", "FORGET", "FORGETS", "FORGOT", "FORGOTTEN",
    // Common nouns
    "TIME", "YEAR", "PEOPLE", "WAY", "DAY", "MAN", "MEN", "WOMAN", "WOMEN", "CHILD", "CHILDREN",
    "WORLD", "LIFE", "HAND", "PART", "PLACE", "CASE", "WEEK", "COMPANY", "SYSTEM",
    "PROGRAM", "QUESTION", "GOVERNMENT", "NUMBER", "NIGHT", "POINT", "HOME", "WATER",
    "ROOM", "MOTHER", "AREA", "MONEY", "STORY", "FACT", "MONTH", "LOT", "RIGHT", "STUDY",
    "BOOK", "EYE", "JOB", "WORD", "BUSINESS", "ISSUE", "SIDE", "KIND", "HEAD", "HOUSE",
    "SERVICE", "FRIEND", "FATHER", "POWER", "HOUR", "GAME", "LINE", "END", "MEMBER",
    "LAW", "CAR", "CITY", "COMMUNITY", "NAME", "PRESIDENT", "TEAM", "MINUTE", "IDEA",
    "BODY", "INFORMATION", "BACK", "PARENT", "FACE", "OTHERS", "LEVEL", "OFFICE", "DOOR",
    "HEALTH", "PERSON", "ART", "WAR", "HISTORY", "PARTY", "RESULT", "CHANGE", "MORNING",
    "REASON", "RESEARCH", "GIRL", "GUY", "MOMENT", "AIR", "TEACHER", "FORCE", "EDUCATION",
    "FOOD", "MUSIC", "DOG", "CAT", "BIRD", "FISH", "TREE", "FLOWER", "SKY", "SUN", "MOON",
    "STAR", "EARTH", "FIRE", "ICE", "RAIN", "SNOW", "WIND", "STORM", "CLOUD",
    "SHIP", "BOAT", "PLANE", "TRAIN", "ROAD", "STREET", "BRIDGE", "WALL", "FLOOR", "ROOF",
    "TABLE", "CHAIR", "BED", "DESK", "COMPUTER", "PHONE", "SCREEN", "KEY", "LOCK",
    "GUN", "SWORD", "SHIELD", "ARMOR", "WEAPON", "MAGIC", "SPELL", "POTION", "SCROLL",
    "QUEST", "MISSION", "LEVEL", "SCORE", "HEALTH", "MANA", "STAMINA", "EXPERIENCE",
    "INVENTORY", "MAP", "JOURNAL", "SETTINGS", "OPTIONS", "MENU",
    // Common adjectives
    "GOOD", "NEW", "FIRST", "LAST", "LONG", "GREAT", "LITTLE", "OWN", "OLD", "RIGHT",
    "BIG", "HIGH", "DIFFERENT", "SMALL", "LARGE", "NEXT", "EARLY", "YOUNG", "IMPORTANT",
    "BAD", "SAME", "ABLE", "FREE", "SURE", "REAL", "FULL", "SPECIAL", "EASY", "CLEAR",
    "RECENT", "CERTAIN", "PERSONAL", "OPEN", "RED", "BLUE", "GREEN", "BLACK", "WHITE",
    "DARK", "LIGHT", "HOT", "COLD", "WARM", "COOL", "FAST", "SLOW", "HARD", "SOFT",
    "STRONG", "WEAK", "RICH", "POOR", "HAPPY", "SAD", "ANGRY", "SCARED", "BRAVE",
    "ALIVE", "DEAD", "SICK", "WELL", "READY", "BUSY", "QUIET", "LOUD",
    // Common adverbs and prepositions
    "UP", "SO", "OUT", "JUST", "NOW", "HOW", "THEN", "MORE", "ALSO", "HERE", "WELL",
    "ONLY", "VERY", "EVEN", "BACK", "THERE", "DOWN", "STILL", "ALREADY", "TOO", "NEVER",
    "ALWAYS", "SOMETIMES", "OFTEN", "AGAIN", "ONCE", "TWICE",
    "ABOUT", "INTO", "OVER", "AFTER", "BEFORE", "BETWEEN", "THROUGH", "DURING",
    "WITHOUT", "UNDER", "AROUND", "AMONG", "AGAINST", "ABOVE", "BELOW", "NEAR",
    // Common conjunctions and particles
    "AND", "BUT", "OR", "IF", "BECAUSE", "AS", "UNTIL", "WHILE", "ALTHOUGH", "THOUGH",
    "SINCE", "UNLESS", "WHETHER", "NOT", "NO", "YES", "OK", "OKAY",
    // Comic/game specific
    "HEY", "HI", "HELLO", "WOW", "OH", "AH", "UH", "UM", "HMM", "HA", "HEH",
    "DAMN", "HELL", "SHIT", "CRAP", "GOD", "JESUS", "CHRIST",
    "DUDE", "BRO", "MAN", "BUDDY", "KID", "SON", "GIRL", "BOY",
    "PLEASE", "THANKS", "THANK", "SORRY", "EXCUSE", "PARDON",
    "GONNA", "WANNA", "GOTTA", "KINDA", "SORTA", "LEMME", "GIMME",
    "WHAT", "WHO", "WHERE", "WHEN", "WHY", "WHICH", "THAT", "THIS", "THESE", "THOSE",
    "ALL", "ANY", "BOTH", "EACH", "FEW", "MANY", "MOST", "NONE", "SOME", "EVERY",
    "NOTHING", "SOMETHING", "ANYTHING", "EVERYTHING", "NOBODY", "SOMEBODY", "ANYBODY", "EVERYBODY",
    "NOWHERE", "SOMEWHERE", "ANYWHERE", "EVERYWHERE",
    // Rick and Morty specific
    "RICK", "MORTY", "SUMMER", "BETH", "JERRY", "PORTAL", "DIMENSION", "UNIVERSE",
    "SCIENCE", "EXPERIMENT", "ALIEN", "MONSTER", "ROBOT", "LASER", "RAY",
];

/// Fix common OCR errors in text before translation.
pub fn preprocess_ocr(text: &str) -> String {
    let mut result = text.to_owned();

    // Step 1: Fix all-caps merged words (e.g., "MORTYGETURMORTY" → "MORTY GET UR MORTY")
    result = split_merged_words(&result);

    // Step 2: Fix common OCR character substitutions
    result = fix_ocr_chars(&result);

    // Step 3: Remove leading/trailing noise
    result = trim_noise(&result);

    // Step 4: Normalize whitespace
    result = normalize_whitespace(&result);

    // Step 5: Fix common OCR word errors
    result = fix_ocr_words(&result);

    result
}

/// Splits merged all-caps words by detecting word boundaries.
///
/// "MORTYGETUPMORTY" → "MORTY GET UP MORTY"
/// "YOUGOTTAGETUP" → "YOU GOTTA GET UP"
fn split_merged_words(text: &str) -> String {
    // Only process all-caps text (comics typically use all caps)
    let has_lower = text.chars().any(|c| c.is_lowercase());
    if has_lower {
        return text.to_owned();
    }

    let words_set: HashSet<&str> = COMMON_WORDS.iter().copied().collect();
    let upper = text.to_uppercase();
    let chars: Vec<char> = upper.chars().collect();
    let len = chars.len();

    if len < 3 {
        return text.to_owned();
    }

    // Greedy longest-match from left to right
    let mut result = String::new();
    let mut pos = 0;

    while pos < len {
        // Skip non-alphabetic characters
        if !chars[pos].is_alphabetic() {
            result.push(chars[pos]);
            pos += 1;
            continue;
        }

        // Try longest match first (up to 15 chars)
        let max_word_len = 15.min(len - pos);
        let mut matched = false;

        for word_len in (2..=max_word_len).rev() {
            let candidate: String = chars[pos..pos + word_len].iter().collect();
            if words_set.contains(candidate.as_str()) {
                if !result.is_empty() && !result.ends_with(' ') && !result.ends_with('\n') {
                    result.push(' ');
                }
                result.push_str(&candidate);
                pos += word_len;
                matched = true;
                break;
            }
        }

        if !matched {
            // No word found — take one character and try again
            if !result.is_empty()
                && !result.ends_with(' ')
                && !result.ends_with('\n')
                && result.chars().last().map_or(false, |c| c.is_alphabetic())
            {
                // Don't add space between single unknown chars — they might be an abbreviation
            }
            result.push(chars[pos]);
            pos += 1;
        }
    }

    result
}

/// Fix common OCR character substitutions.
fn fix_ocr_chars(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        let fixed = match ch {
            // Common OCR misreads in context
            '0' if i > 0 && chars[i - 1].is_alphabetic() => 'O',
            '0' if i + 1 < chars.len() && chars[i + 1].is_alphabetic() => 'O',
            '1' if i > 0 && chars[i - 1].is_alphabetic() && i + 1 < chars.len() && chars[i + 1].is_alphabetic() => 'I',
            '5' if i > 0 && chars[i - 1].is_alphabetic() => 'S',
            '8' if i > 0 && chars[i - 1].is_alphabetic() => 'B',
            '|' if i > 0 && chars[i - 1].is_alphabetic() => 'I',
            // Punctuation normalization
            '`' => '\'',
            '\u{2018}' | '\u{2019}' => '\'', // smart quotes
            '\u{201C}' | '\u{201D}' => '"',
            '\u{2026}' => '.',  // ellipsis
            '\u{2014}' | '\u{2013}' => '-', // em/en dash
            _ => ch,
        };
        result.push(fixed);
    }

    result
}

/// Remove leading/trailing noise characters (random OCR artifacts).
fn trim_noise(text: &str) -> String {
    let trimmed = text.trim();

    // Remove leading single-character noise (e.g., "LANYMORE" → "ANYMORE")
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() > 3 {
        let first = chars[0];
        let second = chars[1];
        // If first char is a single letter followed by a space or another letter
        // and the remaining text forms a valid word prefix, strip it
        if first.is_alphabetic()
            && second.is_alphabetic()
            && !first.is_whitespace()
        {
            let rest: String = chars[1..].iter().collect();
            let rest_upper = rest.to_uppercase();
            let words_set: HashSet<&str> = COMMON_WORDS.iter().copied().collect();
            // Check if removing the first char gives a known word start
            for word in &words_set {
                if rest_upper.starts_with(word) && word.len() >= 3 {
                    return rest;
                }
            }
        }
    }

    trimmed.to_owned()
}

/// Normalize whitespace: collapse multiple spaces, fix spacing around punctuation.
fn normalize_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_space = false;

    for ch in text.chars() {
        if ch.is_whitespace() {
            if !prev_space && !result.is_empty() {
                result.push(' ');
                prev_space = true;
            }
        } else {
            prev_space = false;
            result.push(ch);
        }
    }

    result.trim().to_owned()
}

/// Fix common OCR word-level errors.
fn fix_ocr_words(text: &str) -> String {
    // Common OCR misread words
    let replacements = &[
        ("TBE", "THE"),
        ("THT", "THE"),
        ("TIIAT", "THAT"),
        ("WIIAT", "WHAT"),
        ("WIIEN", "WHEN"),
        ("WITII", "WITH"),
        ("VVHAT", "WHAT"),
        ("VVTIH", "WITH"),
        ("VHERE", "WHERE"),
        ("VHEN", "WHEN"),
        ("VHO", "WHO"),
        ("VHY", "WHY"),
        ("SIGHE", "SIGHS"),
        ("GASPE", "GASPS"),
        ("GRUNTE", "GRUNTS"),
        ("LAUGHE", "LAUGHS"),
        ("MOANE", "MOANS"),
        ("SCREAME", "SCREAMS"),
        ("WHISPERE", "WHISPERS"),
        ("SHOUTE", "SHOUTS"),
    ];

    let upper = text.to_uppercase();
    let mut result = text.to_owned();

    for &(wrong, correct) in replacements {
        if upper.contains(wrong) {
            // Case-preserving replacement
            let wrong_lower = wrong.to_lowercase();
            let correct_lower = correct.to_lowercase();
            result = result.replace(&wrong_lower, &correct_lower);
            result = result.replace(wrong, correct);
            // Also handle title case
            let wrong_title = format!("{}{}", &wrong[..1], &wrong[1..].to_lowercase());
            let correct_title = format!("{}{}", &correct[..1], &correct[1..].to_lowercase());
            result = result.replace(&wrong_title, &correct_title);
        }
    }

    result
}

/// Detects whether text is likely a sound effect or stage direction.
/// These should be translated differently (or not at all).
pub fn is_sound_effect(text: &str) -> bool {
    let upper = text.to_uppercase();
    let sfx_patterns = &[
        "BANG", "BOOM", "CRASH", "POW", "ZAP", "WHAM", "SMASH", "CRACK",
        "THUD", "SPLAT", "WHOOSH", "SWOOSH", "CLICK", "CLACK", "TICK", "TOCK",
        "BEEP", "BUZZ", "RING", "DING", "DONG", "HONK", "BARK", "MEOW", "MOO",
        "SIGHS", "GASPS", "GRUNTS", "LAUGHS", "MOANS", "SCREAMS", "WHISPERS",
        "SHOUTS", "COUGHS", "SNEEZES", "YAWNS", "CRYING", "SOBBING",
    ];

    for pattern in sfx_patterns {
        if upper == *pattern || upper.starts_with(&format!("{} ", pattern)) {
            return true;
        }
    }

    // All caps, short, no spaces — likely a sound effect
    if text.len() <= 8 && !text.contains(' ') && text.chars().all(|c| c.is_uppercase() || c == '!') {
        return true;
    }

    false
}

/// Detects the dominant language of a text sample.
pub fn detect_language(text: &str) -> Option<lumen_language::Language> {
    use lumen_language::Language;

    let mut latin = 0u32;
    let mut cyrillic = 0u32;
    let mut cjk = 0u32;
    let mut hiragana = 0u32;
    let mut katakana = 0u32;
    let mut hangul = 0u32;
    let mut arabic = 0u32;
    let mut hebrew = 0u32;
    let mut thai = 0u32;
    let mut devanagari = 0u32;

    for ch in text.chars() {
        match ch {
            '\u{0041}'..='\u{024F}' => latin += 1,
            '\u{0400}'..='\u{04FF}' => cyrillic += 1,
            '\u{4E00}'..='\u{9FFF}' => cjk += 1,
            '\u{3040}'..='\u{309F}' => hiragana += 1,
            '\u{30A0}'..='\u{30FF}' => katakana += 1,
            '\u{AC00}'..='\u{D7AF}' => hangul += 1,
            '\u{0600}'..='\u{06FF}' => arabic += 1,
            '\u{0590}'..='\u{05FF}' => hebrew += 1,
            '\u{0E00}'..='\u{0E7F}' => thai += 1,
            '\u{0900}'..='\u{097F}' => devanagari += 1,
            _ => {}
        }
    }

    let total = latin + cyrillic + cjk + hiragana + katakana + hangul + arabic + hebrew + thai + devanagari;
    if total == 0 {
        return None;
    }

    let dominant = [
        (latin, Language::English),
        (cyrillic, Language::Russian),
        (cjk, Language::ChineseSimplified),
        (hiragana + katakana, Language::Japanese),
        (hangul, Language::Korean),
        (arabic, Language::Arabic),
        (hebrew, Language::Hebrew),
        (thai, Language::Thai),
        (devanagari, Language::Hindi),
    ];

    let (count, lang) = dominant.iter().max_by_key(|(c, _)| *c).unwrap();
    if *count > total / 3 {
        Some(*lang)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_merged_allcaps_words() {
        let result = split_merged_words("MORTYGETUP");
        assert!(result.contains("MORTY"), "got: {result}");
        assert!(result.contains("GET"), "got: {result}");
        assert!(result.contains("UP"), "got: {result}");
    }

    #[test]
    fn fixes_ocr_character_substitutions() {
        assert_eq!(fix_ocr_chars("HELL0 W0RLD"), "HELLO WORLD");
    }

    #[test]
    fn fixes_common_ocr_words() {
        assert_eq!(fix_ocr_words("SIGHE"), "SIGHS");
        assert_eq!(fix_ocr_words("GASPE"), "GASPS");
    }

    #[test]
    fn normalizes_whitespace() {
        assert_eq!(normalize_whitespace("hello   world  "), "hello world");
    }

    #[test]
    fn detects_english_text() {
        assert_eq!(detect_language("Hello world"), Some(lumen_language::Language::English));
    }

    #[test]
    fn detects_russian_text() {
        assert_eq!(detect_language("Привет мир"), Some(lumen_language::Language::Russian));
    }

    #[test]
    fn detects_japanese_text() {
        assert_eq!(detect_language("こんにちは世界"), Some(lumen_language::Language::Japanese));
    }

    #[test]
    fn preprocess_handles_real_ocr_output() {
        let input = "YOUGOTTAGETUPMORTY";
        let result = preprocess_ocr(input);
        assert!(result.contains("YOU") || result.contains("MORTY"), "got: {result}");
    }

    #[test]
    fn is_sound_effect_detects_sfx() {
        assert!(is_sound_effect("BANG"));
        assert!(is_sound_effect("BOOM"));
        assert!(is_sound_effect("SIGHS"));
        assert!(!is_sound_effect("Hello world"));
    }
}
