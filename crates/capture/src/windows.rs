//! Windows capture adapter.
//!
//! This is the only place in the capture crate that knows about window handles. It enumerates
//! candidate targets and copies their pixels through the desktop compositor's own path, which is
//! sanctioned by the operating system and invisible to the captured application: nothing is
//! injected, hooked or read out of the target process.

use std::ffi::c_void;

use lumen_core::{Frame, Rect};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, PrintWindow,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, SRCCOPY,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, PRINT_WINDOW_FLAGS,
};

use crate::{CaptureError, CaptureSource, CaptureTarget};

/// Lists the windows a user could plausibly want translated: visible, not minimised, not cloaked by
/// the shell, and large enough to hold readable text.
pub fn enumerate_targets() -> Result<Vec<CaptureTarget>, CaptureError> {
    let mut found: Vec<CaptureTarget> = Vec::new();
    let pointer = LPARAM(&mut found as *mut Vec<CaptureTarget> as isize);

    unsafe { EnumWindows(Some(collect_target), pointer) }.map_err(|error| CaptureError::Platform {
        operation: "EnumWindows",
        detail: error.message(),
    })?;

    found.sort_by_key(|target| std::cmp::Reverse(target.bounds.area()));
    Ok(found)
}

unsafe extern "system" fn collect_target(handle: HWND, state: LPARAM) -> BOOL {
    let targets = &mut *(state.0 as *mut Vec<CaptureTarget>);
    if let Some(target) = describe_window(handle) {
        targets.push(target);
    }
    TRUE
}

fn describe_window(handle: HWND) -> Option<CaptureTarget> {
    if !unsafe { IsWindowVisible(handle) }.as_bool() || unsafe { IsIconic(handle) }.as_bool() {
        return None;
    }
    if is_cloaked(handle) {
        return None;
    }

    let bounds = window_bounds(handle)?;
    if bounds.width < 160 || bounds.height < 120 {
        return None;
    }

    let title = window_title(handle);
    if title.is_empty() {
        return None;
    }

    Some(CaptureTarget {
        id: handle.0 as u64,
        title,
        process: process_name(handle).unwrap_or_else(|| "unknown".to_owned()),
        bounds,
    })
}

/// Windows keeps suspended store apps and hidden shell surfaces in the enumeration; they are
/// reported as cloaked rather than invisible and would otherwise capture as empty rectangles.
fn is_cloaked(handle: HWND) -> bool {
    let mut cloaked: u32 = 0;
    let result = unsafe {
        DwmGetWindowAttribute(
            handle,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    result.is_ok() && cloaked != 0
}

/// The extended frame bounds exclude the invisible resize border the window rectangle includes, so
/// captured frames line up with what the user sees.
fn window_bounds(handle: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    let extended = unsafe {
        DwmGetWindowAttribute(
            handle,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut c_void,
            std::mem::size_of::<RECT>() as u32,
        )
    };
    if extended.is_err() && unsafe { GetWindowRect(handle, &mut rect) }.is_err() {
        return None;
    }
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(Rect::new(rect.left, rect.top, width as u32, height as u32))
}

fn window_title(handle: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(handle) };
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let written = unsafe { GetWindowTextW(handle, &mut buffer) };
    String::from_utf16_lossy(&buffer[..written as usize])
}

fn process_name(handle: HWND) -> Option<String> {
    let mut process_id = 0u32;
    unsafe { GetWindowThreadProcessId(handle, Some(&mut process_id)) };
    if process_id == 0 {
        return None;
    }

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?;
    let mut buffer = vec![0u16; 260];
    let mut length = buffer.len() as u32;
    let query = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    };
    let _ = unsafe { windows::Win32::Foundation::CloseHandle(process) };
    query.ok()?;

    let path = String::from_utf16_lossy(&buffer[..length as usize]);
    path.rsplit('\\').next().map(str::to_owned)
}

/// Copies a window's pixels through the desktop device context.
///
/// Windows Graphics Capture is the intended production path (ADR 0003) and replaces this source
/// once its session plumbing lands; this adapter exists so the rest of the pipeline can run against
/// real windows today. It shares the same trait, so the swap touches nothing downstream.
pub struct DesktopCopySource {
    target: CaptureTarget,
}

impl DesktopCopySource {
    pub fn new(target: CaptureTarget) -> Self {
        Self { target }
    }

    pub fn for_window(handle: HWND) -> Result<Self, CaptureError> {
        describe_window(handle)
            .map(Self::new)
            .ok_or(CaptureError::TargetNotVisible)
    }

    fn capture(&mut self) -> Result<Frame, CaptureError> {
        let bounds = window_bounds(HWND(self.target.id as *mut c_void)).ok_or(CaptureError::TargetLost)?;
        self.target.bounds = bounds;

        let screen = ScreenContext::acquire()?;
        let memory = MemoryContext::compatible_with(&screen, bounds.width, bounds.height)?;

        unsafe {
            BitBlt(
                memory.dc,
                0,
                0,
                bounds.width as i32,
                bounds.height as i32,
                screen.dc,
                bounds.x,
                bounds.y,
                SRCCOPY,
            )
        }
        .map_err(|error| CaptureError::Platform {
            operation: "BitBlt",
            detail: error.message(),
        })?;

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: bounds.width as i32,
                // Negative height requests a top-down bitmap, matching the row order of every
                // other frame in the pipeline.
                biHeight: -(bounds.height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut pixels = vec![0u8; bounds.width as usize * bounds.height as usize * 4];
        let copied = unsafe {
            GetDIBits(
                memory.dc,
                memory.bitmap,
                0,
                bounds.height,
                Some(pixels.as_mut_ptr() as *mut c_void),
                &mut info,
                DIB_RGB_COLORS,
            )
        };
        if copied == 0 {
            return Err(CaptureError::Platform {
                operation: "GetDIBits",
                detail: "the device context returned no scan lines".to_owned(),
            });
        }

        // GDI leaves the alpha channel undefined for desktop copies; downstream stages treat frames
        // as opaque, so it is normalised here rather than guarded for everywhere else.
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }

        Frame::packed(bounds.width, bounds.height, pixels).map_err(CaptureError::from)
    }
}

