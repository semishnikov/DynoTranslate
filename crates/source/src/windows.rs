//! Windows UI Automation text source.
//!
//! The accessibility tree already carries the exact characters an application drew and the exact
//! rectangle it drew them in, so nothing has to be guessed from pixels: no recognition error, no
//! font or styling problems, and no model cost beyond a tree walk. UI Automation reads are
//! cross-process COM calls the operating system brokers, which keeps the no-injection rule from
//! ADR 0002 — Lumen asks Windows what a window contains and never touches the target process.
//!
//! Coverage is partial by nature: games and anything that draws its text as pixels expose no text
//! pattern, which is why recognition stays in the pipeline and why [`crate::merge`] exists.

use std::ffi::c_void;

use lumen_core::Rect;
use windows::Win32::Foundation::{HWND, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, SAFEARRAY,
};
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound, SafeArrayUnaccessData,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern, IUIAutomationTextRange,
    IUIAutomationValuePattern, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, TextUnit_Line,
    TreeScope_Descendants, UIA_TextPatternId, UIA_ValuePatternId,
};

use crate::{local_bounds, sort_reading_order, within_regions};
use crate::{ReadRequest, SourceError, SourceKind, TextRun, TextSource};

/// Stops a misbehaving provider from turning one window into an endless walk. A dense desktop
/// application exposes a few hundred text-bearing elements, so this is well past one.
const MAX_ELEMENTS: i32 = 4096;

/// The same guard inside one text pattern: a provider that refuses to advance still terminates.
const MAX_LINES: usize = 1024;

/// `GetText` takes a character limit; -1 asks for everything the range holds.
const UNLIMITED: i32 = -1;

/// Reads the text a window exposes through UI Automation.
///
/// The adapter owns a COM apartment for as long as it lives and releases it on drop, so a source
/// can be created once per capture session instead of paying an initialisation per frame.
pub struct UiAutomationSource {
    automation: IUIAutomation,
    /// Whether this source initialised COM on its thread and therefore has to unwind it.
    owns_apartment: bool,
}

impl UiAutomationSource {
    /// Connects to UI Automation on the calling thread.
    ///
    /// UI Automation is COM, so the thread needs an apartment first. A success that is not `S_OK`
    /// means the thread already had one and this source shares it; `RPC_E_CHANGED_MODE` means the
    /// thread was initialised multi-threaded, which UI Automation serves just as well. Only the
    /// first case leaves this source owning an initialisation it must unwind.
    pub fn connect() -> Result<Self, SourceError> {
        let status = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let owns_apartment = if status.is_ok() {
            true
        } else if status == RPC_E_CHANGED_MODE {
            false
        } else {
            return Err(code_failure("CoInitializeEx", status.0));
        };

        let automation: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
            .map_err(|error| failed("CoCreateInstance", &error))?;

        Ok(Self {
            automation,
            owns_apartment,
        })
    }

    /// Walks the window's descendants and reads the text each one exposes.
    fn walk(&self, root: &IUIAutomationElement, frame: Rect) -> Result<Vec<TextRun>, SourceError> {
        let condition =
            unsafe { self.automation.CreateTrueCondition() }.map_err(|error| failed("CreateTrueCondition", &error))?;
        let found = unsafe { root.FindAll(TreeScope_Descendants, &condition) };
        let found = found.map_err(|error| failed("FindAll", &error))?;

        let length = unsafe { found.Length() }.map_err(|error| failed("Length", &error))?;
        let mut runs = Vec::new();
        for index in 0..length.min(MAX_ELEMENTS) {
            let element = match unsafe { found.GetElement(index) } {
                Ok(element) => element,
                // One element a provider refuses to hand over is no reason to lose the window.
                Err(_) => continue,
            };
            if is_offscreen(&element) {
                continue;
            }
            runs.extend(read_element(&element, frame));
        }
        Ok(runs)
    }
}

impl TextSource for UiAutomationSource {
    fn name(&self) -> &str {
        "ui-automation"
    }

    fn kind(&self) -> SourceKind {
        SourceKind::UiAutomation
    }

    fn read(&mut self, request: &ReadRequest<'_>) -> Result<Vec<TextRun>, SourceError> {
        let frame = request.target.bounds;
        let handle = HWND(request.target.id as *mut c_void);
        let root = unsafe { self.automation.ElementFromHandle(handle) }.map_err(|_| SourceError::TargetLost)?;

        let runs = self.walk(&root, frame)?;
        let mut visible = within_regions(runs, request.regions);
        sort_reading_order(&mut visible);
        Ok(visible)
    }
}

