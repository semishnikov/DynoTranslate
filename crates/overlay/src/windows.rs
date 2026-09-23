//! Windows overlay surface.
//!
//! A layered, topmost tool window that cannot be clicked, cannot take focus, does not appear in the
//! switcher, and is excluded from screen capture so the pipeline never reads its own output back.

use std::ffi::c_void;

use lumen_core::{Frame, Rect};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowDisplayAffinity, RegisterClassExW,
    SetWindowDisplayAffinity, SetWindowPos, ShowWindow, UpdateLayeredWindow, CS_HREDRAW, CS_VREDRAW, HWND_TOPMOST,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNA, ULW_ALPHA,
    WDA_EXCLUDEFROMCAPTURE, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::compositor::CLEAR;
use crate::surface::{OverlaySurface, SurfaceError, SurfaceProperties};

const CLASS_NAME: PCWSTR = w!("LumenOverlaySurface");

pub struct LayeredOverlay {
    handle: HWND,
    width: u32,
    height: u32,
    origin: POINT,
    excluded_from_capture: bool,
}

impl LayeredOverlay {
    pub fn create(bounds: Rect) -> Result<Self, SurfaceError> {
        register_class()?;

        let handle = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                CLASS_NAME,
                PCWSTR::null(),
                WS_POPUP,
                bounds.x,
                bounds.y,
                bounds.width as i32,
                bounds.height as i32,
                None,
                None,
                None,
                None,
            )
        }
        .map_err(|error| SurfaceError::Platform {
            operation: "CreateWindowExW",
            detail: error.message(),
        })?;

        // WDA_EXCLUDEFROMCAPTURE needs Windows 10 2004 or newer. On older builds the overlay stays
        // visible to capture, and the pipeline subtracts its rectangles from the frame instead.
        let excluded = unsafe { SetWindowDisplayAffinity(handle, WDA_EXCLUDEFROMCAPTURE) }.is_ok();
        if !excluded {
            tracing_excluded_fallback();
        }

        // Do not call SetLayeredWindowAttributes here. After that call, UpdateLayeredWindow
        // fails until the layered style is cleared, so the translation never lands on the window.
        unsafe {
            let _ = SetWindowPos(
                handle,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            let _ = ShowWindow(handle, SW_SHOWNA);
        }

        Ok(Self {
            handle,
            width: bounds.width,
            height: bounds.height,
            origin: POINT {
                x: bounds.x,
                y: bounds.y,
            },
            excluded_from_capture: excluded,
        })
    }

    pub fn move_to(&mut self, bounds: Rect) -> Result<(), SurfaceError> {
        self.origin = POINT {
            x: bounds.x,
            y: bounds.y,
        };
        unsafe {
            SetWindowPos(
                self.handle,
                HWND_TOPMOST,
                bounds.x,
                bounds.y,
                bounds.width as i32,
                bounds.height as i32,
                SWP_NOACTIVATE,
            )
        }
        .map_err(|error| SurfaceError::Platform {
            operation: "SetWindowPos",
            detail: error.message(),
        })?;
        self.width = bounds.width;
        self.height = bounds.height;
        Ok(())
    }

    /// `UpdateLayeredWindow` consumes premultiplied alpha; the compositor works in straight alpha
    /// because that is what recognition and colour sampling need.
    fn premultiply(frame: &Frame) -> Vec<u8> {
        let mut pixels = Vec::with_capacity(frame.width() as usize * frame.height() as usize * 4);
        for y in 0..frame.height() {
            for x in 0..frame.width() {
                let [b, g, r, a] = frame.pixel(x, y);
                let scale = a as u32;
                pixels.extend_from_slice(&[
                    ((b as u32 * scale + 127) / 255) as u8,
                    ((g as u32 * scale + 127) / 255) as u8,
                    ((r as u32 * scale + 127) / 255) as u8,
                    a,
                ]);
            }
        }
        pixels
    }
}

impl OverlaySurface for LayeredOverlay {
    fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn properties(&self) -> SurfaceProperties {
        let mut affinity = 0u32;
        let excluded = unsafe { GetWindowDisplayAffinity(self.handle, &mut affinity) }.is_ok()
            && affinity == WDA_EXCLUDEFROMCAPTURE.0;
        SurfaceProperties {
            click_through: true,
            never_activates: true,
            hidden_from_switcher: true,
            excluded_from_capture: excluded && self.excluded_from_capture,
            topmost: true,
        }
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), SurfaceError> {
        self.move_to(Rect::new(self.origin.x, self.origin.y, width, height))
    }

