use lumen_language::Language;
use lumen_layout::BlockKind;
use lumen_translate::{
    protect, restore, CircuitBreaker, CircuitState, DialogueContext, FallbackEngine, Glossary,
    MemoryKey, StubTranslationEngine, TranslateItem, TranslationEngine, TranslationMemory,
    TranslationRequest,
};

#[test]
fn end_to_end_translation_pipeline_with_caching_and_protection() {
    let mut glossary = Glossary::new("game-app");
    glossary.add("Cyberdeck", "Кибердека", false);

    let mut memory = TranslationMemory::new(64);
    let mut engine = StubTranslationEngine::new();
    let mut context = DialogueContext::standard("game-app");

    context.push(Some("AI"), "System online.", Some("Система в сети."));

    let items = vec![
        TranslateItem {
            id: 1,
            text: "New Game".to_owned(),
            kind: Some(BlockKind::Button),
        },
        TranslateItem {
            id: 2,
            text: "Options".to_owned(),
            kind: Some(BlockKind::Button),
        },
        TranslateItem {
            id: 3,
            text: "Press [E] to equip Cyberdeck".to_owned(),
            kind: Some(BlockKind::Tooltip),
        },
        TranslateItem {
            id: 4,
            text: "Level 42".to_owned(),
            kind: Some(BlockKind::Label),
        },
    ];

    let request = TranslationRequest {
        items: items.clone(),
        source_language: Language::English,
        target_language: Language::Russian,
        context: Some(context.format_prompt_context()),
        app_id: Some("game-app".to_owned()),
    };

    // First pass: cache miss, engine translated
    let mut translated_first = Vec::new();
    for item in &request.items {
        let key = MemoryKey::new(
            &item.text,
            request.source_language,
            request.target_language,
            glossary.version,
        );
        let cached = memory.get(&key).map(|r| r.translation.clone());
        let result = match cached {
            Some(hit) => hit,
            None => {
                let dnt = glossary.do_not_translate();
                let (masked, tokens) = protect(&item.text, &dnt);
                let single_req = TranslationRequest {
                    items: vec![TranslateItem {
                        id: item.id,
                        text: masked,
                        kind: item.kind,
                    }],
                    source_language: request.source_language,
                    target_language: request.target_language,
                    context: request.context.clone(),
                    app_id: request.app_id.clone(),
                };
                let resp = engine.translate(&single_req).unwrap();
                let raw_trans = &resp.items[0].translated;
                let restored = restore(raw_trans, &tokens);
                let with_glossary = glossary.apply(&restored);
                memory.insert(key, with_glossary.clone(), "stub", 0.99);
                with_glossary
            }
        };
        translated_first.push(result);
    }

    assert_eq!(translated_first[0], "Новая игра");
    assert_eq!(translated_first[1], "Настройки");
    assert!(translated_first[2].contains("[E]"));
    assert!(translated_first[2].contains("Кибердека"));
    assert!(translated_first[3].contains("42"));
    assert_eq!(memory.stats().misses, 4);
    assert_eq!(memory.stats().inserts, 4);
    assert_eq!(memory.stats().hits, 0);

    // Second pass with identical input: 100% cache hits, engine not called
    let initial_calls = engine.call_count();
    let mut translated_second = Vec::new();
    for item in &request.items {
        let key = MemoryKey::new(
            &item.text,
            request.source_language,
            request.target_language,
            glossary.version,
        );
        let hit = memory.get(&key).expect("must be in translation memory");
        translated_second.push(hit.translation.clone());
    }

    assert_eq!(translated_first, translated_second);
    assert_eq!(memory.stats().hits, 4);
    assert_eq!(engine.call_count(), initial_calls);

    // Glossary version change: cache misses because version changed
    glossary.add("Cyberdeck", "Дека", false);
    let key_updated = MemoryKey::new(
        &items[2].text,
        request.source_language,
        request.target_language,
        glossary.version,
    );
    assert!(memory.get(&key_updated).is_none());
}

#[test]
fn fallback_recovers_gracefully_under_circuit_open() {
    let primary = StubTranslationEngine::new();
    let secondary = StubTranslationEngine::new();
    let breaker = CircuitBreaker::standard();
    let mut fallback = FallbackEngine::new(primary, secondary, breaker);

    let req = TranslationRequest {
        items: vec![TranslateItem {
            id: 1,
            text: "Quit".to_owned(),
            kind: None,
        }],
        source_language: Language::English,
        target_language: Language::Russian,
        context: None,
        app_id: None,
    };

    let resp = fallback.translate(&req).unwrap();
    assert_eq!(resp.items[0].translated, "Выход");
    assert_eq!(fallback.circuit_breaker().state(), CircuitState::Closed);
}