impl Drop for UiAutomationSource {
    fn drop(&mut self) {
        if self.owns_apartment {
            unsafe { CoUninitialize() };
        }
    }
}

/// Reads one element: the lines its text pattern reports, or failing that the name or value the
/// control exposes, which is what a screen reader would read aloud.
fn read_element(element: &IUIAutomationElement, frame: Rect) -> Vec<TextRun> {
    let lines = text_lines(element, frame);
    if !lines.is_empty() {
        return lines;
    }
    labelled_text(element, frame).into_iter().collect()
}

/// The lines an element's text pattern reports.
///
/// Elements without a text pattern — most buttons and labels — report nothing, which sends
/// [`read_element`] to the element's name instead.
fn text_lines(element: &IUIAutomationElement, frame: Rect) -> Vec<TextRun> {
    let pattern = unsafe { element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) };
    match pattern {
        Ok(pattern) => lines_of(&pattern, frame).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Walks a text pattern one line at a time.
///
/// The cursor starts collapsed at the beginning of the document. Each step expands it over the line
/// it is on, reads that line, then moves the cursor to where the line ended. The walk stops when the
/// cursor reaches the end of the document, or when the line budget runs out because a provider
/// stopped advancing.
fn lines_of(pattern: &IUIAutomationTextPattern, frame: Rect) -> Result<Vec<TextRun>, SourceError> {
    let document = unsafe { pattern.DocumentRange() }.map_err(|error| failed("DocumentRange", &error))?;
    let cursor = unsafe { document.Clone() }.map_err(|error| failed("Clone", &error))?;
    collapse_to_start(&cursor)?;

    let mut runs = Vec::new();
    for _ in 0..MAX_LINES {
        let line = unsafe { cursor.Clone() }.map_err(|error| failed("Clone", &error))?;
        let expanded = unsafe { line.ExpandToEnclosingUnit(TextUnit_Line) };
        expanded.map_err(|error| failed("ExpandToEnclosingUnit", &error))?;

        if let Some(run) = line_run(&line, frame)? {
            runs.push(run);
        }

        advance_past(&cursor, &line)?;
        if reached_end(&cursor, &document) {
            break;
        }
    }
    Ok(runs)
}

/// Collapses a range onto its own start, so that expanding it next covers one line rather than the
/// rest of the document.
fn collapse_to_start(range: &IUIAutomationTextRange) -> Result<(), SourceError> {
    unsafe { range.MoveEndpointByRange(TextPatternRangeEndpoint_End, range, TextPatternRangeEndpoint_Start) }
        .map_err(|error| failed("MoveEndpointByRange", &error))
}

/// Moves the cursor to the end of the line just read, ready to expand over the next one.
fn advance_past(cursor: &IUIAutomationTextRange, line: &IUIAutomationTextRange) -> Result<(), SourceError> {
    unsafe { cursor.MoveEndpointByRange(TextPatternRangeEndpoint_Start, line, TextPatternRangeEndpoint_End) }
        .map_err(|error| failed("MoveEndpointByRange", &error))
}

/// Whether the cursor has reached the end of the document. A provider that will not answer is
/// treated as "not yet", which the line budget then stops.
fn reached_end(cursor: &IUIAutomationTextRange, document: &IUIAutomationTextRange) -> bool {
    let comparison =
        unsafe { cursor.CompareEndpoints(TextPatternRangeEndpoint_Start, document, TextPatternRangeEndpoint_End) };
    comparison.unwrap_or(-1) >= 0
}

/// One text range as a run: its characters and the rectangle Windows reports for it, in frame
/// pixels. Empty ranges — the padding between paragraphs — are dropped.
fn line_run(range: &IUIAutomationTextRange, frame: Rect) -> Result<Option<TextRun>, SourceError> {
    let text = unsafe { range.GetText(UNLIMITED) }.map_err(|error| failed("GetText", &error))?;
    let text = match trimmed(text.to_string()) {
        Some(text) => text,
        None => return Ok(None),
    };
    let bounds = match range_bounds(range, frame)? {
        Some(bounds) => bounds,
        None => return Ok(None),
    };
    Ok(Some(TextRun::new(text, bounds, SourceKind::UiAutomation, 1.0)))
}

/// The rectangle a text range occupies, in frame pixels.
///
/// Windows reports one rectangle per visual line the range spans. They are unioned, because the
/// translation replaces the whole logical line at once.
fn range_bounds(range: &IUIAutomationTextRange, frame: Rect) -> Result<Option<Rect>, SourceError> {
    let rectangles = unsafe { range.GetBoundingRectangles() };
    let rectangles = rectangles.map_err(|error| failed("GetBoundingRectangles", &error))?;

    // `Rect::union` hands back the other side when one of them is empty, so the accumulator can
    // start at zero size and still end up exactly covering the text.
    let mut union = Rect::new(0, 0, 0, 0);
    for value in take_doubles(rectangles)?.chunks_exact(4) {
        // Each group of four is left, top, width and height, in desktop coordinates.
        let (left, top, width, height) = (value[0], value[1], value[2], value[3]);
        if !(width > 0.0 && height > 0.0) {
            continue;
        }
        let desktop = Rect::new(left as i32, top as i32, width as u32, height as u32);
        if let Some(local) = local_bounds(desktop, frame) {
            union = union.union(&local);
        }
    }

    Ok((!union.is_empty()).then_some(union))
}

/// Copies a COM `SAFEARRAY` of doubles into a `Vec` and releases the array.
///
/// The array belongs to the callee, so holding the raw pointer past this call would read freed
/// memory: the values are copied out and the array destroyed before returning.
fn take_doubles(array: *mut SAFEARRAY) -> Result<Vec<f64>, SourceError> {
    if array.is_null() {
        return Ok(Vec::new());
    }
    let _guard = ArrayGuard(array);

    let lower = unsafe { SafeArrayGetLBound(array, 1) }.map_err(|error| failed("SafeArrayGetLBound", &error))?;
    let upper = unsafe { SafeArrayGetUBound(array, 1) }.map_err(|error| failed("SafeArrayGetUBound", &error))?;
    let count = (upper - lower + 1).max(0) as usize;
    if count == 0 {
        return Ok(Vec::new());
    }

    let mut data: *mut c_void = std::ptr::null_mut();
    unsafe { SafeArrayAccessData(array, &mut data) }.map_err(|error| failed("SafeArrayAccessData", &error))?;
    let values = if data.is_null() {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(data as *const f64, count) }.to_vec()
    };
    unsafe { SafeArrayUnaccessData(array) }.map_err(|error| failed("SafeArrayUnaccessData", &error))?;

    Ok(values)
}

