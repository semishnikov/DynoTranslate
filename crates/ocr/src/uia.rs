//! The UI Automation text source.
//!
//! UI Automation exposes the text native applications and browsers already know about themselves —
//! exact characters, no recognition errors — which is the first source [`merge`] prefers. The
//! Windows client lives behind [`UiaSource`]; the span policy it applies ([`span_from_element`]) is
//! portable and tested everywhere, so only the COM walk itself is platform code.
//!
//! [`merge`]: crate::source::merge

use lumen_core::Rect;

use crate::language::identify;
use crate::source::{TextOrigin, TextSpan};

/// How many elements one snapshot may describe. A crowded desktop is truncated rather than walked
/// in full: the visible foreground window's text matters more than the shell's.
pub const MAX_ELEMENTS: usize = 512;

/// How deep the walk descends from the desktop root.
pub const MAX_DEPTH: usize = 32;

/// Turns one accessibility element into a span, in frame coordinates.
///
/// `screen_bounds` is what UIA reports (desktop coordinates) and `screen_origin` is where the
/// captured frame sits on the desktop, so spans land on the same pixels recognition would read.
/// Elements without a name, without area, or outside the frame produce no text. Accessibility
/// names are exact, so every span carries full confidence.
pub fn span_from_element(name: &str, screen_bounds: Rect, screen_origin: (i32, i32), frame: &Rect) -> Option<TextSpan> {
    let text = name.trim();
    if text.is_empty() {
        return None;
    }
    let bounds = Rect::new(
        screen_bounds.x - screen_origin.0,
        screen_bounds.y - screen_origin.1,
        screen_bounds.width,
        screen_bounds.height,
    );
    let bounds = bounds.clamp_to(frame)?;
    Some(TextSpan {
        language: identify(text),
        text: text.to_owned(),
        origin: TextOrigin::Automation,
        bounds,
        confidence: 1.0,
    })
}

#[cfg(windows)]
use crate::source::SourceError;
#[cfg(windows)]
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
#[cfg(windows)]
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};

/// A [`TextSource`](crate::source::TextSource) over the live UI Automation element tree.
#[cfg(windows)]
pub struct UiaSource {
    automation: IUIAutomation,
    screen_origin: (i32, i32),
    // Declared after `automation` on purpose: fields drop in declaration order, so the client is
    // released before the apartment it was initialised for is torn down.
    _com: ComGuard,
}

/// Balances the `CoInitializeEx` call in [`UiaSource::new`], but only when that call initialised
/// COM for this thread.
#[cfg(windows)]
struct ComGuard {
    owned: bool,
}

#[cfg(windows)]
impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.owned {
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(windows)]
impl UiaSource {
    /// Starts a UI Automation client. `screen_origin` is where the captured frame sits on the
    /// desktop; UIA reports desktop coordinates and spans come back in frame coordinates.
    pub fn new(screen_origin: (i32, i32)) -> Result<Self, SourceError> {
        let com_owned = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL) }.map_err(|error| SourceError::Failed {
                name: "ui-automation".to_owned(),
                detail: error.message(),
            })?;
        Ok(Self {
            automation,
            screen_origin,
            _com: ComGuard { owned: com_owned },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::LanguageId;

    #[test]
    fn a_named_element_becomes_an_automation_span_in_frame_coordinates() {
        let frame = Rect::new(0, 0, 800, 600);
        let span = span_from_element("Play", Rect::new(140, 90, 120, 30), (100, 50), &frame).unwrap();
        assert_eq!(span.text, "Play");
        assert_eq!(span.language, LanguageId::En);
        assert_eq!(span.origin, TextOrigin::Automation);
        assert_eq!(span.bounds, Rect::new(40, 40, 120, 30));
        assert!((span.confidence - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn elements_without_text_or_area_are_dropped() {
        let frame = Rect::new(0, 0, 800, 600);
        assert_eq!(span_from_element("   ", Rect::new(0, 0, 10, 10), (0, 0), &frame), None);
        assert_eq!(span_from_element("Play", Rect::new(0, 0, 0, 10), (0, 0), &frame), None);
    }

    #[test]
    fn elements_outside_the_frame_are_dropped_and_partial_ones_are_clamped() {
        let frame = Rect::new(0, 0, 800, 600);
        assert_eq!(
            span_from_element("Play", Rect::new(900, 10, 50, 20), (0, 0), &frame),
            None
        );
        let span = span_from_element("Play", Rect::new(780, 10, 50, 20), (0, 0), &frame).unwrap();
        assert_eq!(span.bounds, Rect::new(780, 10, 20, 20));
    }
}
