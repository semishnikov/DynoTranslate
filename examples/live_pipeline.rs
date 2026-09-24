//! Example: Live translation pipeline — works out of the box, no API key needed.
//!
//! Demonstrates the production pipeline with the free Google Translate engine.
//! Run: cargo run --example live_pipeline

use lumen_core::Rect;
use lumen_stability::Observation;
use lumen_translate::{LiveConfig, LivePipeline, GoogleFreeEngine};

fn main() {
    println!("=== DynoTranslate Live Pipeline Demo ===\n");
    println!("Using FREE Google Translate (no API key needed)\n");

    // Configure the pipeline
    let config = LiveConfig {
        source_language: lumen_language::Language::English,
        target_language: lumen_language::Language::Russian,
        memory_capacity: 1000,
        use_context: true,
        ..Default::default()
    };

    let mut pipeline = LivePipeline::new(config);
    let mut engine = GoogleFreeEngine::new();

    // Frame 1: Two new text blocks appear
    println!("Frame 1: Translating new blocks...");
    let observations_1 = vec![
        Observation {
            rect: Rect::new(100, 50, 200, 30),
            text: "New Game".to_owned(),
            confidence: 0.95,
        },
        Observation {
            rect: Rect::new(100, 100, 200, 30),
            text: "Options".to_owned(),
            confidence: 0.92,
        },
    ];

    let blocks = pipeline.process_frame(&observations_1, &mut engine);
    println!("  Blocks: {}", blocks.len());
    for block in &blocks {
        println!(
            "    '{}' → '{}'",
            block.source_text,
            block.display_text,
        );
    }
    println!("  Frame time: {}ms", pipeline.stats().last_frame_ms);
    println!("  Cache hits: {}", pipeline.stats().cache_hits);
    println!("  Engine calls: {}\n", pipeline.stats().engine_calls);

    // Frame 2: Same blocks (should use cache)
    println!("Frame 2: Same blocks (should use cache)...");
    let blocks = pipeline.process_frame(&observations_1, &mut engine);
    println!("  Blocks: {}", blocks.len());
    for block in &blocks {
        println!("    '{}' → '{}'", block.source_text, block.display_text);
    }
    println!("  Frame time: {}ms (should be ~0ms — cached!)", pipeline.stats().last_frame_ms);
    println!("  Cache hits: {}", pipeline.stats().cache_hits);
    println!("  Engine calls: {} (should not increase)\n", pipeline.stats().engine_calls);

    // Frame 3: New block
    println!("Frame 3: New block appears...");
    let observations_3 = vec![
        Observation {
            rect: Rect::new(100, 50, 200, 30),
            text: "New Game".to_owned(),
            confidence: 0.95,
        },
        Observation {
            rect: Rect::new(100, 100, 200, 30),
            text: "Options".to_owned(),
            confidence: 0.92,
        },
        Observation {
            rect: Rect::new(100, 150, 200, 30),
            text: "Quit Game".to_owned(),
            confidence: 0.96,
        },
    ];

    let blocks = pipeline.process_frame(&observations_3, &mut engine);
    println!("  Blocks: {}", blocks.len());
    for block in &blocks {
        println!("    '{}' → '{}'", block.source_text, block.display_text);
    }
    println!("  Frame time: {}ms", pipeline.stats().last_frame_ms);

    println!("\n=== Pipeline Complete ===");
    println!("Total frames: {}", pipeline.stats().frames);
    println!("Total engine calls: {}", pipeline.stats().engine_calls);
    println!("Total cache hits: {}", pipeline.stats().cache_hits);
    println!("Total items translated: {}", pipeline.stats().items_translated);
    
    let total_lookups = pipeline.stats().cache_hits + pipeline.stats().cache_misses;
    if total_lookups > 0 {
        println!(
            "Cache hit rate: {:.1}%",
            100.0 * pipeline.stats().cache_hits as f64 / total_lookups as f64
        );
    }
}
