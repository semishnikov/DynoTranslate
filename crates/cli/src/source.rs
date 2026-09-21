//! The text source the harness reads from.
//!
//! A scripted scene already knows which words it is showing and where, so the harness can report
//! them the way a recognition engine would. That keeps the whole path — source, merge, layout,
//! compositor — exercised end to end without a platform and without a trained model.

use lumen_capture::synthetic::Scene;
use lumen_source::{within_regions, ReadRequest, SourceError, SourceKind, TextRun, TextSource};

pub struct SceneTextSource {
    scene: Scene,
    index: usize,
}

impl SceneTextSource {
    pub fn new(scene: Scene) -> Self {
        Self { scene, index: 0 }
    }
}

impl TextSource for SceneTextSource {
    fn name(&self) -> &str {
        "scene"
    }

    fn kind(&self) -> SourceKind {
        SourceKind::Ocr
    }

    /// Reports the labels the scene shows on the current frame, then steps to the next one, so the
    /// source stays in step with the capture source rendering the same scene.
    fn read(&mut self, request: &ReadRequest<'_>) -> Result<Vec<TextRun>, SourceError> {
        let runs: Vec<TextRun> = self
            .scene
            .labels_at(self.index)
            .into_iter()
            .map(|(bounds, text)| TextRun::new(text, bounds, SourceKind::Ocr, 1.0))
            .collect();
        self.index += 1;
        Ok(within_regions(runs, request.regions))
    }
}
