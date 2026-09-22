//! The chaos run: the same pipeline under scripted failures, asserting fail-open.
//!
//! The plan's safety promise is "any pipeline error removes the overlay and leaves the host
//! application untouched". This run makes that promise testable: a deterministic fault plan
//! injects recognition errors, window loss and translation outages into the live pass, and the
//! report says which frames failed how. The overlay must clear exactly when the window is lost,
//! survive every other failure, and the process must never panic.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use lumen_capture::synthetic::SyntheticSource;
use lumen_capture::CaptureSource;
use lumen_ocr::OcrError;
use lumen_source::{ReadRequest, SourceError, SourceKind, TextRun, TextSource};
use lumen_translate::{
    EngineKind, StubTranslationEngine, TranslationEngine, TranslationError, TranslationRequest,
    TranslationResponse,
};
use serde::Serialize;

use crate::rng::SplitMix64;
use crate::run::{PassConfig, PassParams, PassRunner, RunError};
use crate::source::SceneTextSource;
use crate::Options;

/// Where the failures land. Fixed strides rather than probabilities: a run with a seed must
/// reproduce the same fault sequence on every machine, so the report is comparable.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FaultPlan {
    /// Every Nth read fails with a transient recognition error.
    pub(crate) every_read_error: usize,
    /// Every Nth read reports the window as gone, which must clear the overlay.
    pub(crate) every_target_lost: usize,
    /// Every Nth translation request hits a network outage.
    pub(crate) every_engine_error: usize,
}

impl Default for FaultPlan {
    fn default() -> Self {
        // Sized against the reads a run actually performs. A 400-frame chaos scene holds
        // about thirty blocks, each appearing and usually disappearing, so the run reads a
        // few dozen times and every fault lands several times. Strides near or past the
        // read count would leave faults unfired, and the integration test would assert
        // against failures that never happened.
        Self {
            every_read_error: 7,
            every_target_lost: 13,
            every_engine_error: 3,
        }
    }
}

/// Counters the wrappers write and the report reads, so the fault injection can be observed from
/// outside the trait objects.
#[derive(Debug, Clone)]
pub(crate) struct FaultCounters {
    pub(crate) read_errors: Arc<AtomicUsize>,
    pub(crate) target_lost: Arc<AtomicUsize>,
    pub(crate) engine_errors: Arc<AtomicUsize>,
}

impl FaultCounters {
    fn new() -> Self {
        Self {
            read_errors: Arc::new(AtomicUsize::new(0)),
            target_lost: Arc::new(AtomicUsize::new(0)),
            engine_errors: Arc::new(AtomicUsize::new(0)),
        }
    }
}

struct FaultySource {
    inner: Box<dyn TextSource>,
    plan: FaultPlan,
    reads: usize,
    counters: FaultCounters,
}

impl TextSource for FaultySource {
    fn name(&self) -> &str {
        "fault-injection"
    }

    fn kind(&self) -> SourceKind {
        self.inner.kind()
    }

    fn read(&mut self, request: &ReadRequest<'_>) -> Result<Vec<TextRun>, SourceError> {
        self.reads += 1;
        if self.reads % self.plan.every_target_lost == 0 {
            self.counters.target_lost.fetch_add(1, Ordering::Relaxed);
            return Err(SourceError::TargetLost);
        }
        if self.reads % self.plan.every_read_error == 0 {
            self.counters.read_errors.fetch_add(1, Ordering::Relaxed);
            return Err(SourceError::Recognition(OcrError::EngineFailed {
                engine: "fault-injection".to_owned(),
                detail: "scripted failure".to_owned(),
            }));
        }
        self.inner.read(request)
    }

    fn advance(&mut self) {
        self.inner.advance()
    }
}

struct FaultyEngine {
    inner: Box<dyn TranslationEngine>,
    plan: FaultPlan,
    calls: usize,
    counters: FaultCounters,
}

impl TranslationEngine for FaultyEngine {
    fn translate(&mut self, request: &TranslationRequest) -> Result<TranslationResponse, TranslationError> {
        self.calls += 1;
        if self.calls % self.plan.every_engine_error == 0 {
            self.counters.engine_errors.fetch_add(1, Ordering::Relaxed);
            return Err(TranslationError::Network("scripted outage".to_owned()));
        }
        self.inner.translate(request)
    }

    fn is_available(&self, source: lumen_language::Language, target: lumen_language::Language) -> bool {
        self.inner.is_available(source, target)
    }

    fn engine_kind(&self) -> EngineKind {
        self.inner.engine_kind()
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ChaosReport {
    pub frames: usize,
    pub static_frames: usize,
    pub changed_frames: usize,
    pub faults: FaultsReport,
    pub cleared_frames: usize,
    pub final_blocks: usize,
    /// Zero when the run reaches this line: a panic would end the process before the report.
    pub panics: usize,
}

#[derive(Debug, Serialize)]
pub(crate) struct FaultsReport {
    pub read_errors: usize,
    pub target_lost: usize,
    pub engine_errors: usize,
}

pub fn execute(frames: u32, options: &Options) -> Result<String, RunError> {
    let frames = frames as usize;
    let mut rng = SplitMix64::new(options.seed);
    let scene = crate::soak::random_scene(options.width, options.height, frames, &mut rng);
    let plan = FaultPlan::default();
    let counters = FaultCounters::new();

    let inner_source: Box<dyn TextSource> = Box::new(SceneTextSource::new(scene.clone()));
    let faulty_source = FaultySource {
        inner: inner_source,
        plan,
        reads: 0,
        counters: counters.clone(),
    };
    let inner_engine: Box<dyn TranslationEngine> = Box::new(StubTranslationEngine::new());
    let faulty_engine = FaultyEngine {
        inner: inner_engine,
        plan,
        calls: 0,
        counters: counters.clone(),
    };

    let mut runner = PassRunner::new(PassParams {
        width: options.width,
        height: options.height,
        tile: options.tile,
        style: options.style,
        speed: options.speed,
        config: PassConfig::default(),
        source: Some(Box::new(faulty_source)),
        engine: Box::new(faulty_engine),
    });

    let mut capture = SyntheticSource::new(scene);
    let mut static_frames = 0;
    let mut cleared_frames = 0;
    let mut final_blocks = 0;

    for _ in 0..frames {
        let Some(frame) = capture.next_frame()? else {
            break;
        };
        let outcome = runner.run_frame(&frame)?;
        runner.advance_source();
        if outcome.skipped {
            static_frames += 1;
        }
        if outcome.cleared {
            cleared_frames += 1;
        }
        final_blocks = outcome.blocks.len();
    }

    let report = ChaosReport {
        frames,
        static_frames,
        changed_frames: frames - static_frames,
        faults: FaultsReport {
            read_errors: counters.read_errors.load(Ordering::Relaxed),
            target_lost: counters.target_lost.load(Ordering::Relaxed),
            engine_errors: counters.engine_errors.load(Ordering::Relaxed),
        },
        cleared_frames,
        final_blocks,
        panics: 0,
    };

    serde_json::to_string(&report).map_err(RunError::Serialize)
}