/// The window's own picture. An overlay drawn on top is not included. Exclusive fullscreen
/// often comes back black; the caller then hides its overlay and uses [`capture_window_screen`].
pub fn capture_window_picture(handle: isize) -> Result<(Frame, Rect), CaptureError> {
    let hwnd = HWND(handle as *mut c_void);
    let bounds = window_bounds(hwnd).ok_or(CaptureError::TargetLost)?;
    let frame = print_window(hwnd, bounds.width, bounds.height)?;
    if mostly_black(&frame) {
        return Err(CaptureError::Platform {
            operation: "PrintWindow",
            detail: "the window rendered an empty frame".to_owned(),
        });
    }
    Ok((frame, bounds))
}

/// A copy of the screen where the window sits. Includes anything drawn on top, so the caller
/// hides its overlay before calling.
pub fn capture_window_screen(handle: isize) -> Result<(Frame, Rect), CaptureError> {
    let hwnd = HWND(handle as *mut c_void);
    let bounds = window_bounds(hwnd).ok_or(CaptureError::TargetLost)?;
    let mut source = DesktopCopySource::new(CaptureTarget {
        id: handle as u64,
        title: window_title(hwnd),
        process: String::new(),
        bounds,
    });
    let frame = source.next_frame()?.ok_or(CaptureError::TargetLost)?;
    Ok((frame, source.target().bounds))
}

fn print_window(hwnd: HWND, width: u32, height: u32) -> Result<Frame, CaptureError> {
    let screen = ScreenContext::acquire()?;
    let memory = MemoryContext::compatible_with(&screen, width, height)?;
    let printed = unsafe { PrintWindow(hwnd, memory.dc, PRINT_WINDOW_FLAGS(2)) };
    if !printed.as_bool() {
        return Err(CaptureError::Platform {
            operation: "PrintWindow",
            detail: "the window declined to render".to_owned(),
        });
    }
    read_bitmap(&memory, width, height)
}

fn read_bitmap(memory: &MemoryContext, width: u32, height: u32) -> Result<Frame, CaptureError> {
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let copied = unsafe {
        GetDIBits(
            memory.dc,
            memory.bitmap,
            0,
            height,
            Some(pixels.as_mut_ptr() as *mut c_void),
            &mut info,
            DIB_RGB_COLORS,
        )
    };
    if copied == 0 {
        return Err(CaptureError::Platform {
            operation: "GetDIBits",
            detail: "the device context returned no scan lines".to_owned(),
        });
    }
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    Frame::packed(width, height, pixels).map_err(CaptureError::from)
}

fn mostly_black(frame: &Frame) -> bool {
    let mut dark = 0u32;
    let mut seen = 0u32;
    let step = (frame.width() / 32).max(1);
    let mut y = 0;
    while y < frame.height() {
        let mut x = 0;
        while x < frame.width() {
            let pixel = frame.pixel(x, y);
            seen += 1;
            if pixel[0] < 12 && pixel[1] < 12 && pixel[2] < 12 {
                dark += 1;
            }
            x += step;
        }
        y += step;
    }
    seen > 0 && dark * 100 / seen > 92
}

impl CaptureSource for DesktopCopySource {
    fn target(&self) -> &CaptureTarget {
        &self.target
    }

    fn next_frame(&mut self) -> Result<Option<Frame>, CaptureError> {
        self.capture().map(Some)
    }
}

struct ScreenContext {
    dc: HDC,
}

impl ScreenContext {
    fn acquire() -> Result<Self, CaptureError> {
        let dc = unsafe { GetDC(None) };
        if dc.is_invalid() {
            return Err(CaptureError::Platform {
                operation: "GetDC",
                detail: "the desktop device context is unavailable".to_owned(),
            });
        }
        Ok(Self { dc })
    }
}

impl Drop for ScreenContext {
    fn drop(&mut self) {
        unsafe { ReleaseDC(None, self.dc) };
    }
}

struct MemoryContext {
    dc: HDC,
    bitmap: HBITMAP,
}

impl MemoryContext {
    fn compatible_with(screen: &ScreenContext, width: u32, height: u32) -> Result<Self, CaptureError> {
        let dc = unsafe { CreateCompatibleDC(screen.dc) };
        if dc.is_invalid() {
            return Err(CaptureError::Platform {
                operation: "CreateCompatibleDC",
                detail: "no memory device context could be created".to_owned(),
            });
        }
        let bitmap = unsafe { CreateCompatibleBitmap(screen.dc, width as i32, height as i32) };
        if bitmap.is_invalid() {
            unsafe {
                let _ = DeleteDC(dc);
            };
            return Err(CaptureError::Platform {
                operation: "CreateCompatibleBitmap",
                detail: format!("no {width}x{height} bitmap could be allocated"),
            });
        }
        unsafe { SelectObject(dc, bitmap) };
        Ok(Self { dc, bitmap })
    }
}

impl Drop for MemoryContext {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.bitmap);
            let _ = DeleteDC(self.dc);
        }
    }
}
