//! Watches the foreground window and draws Russian over the text it is showing.
//!
//! The settings window is not the product. This loop is. It starts with the process, follows
//! whichever window the user is looking at, and gets out of the way when capture or translation
//! fails.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use lumen_capture::{capture_window_picture, capture_window_screen};
use lumen_core::{Frame, Rect};
use lumen_ocr::windows::WindowsOcr;
use lumen_ocr::OcrEngine;
use lumen_overlay::windows::LayeredOverlay;
use lumen_overlay::{Compositor, OverlayBlock, OverlayLayout, OverlayStyle, OverlaySurface};
use lumen_render::FontWeight;

use crate::model::Translator;

const MAX_OCR_WIDTH: u32 = 1280;
const NEW_LINES_PER_TICK: usize = 4;

pub fn run(bundled: Option<PathBuf>) {
    if let Err(error) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| loop_forever(bundled))) {
        let _ = log(&format!("loop panicked: {error:?}"));
    }
}

fn loop_forever(bundled: Option<PathBuf>) {
    let _ = log("live loop started");
    let mut paused = false;
    let _ = register_pause_hotkey();

    let mut status = StatusOverlay::new();
    status.show("Скачиваю модель перевода. Это один раз, дальше без интернета.");

    let mut log_file = open_log();
    let translator = Translator::load(bundled.as_deref(), &mut log_file);
    let mut translator = match translator {
        Ok(translator) => translator,
        Err(error) => {
            let _ = writeln!(log_file, "model: {error}");
            status.show("Модель перевода не скачалась. Проверьте интернет и запустите снова.");
            std::thread::sleep(Duration::from_secs(30));
            return;
        }
    };
    let mut ocr = match WindowsOcr::new() {
        Ok(ocr) => ocr,
        Err(error) => {
            let _ = writeln!(log_file, "ocr: {error}");
            status.show("Распознавание Windows недоступно на этом компьютере.");
            std::thread::sleep(Duration::from_secs(30));
            return;
        }
    };
    drop(status);

    let mut compositor = Compositor::new();
    let mut overlay: Option<LayeredOverlay> = None;
    let mut cache: HashMap<String, String> = HashMap::new();
    let mut seen = 0u64;

    loop {
        pump(&mut paused);
        if paused {
            clear(&mut overlay);
            std::thread::sleep(Duration::from_millis(150));
            continue;
        }

        let Some(hwnd) = foreground_other_process() else {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };

        let captured = match capture_window_picture(hwnd) {
            Ok(captured) => captured,
            Err(error) => {
                let _ = writeln!(log_file, "picture capture: {error}");
                clear(&mut overlay);
                std::thread::sleep(Duration::from_millis(40));
                match capture_window_screen(hwnd) {
                    Ok(captured) => captured,
                    Err(error) => {
                        let _ = writeln!(log_file, "screen capture: {error}");
                        clear(&mut overlay);
                        std::thread::sleep(Duration::from_millis(400));
                        continue;
                    }
                }
            }
        };
        let (frame, bounds) = captured;
        let fingerprint = fingerprint_of(hwnd, &frame);
        if fingerprint == seen {
            std::thread::sleep(Duration::from_millis(180));
            continue;
        }

        let (small, scale) = downscale(&frame);
        let lines = match ocr.recognize(&small, &[]) {
            Ok(lines) => lines,
            Err(error) => {
                let _ = writeln!(log_file, "ocr: {error}");
                clear(&mut overlay);
                std::thread::sleep(Duration::from_millis(400));
                continue;
            }
        };

        let mut fresh = 0usize;
        let mut blocks = Vec::new();
        for line in lines {
            let source = line.text.trim();
            if source.chars().all(|ch| !ch.is_alphabetic()) {
                continue;
            }
            let translated = if let Some(cached) = cache.get(source) {
                cached.clone()
            } else if fresh >= NEW_LINES_PER_TICK {
                continue;
            } else {
                fresh += 1;
                match translator.translate(source) {
                    Ok(text) if !text.trim().is_empty() => {
                        cache.insert(source.to_owned(), text.clone());
                        text
                    }
                    Ok(_) => continue,
                    Err(error) => {
                        let _ = writeln!(log_file, "translate: {error}");
                        continue;
                    }
                }
            };
            if cache.len() > 2000 {
                cache.clear();
            }
            let rect = scale_rect(line.bounds, scale, frame.width(), frame.height());
            if rect.height < 8 || rect.width < 8 {
                continue;
            }
            let size = (rect.height as f32 * 0.72).clamp(12.0, 42.0) as u32;
            blocks.push(
                OverlayBlock::new(rect, translated)
                    .with_font(size, FontWeight::Regular, false)
                    .with_colors([16, 16, 16, 230], [244, 244, 244, 255])
                    .with_confidence(line.confidence),
            );
        }

        if blocks.is_empty() {
            clear(&mut overlay);
            seen = fingerprint;
            std::thread::sleep(Duration::from_millis(180));
            continue;
        }

        if overlay.as_ref().is_none_or(|surface| surface.size() != (bounds.width, bounds.height)) {
            overlay = LayeredOverlay::create(bounds).ok();
        } else if let Some(surface) = overlay.as_mut() {
            if surface.move_to(bounds).is_err() {
                overlay = None;
            }
        }
        let Some(surface) = overlay.as_mut() else {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        let layout = OverlayLayout::new(OverlayStyle::Plate).with_blocks(blocks);
        let composition = compositor.compose(&frame, &layout);
        if surface.present(&composition.frame, &composition.damage).is_err() {
            overlay = None;
        }
        seen = fingerprint;
        std::thread::sleep(Duration::from_millis(160));
    }
}

fn foreground_other_process() -> Option<isize> {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }
    let mut process = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process)) };
    if process == 0 || process == unsafe { GetCurrentProcessId() } {
        return None;
    }
    Some(hwnd.0 as isize)
}