    /// The damage list is honoured by the compositor upstream; a layered window is updated as one
    /// atomic bitmap, so partial uploads would tear against the desktop compositor.
    fn present(&mut self, frame: &Frame, _damage: &[Rect]) -> Result<(), SurfaceError> {
        if frame.width() != self.width || frame.height() != self.height {
            return Err(SurfaceError::SizeMismatch {
                width: frame.width(),
                height: frame.height(),
                surface_width: self.width,
                surface_height: self.height,
            });
        }

        let screen = ScreenDc::acquire()?;
        let mut dib = DibSection::create(&screen, self.width, self.height)?;
        dib.write(&Self::premultiply(frame));

        let size = windows::Win32::Foundation::SIZE {
            cx: self.width as i32,
            cy: self.height as i32,
        };
        let source = POINT { x: 0, y: 0 };
        let blend = windows::Win32::Graphics::Gdi::BLENDFUNCTION {
            BlendOp: windows::Win32::Graphics::Gdi::AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: windows::Win32::Graphics::Gdi::AC_SRC_ALPHA as u8,
        };

        if paint_window(self.handle, screen.dc, self.origin, size, dib.dc, source, blend).is_ok() {
            return Ok(());
        }
        // A window left in attribute mode rejects per-pixel updates. Clearing the layered
        // bit and putting it back is the documented way to make UpdateLayeredWindow work again.
        relayer(self.handle);
        paint_window(self.handle, screen.dc, self.origin, size, dib.dc, source, blend).map_err(|error| {
            SurfaceError::Platform {
                operation: "UpdateLayeredWindow",
                detail: error.message(),
            }
        })
    }

    fn clear(&mut self) -> Result<(), SurfaceError> {
        let empty = Frame::filled(self.width, self.height, CLEAR).map_err(|error| SurfaceError::Platform {
            operation: "clear",
            detail: error.to_string(),
        })?;
        self.present(&empty, &[Rect::new(0, 0, self.width, self.height)])
    }
}

impl Drop for LayeredOverlay {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.handle);
        }
    }
}

fn paint_window(
    handle: HWND,
    screen: HDC,
    origin: POINT,
    size: windows::Win32::Foundation::SIZE,
    source_dc: HDC,
    source: POINT,
    blend: windows::Win32::Graphics::Gdi::BLENDFUNCTION,
) -> windows::core::Result<()> {
    unsafe {
        UpdateLayeredWindow(
            handle,
            screen,
            Some(&origin),
            Some(&size),
            source_dc,
            Some(&source),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        )
    }
}

fn relayer(handle: HWND) {
    #[link(name = "user32")]
    extern "system" {
        fn GetWindowLongPtrW(hwnd: HWND, index: i32) -> isize;
        fn SetWindowLongPtrW(hwnd: HWND, index: i32, value: isize) -> isize;
    }
    const GWL_EXSTYLE: i32 = -20;
    let layered = WS_EX_LAYERED.0 as isize;
    unsafe {
        let style = GetWindowLongPtrW(handle, GWL_EXSTYLE);
        SetWindowLongPtrW(handle, GWL_EXSTYLE, style & !layered);
        SetWindowLongPtrW(handle, GWL_EXSTYLE, style | layered);
    }
}

fn tracing_excluded_fallback() {
    // The overlay crate stays free of a logging dependency; the caller inspects `properties()` and
    // decides whether to subtract the overlay rectangles from captured frames.
}

fn register_class() -> Result<(), SurfaceError> {
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(overlay_proc),
        hInstance: HINSTANCE::default(),
        lpszClassName: CLASS_NAME,
        ..Default::default()
    };
    // A duplicate registration is expected whenever a second overlay is created in one process.
    let _ = unsafe { RegisterClassExW(&class) };
    Ok(())
}

unsafe extern "system" fn overlay_proc(handle: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    DefWindowProcW(handle, message, wparam, lparam)
}

struct ScreenDc {
    dc: HDC,
}

impl ScreenDc {
    fn acquire() -> Result<Self, SurfaceError> {
        let dc = unsafe { GetDC(None) };
        if dc.is_invalid() {
            return Err(SurfaceError::Platform {
                operation: "GetDC",
                detail: "the desktop device context is unavailable".to_owned(),
            });
        }
        Ok(Self { dc })
    }
}

impl Drop for ScreenDc {
    fn drop(&mut self) {
        unsafe { ReleaseDC(None, self.dc) };
    }
}

struct DibSection {
    dc: HDC,
    bitmap: HBITMAP,
    bits: *mut c_void,
    len: usize,
}

impl DibSection {
    fn create(screen: &ScreenDc, width: u32, height: u32) -> Result<Self, SurfaceError> {
        let dc = unsafe { CreateCompatibleDC(screen.dc) };
        if dc.is_invalid() {
            return Err(SurfaceError::Platform {
                operation: "CreateCompatibleDC",
                detail: "no memory device context could be created".to_owned(),
            });
        }

        let info = BITMAPINFO {
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

        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap =
            unsafe { CreateDIBSection(screen.dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) }.map_err(|error| {
                SurfaceError::Platform {
                    operation: "CreateDIBSection",
                    detail: error.message(),
                }
            })?;

        unsafe { SelectObject(dc, bitmap) };
        Ok(Self {
            dc,
            bitmap,
            bits,
            len: width as usize * height as usize * 4,
        })
    }

    fn write(&mut self, pixels: &[u8]) {
        let count = pixels.len().min(self.len);
        // SAFETY: `bits` points at the DIB section allocated above, which holds exactly `len`
        // bytes and outlives this copy because it is owned by `self`.
        unsafe { std::ptr::copy_nonoverlapping(pixels.as_ptr(), self.bits as *mut u8, count) };
    }
}

impl Drop for DibSection {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.bitmap);
            let _ = DeleteDC(self.dc);
        }
    }
}
