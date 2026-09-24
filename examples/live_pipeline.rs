//! Example: Live translation pipeline with temporal stability and caching.
//!
//! Demonstrates the production pipeline processing observations across frames,
//! showing how caching, stability, and batch translation work together.

use lumen_core::Rect;
use lumen_stability::Observation;
use lumen_translate::{LiveConfig, LivePipeline, StubTranslationEngine};

fn main() {
    println!("=== DynoTranslate Live Pipeline Demo ===\n");

    // Configure the pipeline
    let config = LiveConfig {
        source_language: lumen_language::Language::English,
        target_language: lumen_language::Language::Russian,
        memory_capacity: 1000,
        use_context: true,
        ..Default::default()
    };

    let mut pipeline = LivePipeline::new(config);
    let mut engine = StubTranslationEngine::new();

    // Frame 1: Two new text blocks appear
    println!("Frame 1: New blocks appear");
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
            "    [{}] '{}' → '{}' (new: {})",
            format_rect(&block.rect),
            block.source_text,
            block.display_text,
            block.is_new
        );
    }
    println!("  Stats: {:?}", pipeline.stats());
    println!("  Engine calls: {}\n", engine.call_count());

    // Frame 2: Same blocks, slight position drift (OCR noise)
    println!("Frame 2: Same blocks with OCR drift");
    let observations_2 = vec![
        Observation {
            rect: Rect::new(101, 51, 200, 30),  // 1px drift
            text: "New Game".to_owned(),
            confidence: 0.94,
        },
        Observation {
            rect: Rect::new(99, 101, 200, 30),  // 1px drift
            text: "Options".to_owned(),
            confidence: 0.91,
        },
    ];

    let blocks = pipeline.process_frame(&observations_2, &mut engine);
    println!("  Blocks: {}", blocks.len());
    for block in &blocks {
        println!(
            "    [{}] '{}' → '{}' (new: {})",
            format_rect(&block.display_rect),
            block.source_text,
            block.display_text,
            block.is_new
        );
    }
    println!("  Stats: {:?}", pipeline.stats());
    println!("  Engine calls: {} (no new calls — cache hit!)\n", engine.call_count());

    // Frame 3: OCR error in text (fuzzy cache should handle it)
    println!("Frame 3: OCR error in text");
    let observations_3 = vec![
        Observation {
            rect: Rect::new(100, 50, 200, 30),
            text: "New Gane".to_owned(),  // OCR error: 'm' → 'n'
            confidence: 0.88,
        },
        Observation {
            rect: Rect::new(100, 100, 200, 30),
            text: "Options".to_owned(),
            confidence: 0.93,
        },
    ];

    let blocks = pipeline.process_frame(&observations_3, &mut engine);
    println!("  Blocks: {}", blocks.len());
    for block in &blocks {
        println!(
            "    [{}] '{}' → '{}'",
            format_rect(&block.display_rect),
            block.source_text,
            block.display_text
        );
    }
    println!("  Stats: {:?}", pipeline.stats());
    println!("  Engine calls: {}\n", engine.call_count());

    // Frame 4: New block appears
    println!("Frame 4: New block appears");
    let observations_4 = vec![
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
            text: "Quit".to_owned(),
            confidence: 0.96,
        },
    ];

    let blocks = pipeline.process_frame(&observations_4, &mut engine);
    println!("  Blocks: {}", blocks.len());
    for block in &blocks {
        println!(
            "    [{}] '{}' → '{}' (new: {})",
            format_rect(&block.display_rect),
            block.source_text,
            block.display_text,
            block.is_new
        );
    }
    println!("  Stats: {:?}", pipeline.stats());
    println!("  Engine calls: {}\n", engine.call_count());

    // Frame 5: Block disappears
    println!("Frame 5: 'Quit' block disappears");
    let observations_5 = vec![
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

    let blocks = pipeline.process_frame(&observations_5, &mut engine);
    println!("  Blocks: {} (Quit still tracked — will be forgotten after {} misses)", 
             blocks.len(), 
             config.stability.forget_misses);
    for block in &blocks {
        println!(
            "    [{}] '{}' → '{}'",
            format_rect(&block.display_rect),
            block.source_text,
            block.display_text
        );
    }

    println!("\n=== Pipeline Complete ===");
    println!("Total frames: {}", pipeline.stats().frames);
    println!("Total engine calls: {}", engine.call_count());
    println!("Cache hit rate: {:.1}%", 
             if pipeline.stats().cache_hits + pipeline.stats().cache_misses > 0 {
                 100.0 * pipeline.stats().cache_hits as f64 
                     / (pipeline.stats().cache_hits + pipeline.stats().cache_misses) as f64
             } else {
                 0.0
             });
}

fn format_rect(rect: &Rect) -> String {
    format!("{}x{} @ ({},{})", rect.width, rect.height, rect.x, rect.y)
}