fn register_pause_hotkey() -> windows::core::Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_ALT, VK_T};
    unsafe { RegisterHotKey(None, 1, MOD_ALT, VK_T.0 as u32) }
}

fn pump(paused: &mut bool) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_HOTKEY,
    };

    unsafe {
        let mut message = MSG::default();
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            if message.message == WM_HOTKEY {
                *paused = !*paused;
                let _ = log(if *paused { "paused" } else { "resumed" });
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

fn clear(overlay: &mut Option<LayeredOverlay>) {
    if let Some(surface) = overlay.as_mut() {
        let _ = surface.clear();
    }
}

fn downscale(frame: &Frame) -> (Frame, f32) {
    if frame.width() <= MAX_OCR_WIDTH {
        return (frame.clone(), 1.0);
    }
    let scale = frame.width() as f32 / MAX_OCR_WIDTH as f32;
    let width = MAX_OCR_WIDTH;
    let height = ((frame.height() as f32 / scale).round() as u32).max(1);
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height {
        let source_y = ((y as f32 * scale) as u32).min(frame.height() - 1);
        for x in 0..width {
            let source_x = ((x as f32 * scale) as u32).min(frame.width() - 1);
            let pixel = frame.pixel(source_x, source_y);
            let offset = (y as usize * width as usize + x as usize) * 4;
            pixels[offset..offset + 4].copy_from_slice(&pixel);
        }
    }
    let small = Frame::packed(width, height, pixels).unwrap_or_else(|_| frame.clone());
    (small, scale)
}

fn scale_rect(rect: Rect, scale: f32, width: u32, height: u32) -> Rect {
    let scaled = Rect::new(
        (rect.x as f32 * scale) as i32,
        (rect.y as f32 * scale) as i32,
        ((rect.width as f32 * scale).round() as u32).max(1),
        ((rect.height as f32 * scale).round() as u32).max(1),
    );
    scaled
        .clamp_to(&Rect::new(0, 0, width, height))
        .unwrap_or(scaled)
}

fn fingerprint_of(hwnd: isize, frame: &Frame, bounds: Rect) -> u64 {
    let mut hash = hwnd as u64 ^ (bounds.x as u64) << 8 ^ bounds.y as u64;
    let step = (frame.width() / 24).max(8);
    let mut y = 0;
    while y < frame.height() {
        let mut x = 0;
        while x < frame.width() {
            let pixel = frame.pixel(x, y);
            hash = hash.wrapping_mul(16777619) ^ u64::from(pixel[0]);
            x += step;
        }
        y += step;
    }
    hash
}

struct StatusOverlay {
    surface: Option<LayeredOverlay>,
    compositor: Compositor,
}

impl StatusOverlay {
    fn new() -> Self {
        Self {
            surface: None,
            compositor: Compositor::new(),
        }
    }

    fn show(&mut self, text: &str) {
        let bounds = Rect::new(48, 48, 640, 56);
        if self.surface.is_none() {
            self.surface = LayeredOverlay::create(bounds).ok();
        }
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let frame = Frame::filled(bounds.width, bounds.height, [0, 0, 0, 0]).expect("status size");
        let block = OverlayBlock::new(Rect::new(0, 8, bounds.width, 40), text)
            .with_font(18, FontWeight::Regular, false)
            .with_colors([20, 20, 20, 240], [244, 244, 244, 255]);
        let layout = OverlayLayout::new(OverlayStyle::Plate).with_blocks(vec![block]);
        let composition = self.compositor.compose(&frame, &layout);
        let _ = surface.present(&composition.frame, &composition.damage);
    }
}

fn open_log() -> std::fs::File {
    let dir = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("DynoTranslate");
    let _ = std::fs::create_dir_all(&dir);
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("live.log"))
        .unwrap_or_else(|_| fallback_log())
}

fn fallback_log() -> std::fs::File {
    let path = std::env::temp_dir().join("dynotranslate-live.log");
    std::fs::File::create(path).expect("log")
}

fn log(message: &str) -> std::io::Result<()> {
    let mut file = open_log();
    writeln!(file, "{message}")
}
