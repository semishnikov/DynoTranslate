//! What the corpus says.
//!
//! The phrases are the words an interface actually shows: menu entries, buttons, short dialogue.
//! They are written per language and deliberately lean on the vocabulary language identification
//! scores against, so the corpus measures recognition and identification on the same material —
//! the shape of text a real screen produces, not prose chosen to flatter either stage.
//!
//! The corpus covers the Latin and Cyrillic scripts today, which is what the stroke font in
//! [`crate::font`] draws. Greek, Han, kana, hangul, Arabic, Hebrew, Thai and Devanagari need
//! skeletons of their own before their languages can join; until then they stay out rather than
//! render as blanks, and [`crate::font::covers`] is what enforces that.

use lumen_language::Language;

/// The corpus material for one language.
pub struct LanguagePack {
    pub language: Language,
    pub phrases: &'static [&'static str],
}

const ENGLISH: LanguagePack = LanguagePack {
    language: Language::English,
    phrases: &[
        "Start game",
        "Continue",
        "Load last save",
        "Settings",
        "Do you want to quit?",
        "Press any key to begin",
        "The journey starts here",
        "Save and exit",
    ],
};

const GERMAN: LanguagePack = LanguagePack {
    language: Language::German,
    phrases: &[
        "Spiel starten",
        "Fortsetzen",
        "Optionen",
        "Wirklich beenden?",
        "Straße der Ehre",
        "Zurück zum Menü",
        "Speichern und verlassen",
        "Größe der Welt",
    ],
};

const FRENCH: LanguagePack = LanguagePack {
    language: Language::French,
    phrases: &[
        "Jouer",
        "Continuer",
        "Options",
        "Quitter le jeu ?",
        "Le cœur du héros",
        "Une nouvelle partie",
        "Sauvegarder et quitter",
        "Où mène ce chemin ?",
    ],
};

const SPANISH: LanguagePack = LanguagePack {
    language: Language::Spanish,
    phrases: &[
        "Jugar",
        "Continuar",
        "Opciones",
        "¿Salir del juego?",
        "La misión comienza",
        "El niño y el dragón",
        "Guardar partida",
        "Mañana otra vez",
    ],
};

const ITALIAN: LanguagePack = LanguagePack {
    language: Language::Italian,
    phrases: &[
        "Gioca",
        "Continua",
        "Opzioni",
        "Vuoi davvero uscire?",
        "La battaglia inizia",
        "Il viaggio continua",
        "Dov'è il salvataggio?",
        "Che il coraggio sia con te",
    ],
};

const PORTUGUESE: LanguagePack = LanguagePack {
    language: Language::Portuguese,
    phrases: &[
        "Jogar",
        "Continuar",
        "Opções",
        "Não quero sair",
        "Coração valente",
        "A missão não acabou",
        "Salve o jogo",
        "Até amanhã",
    ],
};

const POLISH: LanguagePack = LanguagePack {
    language: Language::Polish,
    phrases: &[
        "Graj",
        "Kontynuuj",
        "Opcje",
        "Czy na pewno wyjść?",
        "Łuk i strzała",
        "Zapisz grę",
        "Życie jest piękne",
        "Świt nadchodzi",
    ],
};

const CZECH: LanguagePack = LanguagePack {
    language: Language::Czech,
    phrases: &[
        "Hrát",
        "Pokračovat",
        "Možnosti",
        "Opravdu skončit?",
        "Příběh právě začíná",
        "Uložit hru",
        "Čeká tě dobrodružství",
        "Na shledanou",
    ],
};

const DUTCH: LanguagePack = LanguagePack {
    language: Language::Dutch,
    phrases: &[
        "Spelen",
        "Doorgaan",
        "Opties",
        "Wil je echt stoppen?",
        "Het avontuur begint",
        "Opslaan en afsluiten",
        "De reis gaat verder",
    ],
};

const RUSSIAN: LanguagePack = LanguagePack {
    language: Language::Russian,
    phrases: &[
        "Начать игру",
        "Продолжить",
        "Настройки",
        "Сохранить прогресс",
        "Вы действительно хотите выйти?",
        "Нажмите любую кнопку",
        "Начнём сначала",
        "Игра будет сохранена",
    ],
};

const UKRAINIAN: LanguagePack = LanguagePack {
    language: Language::Ukrainian,
    phrases: &[
        "Почати гру",
        "Продовжити",
        "Налаштування",
        "Зберегти прогрес",
        "Ви справді хочете вийти?",
        "Натисніть будь-яку клавішу",
        "Їжа та напої",
        "Це єдиний шлях",
    ],
};

static PACKS: &[LanguagePack] = &[
    ENGLISH,
    GERMAN,
    FRENCH,
    SPANISH,
    ITALIAN,
    PORTUGUESE,
    POLISH,
    CZECH,
    DUTCH,
    RUSSIAN,
    UKRAINIAN,
];

/// Every language pack the corpus draws from, in a fixed order.
pub fn packs() -> &'static [LanguagePack] {
    PACKS
}

/// The phrases of one language, or nothing when the corpus does not cover it.
pub fn phrases_for(language: Language) -> Option<&'static [&'static str]> {
    packs().iter().find(|pack| pack.language == language).map(|pack| pack.phrases)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font;
    use lumen_language::{script::dominant_script, Language as Identified, Script, MIN_CHARACTERS};

    #[test]
    fn every_phrase_can_actually_be_drawn() {
        for pack in packs() {
            for phrase in pack.phrases {
                assert_eq!(font::covers(phrase), None, "{phrase} uses a glyph the font lacks");
            }
        }
    }

    #[test]
    fn every_pack_is_worth_scoring() {
        let mut seen: Vec<Identified> = Vec::new();
        for pack in packs() {
            assert!(pack.phrases.len() >= 4, "{:?} has too few phrases", pack.language);
            assert!(!seen.contains(&pack.language), "{:?} appears twice", pack.language);
            seen.push(pack.language);
            for phrase in pack.phrases {
                let letters = phrase
                    .chars()
                    .filter(|c| !c.is_whitespace() && *c != '?' && *c != '!')
                    .count();
                assert!(letters >= MIN_CHARACTERS, "{phrase} is too short to identify");
                assert!(phrase.chars().count() <= 40, "{phrase} is longer than a UI line");
            }
            let unique: std::collections::HashSet<&str> = pack.phrases.iter().copied().collect();
            assert_eq!(
                unique.len(),
                pack.phrases.len(),
                "{:?} repeats a phrase",
                pack.language
            );
        }
    }

    #[test]
    fn every_pack_is_written_in_a_script_the_corpus_covers() {
        for pack in packs() {
            for phrase in pack.phrases {
                let script = dominant_script(phrase);
                assert!(
                    script == Some(Script::Latin) || script == Some(Script::Cyrillic),
                    "{phrase} is not Latin or Cyrillic"
                );
            }
        }
    }

    #[test]
    fn the_packs_feed_both_scripts() {
        let cyrillic = packs()
            .iter()
            .filter(|pack| dominant_script(pack.phrases[0]) == Some(Script::Cyrillic))
            .count();
        assert!(cyrillic >= 2, "the corpus should carry more than one Cyrillic language");
        let latin = packs().len() - cyrillic;
        assert!(latin >= 6, "the corpus should carry the Latin range too");
    }

    #[test]
    fn lookup_by_language_follows_the_packs() {
        assert_eq!(phrases_for(Identified::Russian), Some(RUSSIAN.phrases));
        assert_eq!(phrases_for(Identified::Japanese), None);
        assert_eq!(phrases_for(Identified::Unknown), None);
    }
}
