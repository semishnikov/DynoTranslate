//! What each supported language looks like on screen.
//!
//! Two vocabularies and one alphabet per language. The distinctive letters are the ones no other
//! supported language uses, so a single one of them is strong evidence. The function words are the
//! ones grammar forces into almost any sentence, and the menu words are the handful that appear on
//! nearly every screen of an application — a game menu says very little else.
//!
//! This is orthography, not a trained classifier. It is exact for the script and reliable for the
//! distinctive letters; where two languages share both, it settles for the one with more evidence
//! and says so through the confidence it returns.

use crate::Language;

/// The evidence a language leaves in a passage of text.
pub struct Cues {
    /// Letters only this language uses, within the supported set. Empty for languages that share
    /// their alphabet with the others entirely.
    pub distinctive: &'static str,
    /// Function words, space separated.
    pub words: &'static str,
    /// Words that appear on nearly every screen of an application, space separated.
    pub menu: &'static str,
}

/// The cues for one language.
pub fn cues(language: Language) -> Cues {
    match language {
        Language::English => Cues {
            distinctive: "",
            words: "the and of to in is you that it for with this not are have was will your from can all been they",
            menu: "play start continue load save settings options quit exit back cancel yes no apply resume",
        },
        Language::German => Cues {
            distinctive: "ßẞ",
            words: "der die das und ist nicht sie mit sich für ein eine im aber auch nur wenn wie hat bei oder",
            menu: "spielen starten fortsetzen laden speichern optionen beenden zurück weiter ja nein anwenden",
        },
        Language::French => Cues {
            distinctive: "œŒûÛ",
            words: "le la les des une un et est pour que dans vous ne pas sur avec ce sont être au aux du il elle",
            menu: "jouer démarrer continuer charger sauvegarder options quitter retour oui non appliquer",
        },
        Language::Spanish => Cues {
            distinctive: "ñÑ¿¡",
            words: "el la los las un una de que en es por con para no se su al lo como más pero sus este esta ser",
            menu: "jugar iniciar continuar cargar guardar opciones salir volver sí no aplicar",
        },
        Language::Italian => Cues {
            distinctive: "",
            words: "il lo la gli le un una di che è in con non si per sono questo questa tutto sì no anche più ma",
            menu: "gioca avvia continua carica salva opzioni esci indietro sì no applica riprendi",
        },
        Language::Portuguese => Cues {
            distinctive: "ãÃõÕ",
            words: "o a os as um uma de que e do da em para não com por seu sua mais como mas este esta são tudo",
            menu: "jogar iniciar continuar carregar salvar opções sair voltar sim não aplicar retomar",
        },
        Language::Dutch => Cues {
            distinctive: "",
            words: "de het een van en is op te die in voor met niet aan zijn dat maar om ook als geen meer al ze",
            menu: "spelen starten doorgaan laden opslaan opties afsluiten terug ja nee toepassen",
        },
        Language::Swedish => Cues {
            distinctive: "åÅ",
            words: "och att det är en för med som den till inte om men ett har vi de du alla eller från kan vill",
            menu: "spela starta fortsätt ladda spara alternativ avsluta tillbaka ja nej verkställ",
        },
        Language::Polish => Cues {
            distinctive: "ąćęłńśźżĄĆĘŁŃŚŹŻ",
            words: "i w na z do od się że co to jak za przez nie tak jest są ten ta gra zapisz wczytaj opcje",
            menu: "graj rozpocznij kontynuuj wczytaj zapisz opcje ustawienia wyjście wstecz dalej tak nie",
        },
        Language::Czech => Cues {
            distinctive: "čšžďňěřťůČŠŽĎŇĚŘŤŮ",
            words: "a v na je že co to jak za od pro ke ze se ne ano jsou ten tato hra uložit načíst možnosti",
            menu: "hrát spustit pokračovat načíst uložit možnosti nastavení ukončit zpět dále ano ne",
        },
        Language::Romanian => Cues {
            distinctive: "ășțĂȘȚ",
            words: "și în este de la cu pentru un o nu da joc încarcă opțiuni setări ieșire înapoi continuă",
            menu: "joacă pornește continuă încarcă salvează opțiuni setări ieșire înapoi da nu aplică",
        },
        Language::Hungarian => Cues {
            distinctive: "őűŐŰ",
            words: "és a az egy hogy nem igen játék betöltés beállítások kilépés vissza folytatás ez azt van mint",
            menu: "játék indítás folytatás betöltés mentés beállítások kilépés vissza igen nem alkalmaz",
        },
        Language::Turkish => Cues {
            distinctive: "ğışĞİŞ",
            words: "ve bir bu için ile daha çok gibi ama değil evet hayır oyun yükle seçenekler ayarlar çıkış",
            menu: "oyna başlat devam yükle kaydet seçenekler ayarlar çıkış geri evet hayır uygula",
        },
        Language::Russian => Cues {
            distinctive: "ыэёЫЭЁ",
            words: "и в не что на я с как а то все она так его но да ты к у же вы за по её мне нет для из уже",
            menu: "играть начать продолжить загрузить сохранить настройки выход назад да нет применить",
        },
        Language::Ukrainian => Cues {
            distinctive: "їєґЇЄҐ",
            words: "і в не що на я з як а то всі вона так його але да ти до у же ви за по тільки її мені немає",
            menu: "грати почати продовжити завантажити зберегти налаштування вихід назад так ні",
        },
        Language::Belarusian => Cues {
            distinctive: "ўЎ",
            words: "і ў не што на я з як а то ўсе яна так яго але ды ты да у ж вы за па толькі яе мне няма",
            menu: "гуляць пачаць працягнуць загрузіць захаваць налады выхад назад так не",
        },
        Language::Bulgarian => Cues {
            distinctive: "",
            words: "и в не че на аз с като а то всички тя така но да ти към у вече вие за по само мен няма от",
            menu: "играй започни продължи зареди запази настройки изход назад да не приложи",
        },
        Language::Serbian => Cues {
            distinctive: "ђјљњћџЂЈЉЊЋЏ",
            words: "и у не шта на ја са као а то сви она али да ти ка већ ви за по само њен мени нема од",
            menu: "играј започни настави учитај сачувај подешавања излаз назад да не примени",
        },
        Language::Greek => Cues {
            distinctive: "",
            words: "και το στο δεν για με από είναι ότι αυτός που αλλά ναι όχι παιχνίδι φόρτωση ρυθμίσεις",
            menu: "παίξε έναρξη συνέχεια φόρτωση αποθήκευση ρυθμίσεις έξοδος πίσω ναι όχι εφαρμογή",
        },
        Language::Japanese => Cues {
            distinctive: "",
            words: "は を に が と の です ます から まで する した ない ある この その これ それ",
            menu: "スタート つづきから セーブ ロード せってい おわる もどる はい いいえ",
        },
        Language::ChineseSimplified => Cues {
            distinctive: "国学电开门时来个们这说后发现实话与马鸟长东车风",
            words: "的 了 是 在 和 我 你 他 她 不 有 这 那 就 也 都 而 及 与 或 我们 你们 没有",
            menu: "开始 继续 读取 存档 设置 退出 返回 是 否 应用",
        },
        Language::ChineseTraditional => Cues {
            distinctive: "國學電開門時來個們這說後發現實話與馬鳥長東車風",
            words: "的 了 是 在 和 我 你 他 她 不 有 這 那 就 也 都 而 及 與 或 我們 你們 沒有",
            menu: "開始 繼續 讀取 存檔 設定 離開 返回 是 否 套用",
        },
        Language::Korean => Cues {
            distinctive: "",
            words: "은 는 이 가 을 를 과 와 의 에 에서 로 도 만 그리고 하지만 아니 예 게임 설정 종료",
            menu: "시작 계속 불러오기 저장 설정 종료 뒤로 예 아니오 적용",
        },
        Language::Arabic => Cues {
            distinctive: "",
            words: "و في من على إلى أن هذا التي هو لا ما مع عن كل قد بين ذلك لعبة تحميل إعدادات خروج",
            menu: "ابدأ تابع تحميل حفظ إعدادات خروج رجوع نعم لا تطبيق",
        },
        Language::Hebrew => Cues {
            distinctive: "",
            words: "ו של את על עם לא הוא זה כי אם אבל כן יש עבור כל משחק טעינה הגדרות יציאה חזרה",
            menu: "התחל המשך טעינה שמירה הגדרות יציאה חזרה כן לא החל",
        },
        Language::Thai => Cues {
            distinctive: "",
            words: "และ ที่ ของ ไม่ ใน เป็น มี นี้ ก็ จะ ให้ กับ เกม โหลด ตั้งค่า ออก กลับ ต่อ",
            menu: "เริ่ม เล่น ต่อ โหลด บันทึก ตั้งค่า ออก กลับ ใช่ ไม่ใช่",
        },
        Language::Hindi => Cues {
            distinctive: "",
            words: "और का की के है में को से यह वह नहीं हैं खेल लोड सेटिंग बाहर वापस जारी",
            menu: "शुरू जारी लोड सहेजें सेटिंग बाहर वापस हाँ नहीं लागू",
        },
        Language::Unknown => Cues {
            distinctive: "",
            words: "",
            menu: "",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn languages() -> impl Iterator<Item = Language> {
        Language::all()
            .iter()
            .copied()
            .filter(|language| *language != Language::Unknown)
    }

    #[test]
    fn every_language_has_some_evidence_to_offer() {
        for language in languages() {
            let cues = cues(language);
            let has_evidence = !cues.distinctive.is_empty() || !cues.words.is_empty();
            assert!(has_evidence, "{language:?} has no cues at all");
        }
    }

    #[test]
    fn the_distinctive_letters_are_actually_distinct() {
        for left in languages() {
            for right in languages() {
                if left == right {
                    continue;
                }
                let mine = cues(left).distinctive;
                let theirs = cues(right).distinctive;
                let shared: Vec<char> = mine.chars().filter(|c| theirs.contains(*c)).collect();
                assert!(shared.is_empty(), "{left:?} and {right:?} share {shared:?}");
            }
        }
    }

    #[test]
    fn vocabularies_are_lowercase_and_space_separated() {
        for language in languages() {
            let cues = cues(language);
            for vocabulary in [cues.words, cues.menu] {
                assert!(!vocabulary.starts_with(' '), "{language:?}");
                assert!(!vocabulary.ends_with(' '), "{language:?}");
                assert!(!vocabulary.contains("  "), "{language:?}");
                assert_eq!(vocabulary, vocabulary.to_lowercase(), "{language:?}");
            }
        }
    }
}
