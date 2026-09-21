//! A deterministic capture source that renders a scripted scene.
//!
//! Real capture cannot run on a build agent, so fidelity and scheduling work is driven from scenes
//! described in code: the same script always produces the same pixels, which makes golden images
//! and latency measurements comparable between runs and between machines.

use lumen_core::{Frame, Rect};

use crate::{CaptureError, CaptureSource, CaptureTarget};

#[derive(Debug, Clone)]
pub struct SceneBlock {
    pub rect: Rect,
    pub color: [u8; 4],
    /// Frame index from which the block is drawn.
    pub appears_at: usize,
    /// Frame index from which the block is no longer drawn.
    pub disappears_at: Option<usize>,
}

impl SceneBlock {
    pub fn new(rect: Rect, color: [u8; 4]) -> Self {
        Self {
            rect,
            color,
            appears_at: 0,
            disappears_at: None,
        }
    }

    pub fn from_frame(mut self, frame: usize) -> Self {
        self.appears_at = frame;
        self
    }

    pub fn until_frame(mut self, frame: usize) -> Self {
        self.disappears_at = Some(frame);
        self
    }

    fn visible_at(&self, frame: usize) -> bool {
        frame >= self.appears_at && self.disappears_at.map_or(true, |end| frame < end)
    }
}

#[derive(Debug, Clone)]
pub struct Scene {
    pub width: u32,
    pub height: u32,
    pub background: [u8; 4],
    pub blocks: Vec<SceneBlock>,
    pub frames: usize,
}

impl Scene {
    /// A window whose menu appears on the third frame and whose tooltip flickers in and out, which
    /// is the shape of the workload the scheduler and the change detector are tuned for. Every menu
    /// row and the tooltip carry a dark ink bar across their plate, so colour sampling and layout
    /// analysis see the same two-colour structure real glyphs would give them.
    pub fn menu_appearing(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            background: [28, 24, 20, 255],
            blocks: vec![
                SceneBlock::new(Rect::new(64, 64, 320, 34), [214, 210, 204, 255]).from_frame(2),
                SceneBlock::new(Rect::new(84, 73, 96, 14), [34, 28, 22, 255]).from_frame(2),
                SceneBlock::new(Rect::new(64, 112, 320, 34), [214, 210, 204, 255]).from_frame(2),
                SceneBlock::new(Rect::new(84, 121, 120, 14), [34, 28, 22, 255]).from_frame(2),
                SceneBlock::new(Rect::new(64, 160, 220, 34), [214, 210, 204, 255]).from_frame(2),
                SceneBlock::new(Rect::new(84, 169, 72, 14), [34, 28, 22, 255]).from_frame(2),
                SceneBlock::new(Rect::new(520, 300, 240, 80), [180, 176, 170, 255])
                    .from_frame(5)
                    .until_frame(7),
                SceneBlock::new(Rect::new(540, 332, 160, 12), [30, 26, 20, 255])
                    .from_frame(5)
                    .until_frame(7),
            ],
            frames: 8,
        }
    }

    pub fn render(&self, index: usize) -> Result<Frame, lumen_core::FrameError> {
        let mut frame = Frame::filled(self.width, self.height, self.background)?;
        for block in &self.blocks {
            if block.visible_at(index) {
                frame.fill_rect(block.rect, block.color);
            }
        }
        Ok(frame)
    }
}

#[derive(Debug)]
pub struct SyntheticSource {
    scene: Scene,
    target: CaptureTarget,
    index: usize,
}

impl SyntheticSource {
    pub fn new(scene: Scene) -> Self {
        let target = CaptureTarget {
            id: 1,
            title: "Synthetic scene".to_owned(),
            process: "lumen-pipeline".to_owned(),
            bounds: Rect::new(0, 0, scene.width, scene.height),
        };
        Self {
            scene,
            target,
            index: 0,
        }
    }

    pub fn frames_remaining(&self) -> usize {
        self.scene.frames.saturating_sub(self.index)
    }
}

impl CaptureSource for SyntheticSource {
    fn target(&self) -> &CaptureTarget {
        &self.target
    }

    fn next_frame(&mut self) -> Result<Option<Frame>, CaptureError> {
        if self.index >= self.scene.frames {
            return Ok(None);
        }
        let frame = self.scene.render(self.index)?;
        self.index += 1;
        Ok(Some(frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_core::ChangeDetector;

    #[test]
    fn the_scene_is_reproducible() {
        let scene = Scene::menu_appearing(800, 600);
        assert_eq!(scene.render(4).unwrap(), scene.render(4).unwrap());
    }

    #[test]
    fn blocks_respect_their_lifetime() {
        let scene = Scene::menu_appearing(800, 600);
        let before = scene.render(4).unwrap();
        let during = scene.render(5).unwrap();
        let after = scene.render(7).unwrap();
        assert_ne!(before.pixel(560, 320), during.pixel(560, 320));
        assert_eq!(before.pixel(560, 320), after.pixel(560, 320));
    }

    #[test]
    fn the_source_ends_after_the_scripted_frames() {
        let mut source = SyntheticSource::new(Scene::menu_appearing(320, 240));
        let mut produced = 0;
        while source.next_frame().unwrap().is_some() {
            produced += 1;
        }
        assert_eq!(produced, 8);
        assert_eq!(source.frames_remaining(), 0);
    }

    #[test]
    fn a_still_stretch_of_the_scene_produces_no_change_regions() {
        let scene = Scene::menu_appearing(640, 480);
        let mut detector = ChangeDetector::default();
        detector.accept(&scene.render(2).unwrap());
        let report = detector.accept(&scene.render(3).unwrap());
        assert!(report.is_static());
    }

    #[test]
    fn the_menu_appearing_is_reported_as_change() {
        let scene = Scene::menu_appearing(640, 480);
        let mut detector = ChangeDetector::default();
        detector.accept(&scene.render(1).unwrap());
        let report = detector.accept(&scene.render(2).unwrap());
        assert!(!report.is_static());
        assert!(report.regions.iter().any(|rect| rect.contains(70, 70)));
    }
}