/// Releases a `SAFEARRAY` however the caller leaves the scope.
struct ArrayGuard(*mut SAFEARRAY);

impl Drop for ArrayGuard {
    fn drop(&mut self) {
        let _ = unsafe { SafeArrayDestroy(self.0) };
    }
}

/// The text a control without a text pattern still reports: its accessible name first, then the
/// value pattern that edit boxes, combo boxes and sliders expose.
fn labelled_text(element: &IUIAutomationElement, frame: Rect) -> Option<TextRun> {
    let bounds = local_bounds(desktop_bounds(element)?, frame)?;
    let text = name_of(element).or_else(|| value_of(element))?;
    Some(TextRun::new(text, bounds, SourceKind::UiAutomation, 1.0))
}

/// The element's accessible name, when it has one that is more than whitespace.
fn name_of(element: &IUIAutomationElement) -> Option<String> {
    let name = unsafe { element.CurrentName() }.ok()?.to_string();
    trimmed(name)
}

/// The value pattern's value, when the element exposes one that is more than whitespace.
fn value_of(element: &IUIAutomationElement) -> Option<String> {
    let pattern = unsafe { element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }.ok()?;
    let value = unsafe { pattern.CurrentValue() }.ok()?.to_string();
    trimmed(value)
}

/// `None` when trimming leaves nothing, so a control whose name is only whitespace is not a run.
fn trimmed(text: String) -> Option<String> {
    let text = text.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// Where an element sits on the desktop, or `None` when it has no size at all.
fn desktop_bounds(element: &IUIAutomationElement) -> Option<Rect> {
    let rect = unsafe { element.CurrentBoundingRectangle() }.ok()?;
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(Rect::new(rect.left, rect.top, width as u32, height as u32))
}

/// UI Automation keeps elements that are scrolled or clipped out of view in the tree; their bounds
/// are meaningless, so they never reach the merge.
fn is_offscreen(element: &IUIAutomationElement) -> bool {
    match unsafe { element.CurrentIsOffscreen() } {
        Ok(flag) => flag.as_bool(),
        // An element that will not answer is treated as visible and left to the bounds check.
        Err(_) => false,
    }
}

/// Names the call that failed, so a UI Automation error in a log says which call it came from.
fn failed(operation: &'static str, error: &windows::core::Error) -> SourceError {
    SourceError::Platform {
        operation,
        detail: error.message(),
    }
}

/// The same, for the calls that return a bare code instead of an error object.
fn code_failure(operation: &'static str, code: i32) -> SourceError {
    SourceError::Platform {
        operation,
        detail: format!("HRESULT {code:#010X}"),
    }
}
