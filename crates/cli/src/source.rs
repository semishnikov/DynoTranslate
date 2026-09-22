//! The text source the harness reads from.
//!
//! A scripted scene already knows which words it is showing and where, so the harness can report
//! them the way a recognition engine would. That keeps the whole path — source, merge, layout,
//! compositor — exercised end to end without a platform and without a trained model.

use lumen_capture::synthetic::Scene;
use lumen_core::Rect;
use lumen_source::{within_regions, ReadRequest, SourceError, SourceKind, TextRun, TextSource};

pub struct SceneTextSource {
    scene: Scene,
    index: usize,
    calls: usize,
    last_regions: Vec<Rect>,
}

impl SceneTextSource {
    pub fn new(scene: Scene) -> Self {
        Self {
            scene,
            index: 0,
            calls: 0,
            last_regions: Vec::new(),
        }
    }

    /// How many times [`TextSource::read`] has run.
    pub const fn calls(&self) -> usize {
        self.calls
    }

    /// The regions the last read was restricted to. An empty list means the whole frame.
    pub fn last_regions(&self) -> &[Rect] {
        &self.last_regions
    }
}

impl TextSource for SceneTextSource {
    fn name(&self) -> &str {
        "scene"
    }

    fn kind(&self) -> SourceKind {
        SourceKind::Ocr
    }

    /// Reports the labels the scene shows on the current frame. The harness steps to the next
    /// one with [`TextSource::advance`], once per captured frame, so the source stays in step
    /// with the capture no matter how many frames actually needed a read — a static frame
    /// reuses the previous pass and never calls this at all.
    fn read(&mut self, request: &ReadRequest<'_>) -> Result<Vec<TextRun>, SourceError> {
        let runs: Vec<TextRun> = self
            .scene
            .labels_at(self.index)
            .into_iter()
            .map(|(bounds, text)| TextRun::new(text, bounds, SourceKind::Ocr, 1.0))
            .collect();
        self.calls += 1;
        self.last_regions = request.regions.to_vec();
        Ok(within_regions(runs, request.regions))
    }

    fn advance(&mut self) {
        self.index = (self.index + 1).min(self.scene.frames);
    }
}
