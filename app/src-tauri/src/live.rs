//! Watches the foreground window and draws Russian over English text.
//!
//! The settings window used to be a separate mock. This loop is the product, and the window
//! only reports what the loop is actually doing.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lumen_capture::{capture_window_picture, capture_window_screen};
use lumen_core::{Frame, Rect};
use lumen_ocr::windows::WindowsOcr;
use lumen_ocr::OcrEngine;
use lumen_overlay::windows::LayeredOverlay;
use lumen_overlay::{Compositor, OverlayBlock, OverlayLayout, OverlayStyle, OverlaySurface};
use lumen_render::FontWeight;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::model::Translator;

const MAX_OCR_WIDTH: u32 = 1280;
const NEW_LINES_PER_TICK: usize = 4;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LiveStatus {
    pub phase: String,
    pub title: String,
    pub detail: String,
    pub watched: String,
    pub capture: String,
    pub sample: String,
    pub paused: bool,
    pub translated: u32,
}

#[derive(Clone, Serialize)]
struct PreviewEvent {
    image: String,
}

struct Inner {
    paused: AtomicBool,
    translated: AtomicU32,
    status: Mutex<LiveStatus>,
    preview: Mutex<String>,
}

#[derive(Clone)]
pub struct Control {
    inner: Arc<Inner>,
}

pub fn control() -> Control {
    let paused = load_paused();
    let status = if paused {
        view(
            "paused",
            "Пауза",
            "Перевод спрятан. Нажмите «Продолжить», когда будете готовы.",
        )
    } else {
        view(
            "starting",
            "Запускаю перевод",
            "Секунду. Сейчас будет видно, что программа делает.",
        )
    };
    Control {
        inner: Arc::new(Inner {
            paused: AtomicBool::new(paused),
            translated: AtomicU32::new(0),
            status: Mutex::new(status),
            preview: Mutex::new(String::new()),
        }),
    }
}

impl Control {
    pub fn status(&self) -> LiveStatus {
        let mut status = lock(&self.inner.status).clone();
        status.paused = self.inner.paused.load(Ordering::SeqCst);
        status.translated = self.inner.translated.load(Ordering::SeqCst);
        status
    }

    pub fn preview(&self) -> String {
        lock(&self.inner.preview).clone()
    }

    fn is_paused(&self) -> bool {
        self.inner.paused.load(Ordering::SeqCst)
    }
}

pub fn set_paused(control: &Control, app: &AppHandle, paused: bool) {
    control.inner.paused.store(paused, Ordering::SeqCst);
    save_paused(paused);
    let _ = log(if paused { "paused" } else { "resumed" });
    let current = control.status();
    if paused && !matches!(current.phase.as_str(), "downloading" | "preparing" | "starting" | "error") {
        let mut status = view(
            "paused",
            "Пауза",
            "Перевод спрятан. Нажмите «Продолжить» или Alt+T.",
        );
        status.watched = current.watched;
        status.capture = current.capture;
        status.sample = current.sample;
        publish(control, app, status);
    } else if !paused && current.phase == "paused" {
        publish(
            control,
            app,
            view(
                "waiting",
                "Смотрю окна",
                "Щёлкните по окну с английским текстом.",
            ),
        );
    } else {
        publish(control, app, current);
    }
}

pub fn run(app: AppHandle, control: Control, bundled: Option<PathBuf>) {
    let app_for_panic = app.clone();
    let control_for_panic = control.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        loop_forever(app, control, bundled);
    }));
    if let Err(error) = result {
        let _ = log(&format!("loop panicked: {error:?}"));
        publish(
            &control_for_panic,
            &app_for_panic,
            view(
                "error",
                "Перевод остановился",
                "Закройте программу и откройте её снова.",
            ),
        );
    }
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn loop_forever(app: AppHandle, control: Control, bundled: Option<PathBuf>) {
    let _ = log("live loop started");
    init_winrt();
    if let Err(error) = register_pause_hotkey() {
        let _ = log(&format!("pause hotkey: {error}"));
    }

    let mut corner = StatusOverlay::new();
    let mut log_file = open_log();
    let mut translator = loop {
        let loaded = {
            let mut report = |note: &str| {
                let (phase, title, detail) = describe_progress(note);
                corner.show(&title);
                publish(&control, &app, view(&phase, &title, &detail));
            };
            Translator::load(bundled.as_deref(), &mut log_file, &mut report)
        };
        match loaded {
            Ok(translator) => break translator,
            Err(error) => {
                let _ = writeln!(log_file, "model: {error}");
                corner.show("Перевод не скачался");
                publish(
                    &control,
                    &app,
                    view(
                        "error",
                        "Перевод не скачался",
                        "Нужен интернет. Пробую ещё раз. Не закрывайте программу.",
                    ),
                );
                wait(&control, &app, 8_000);
            }
        }
    };
    let mut ocr = loop {
        match WindowsOcr::new() {
            Ok(ocr) => break ocr,
            Err(error) => {
                let _ = writeln!(log_file, "ocr: {error}");
                corner.show("Не могу читать текст");
                publish(
                    &control,
                    &app,
                    view(
                        "error",
                        "Не могу читать текст",
                        "На этой Windows нет распознавания текста. Пробую ещё раз.",
                    ),
                );
                wait(&control, &app, 4_000);
            }
        }
    };
    corner.hide();

    let mut compositor = Compositor::new();
    let mut overlay: Option<LayeredOverlay> = None;
    let mut cache: HashMap<String, String> = HashMap::new();
    let mut seen = 0u64;
    let mut last_preview = Instant::now() - Duration::from_secs(2);
    let mut last_log = String::new();

    loop {
        pump(&control, &app);
        if control.is_paused() {
            clear(&mut overlay);
            let previous = control.status();
            let mut status = view(
                "paused",
                "Пауза",
                "Перевод спрятан. Нажмите «Продолжить» или Alt+T.",
            );
            status.watched = previous.watched;
            status.capture = previous.capture;
            status.sample = previous.sample;
            publish(&control, &app, status);
            std::thread::sleep(Duration::from_millis(150));
            continue;
        }

        match focus() {
            Focus::Ours => {
                clear(&mut overlay);
                set_preview(&control, &app, String::new());
                publish(
                    &control,
                    &app,
                    view(
                        "waiting",
                        "Откройте текст",
                        "Сейчас впереди это окно, его программа не переводит. Щёлкните по Блокноту или игре.",
                    ),
                );
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Focus::None => {
                clear(&mut overlay);
                set_preview(&control, &app, String::new());
                publish(
                    &control,
                    &app,
                    view("waiting", "Жду окно", "Щёлкните по окну с английским текстом."),
                );
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Focus::Other { hwnd, title } => {
                let name = title;
                let captured = match capture(hwnd) {
                    Ok(captured) => captured,
                    Err(error) => {
                        log_once(&mut log_file, &mut last_log, &format!("capture {name}: {error}"));
                        clear(&mut overlay);
                        set_preview(&control, &app, String::new());
                        let mut status = view(
                            "error",
                            "Не вижу это окно",
                            "Оно не отдаёт картинку. Запустите его в обычном окне, не на весь экран.",
                        );
                        status.watched = name;
                        publish(&control, &app, status);
                        std::thread::sleep(Duration::from_millis(400));
                        continue;
                    }
                };
                let (frame, bounds, method) = captured;
                if last_preview.elapsed() >= Duration::from_millis(800) {
                    set_preview(&control, &app, preview_data_url(&frame));
                    last_preview = Instant::now();
                }

                let fingerprint = fingerprint_of(hwnd, &frame, bounds);
                if fingerprint == seen {
                    let phase = control.status().phase;
                    if phase == "translating" || phase == "watching" {
                        std::thread::sleep(Duration::from_millis(180));
                        continue;
                    }
                }

                let mut looking = view("watching", "Смотрю", "Ищу английский текст.");
                looking.watched = name.clone();
                looking.capture = method.to_owned();
                publish(&control, &app, looking);

                let (small, scale) = downscale(&frame);
                let lines = match ocr.recognize(&small, &[]) {
                    Ok(lines) => lines,
                    Err(error) => {
                        log_once(&mut log_file, &mut last_log, &format!("ocr: {error}"));
                        clear(&mut overlay);
                        let mut status = view(
                            "error",
                            "Не могу прочитать текст",
                            "Распознавание сбилось. Пробую ещё раз.",
                        );
                        status.watched = name;
                        status.capture = method.to_owned();
                        publish(&control, &app, status);
                        std::thread::sleep(Duration::from_millis(400));
                        continue;
                    }
                };

                let mut fresh = 0usize;
                let mut blocks = Vec::new();
                let mut sample = String::new();
                let mut saw_other = false;
                let mut saw_cyrillic = false;
                let mut saw_latin = false;
                for line in lines {
                    let source = line.text.trim();
                    if source.chars().all(|ch| !ch.is_alphabetic()) {
                        continue;
                    }
                    match script_of(source) {
                        Script::Other => {
                            saw_other = true;
                            continue;
                        }
                        Script::Cyrillic => {
                            saw_cyrillic = true;
                            continue;
                        }
                        Script::None => continue,
                        Script::Latin => saw_latin = true,
                    }
                    let key = clip(source, 160);
                    let translated = if let Some(cached) = cache.get(&key) {
                        cached.clone()
                    } else if fresh >= NEW_LINES_PER_TICK {
                        continue;
                    } else {
                        fresh += 1;
                        match translator.translate(&key) {
                            Ok(text) if has_cyrillic(&text) => {
                                cache.insert(key.clone(), text.clone());
                                control.inner.translated.fetch_add(1, Ordering::SeqCst);
                                text
                            }
                            Ok(_) => continue,
                            Err(error) => {
                                log_once(&mut log_file, &mut last_log, &format!("translate: {error}"));
                                continue;
                            }
                        }
                    };
                    if cache.len() > 2000 {
                        cache.clear();
                    }
                    if sample.is_empty() {
                        sample = format!("{} → {}", clip(&key, 42), clip(&translated, 42));
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
                    let detail = if saw_other {
                        "Текст не на английском. Сейчас перевожу английский на русский и пропускаю остальное."
                    } else if saw_cyrillic && !saw_latin {
                        "Текст уже на русском. Переводить нечего."
                    } else if saw_latin {
                        "Английский вижу, перевод ещё не готов. Пробую дальше."
                    } else {
                        "Английского текста пока не вижу. Напишите фразу крупными буквами."
                    };
                    let mut status = view("watching", "Смотрю", detail);
                    status.watched = name;
                    status.capture = method.to_owned();
                    publish(&control, &app, status);
                    seen = fingerprint;
                    std::thread::sleep(Duration::from_millis(180));
                    continue;
                }

                let resized = match overlay.as_ref() {
                    None => true,
                    Some(surface) => surface.size() != (bounds.width, bounds.height),
                };
                if resized {
                    overlay = LayeredOverlay::create(bounds).ok();
                } else if let Some(surface) = overlay.as_mut() {
                    if surface.move_to(bounds).is_err() {
                        overlay = None;
                    }
                }
                let Some(surface) = overlay.as_mut() else {
                    let mut status = view(
                        "error",
                        "Не могу показать перевод",
                        "Окно нашлось, но нарисовать поверх него не получилось. Пробую ещё раз.",
                    );
                    status.watched = name;
                    status.capture = method.to_owned();
                    status.sample = sample;
                    publish(&control, &app, status);
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                };
                let layout = OverlayLayout::new(OverlayStyle::Plate).with_blocks(blocks);
                let composition = compositor.compose(&frame, &layout);
                if surface.present(&composition.frame, &composition.damage).is_err() {
                    overlay = None;
                    let mut status = view(
                        "error",
                        "Не могу показать перевод",
                        "Текст переведён, но не лёг на окно. Пробую ещё раз.",
                    );
                    status.watched = name;
                    status.capture = method.to_owned();
                    status.sample = sample;
                    publish(&control, &app, status);
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                }
                let mut status = view("translating", "Перевожу", "Английский на экране становится русским.");
                status.watched = name;
                status.capture = method.to_owned();
                status.sample = sample;
                publish(&control, &app, status);
                seen = fingerprint;
                std::thread::sleep(Duration::from_millis(160));
            }
        }
    }
}

fn describe_progress(note: &str) -> (String, String, String) {
    if let Some((name, mb)) = note.split_once(':') {
        let part = match name {
            "encoder.onnx" => "Первая часть",
            "decoder.onnx" => "Вторая часть",
            _ => "Файл",
        };
        return (
            "downloading".to_owned(),
            "Скачиваю перевод".to_owned(),
            format!("{part}: скачано {mb} МБ. Не закрывайте программу."),
        );
    }
    let (phase, title, detail) = match note {
        "encoder.onnx" => (
            "downloading",
            "Скачиваю перевод",
            "Это один раз. Нужен интернет. Не закрывайте программу.",
        ),
        "decoder.onnx" => (
            "downloading",
            "Скачиваю перевод",
            "Вторая часть. Не закрывайте программу.",
        ),
        "tokenizer.json" => ("downloading", "Скачиваю перевод", "Словарь. Уже почти."),
        "sessions" => (
            "preparing",
            "Готовлю перевод",
            "Первый запуск может занять минуту. Следующие будут быстрее.",
        ),
        _ => (
            "downloading",
            "Скачиваю перевод",
            "Это один раз. Нужен интернет.",
        ),
    };
    (phase.to_owned(), title.to_owned(), detail.to_owned())
}

fn view(phase: &str, title: &str, detail: &str) -> LiveStatus {
    LiveStatus {
        phase: phase.to_owned(),
        title: title.to_owned(),
        detail: detail.to_owned(),
        watched: String::new(),
        capture: String::new(),
        sample: String::new(),
        paused: false,
        translated: 0,
    }
}

fn publish(control: &Control, app: &AppHandle, mut next: LiveStatus) {
    next.paused = control.is_paused();
    next.translated = control.inner.translated.load(Ordering::SeqCst);
    let mut current = lock(&control.inner.status);
    if next.sample.is_empty() && next.watched == current.watched {
        next.sample = current.sample.clone();
    }
    if *current == next {
        return;
    }
    *current = next.clone();
    drop(current);
    let _ = log(&format!("{} | {} | {}", next.phase, next.watched, next.detail));
    let _ = app.emit("live-status", next);
}

fn set_preview(control: &Control, app: &AppHandle, image: String) {
    let mut current = lock(&control.inner.preview);
    if *current == image {
        return;
    }
    *current = image.clone();
    drop(current);
    let _ = app.emit("live-preview", PreviewEvent { image });
}

fn capture(hwnd: isize) -> Result<(Frame, Rect, &'static str), String> {
    match capture_window_picture(hwnd) {
        Ok((frame, bounds)) => Ok((frame, bounds, "Само окно")),
        Err(picture) => match capture_window_screen(hwnd) {
            Ok((frame, bounds)) => Ok((frame, bounds, "Снимок экрана")),
            Err(screen) => Err(format!("{picture}; {screen}")),
        },
    }
}

enum Focus {
    Other { hwnd: isize, title: String },
    Ours,
    None,
}

fn focus() -> Focus {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() || is_overlay_class(hwnd) {
        return Focus::None;
    }
    let mut process = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process)) };
    if process == 0 {
        return Focus::None;
    }
    if process == unsafe { GetCurrentProcessId() } {
        return Focus::Ours;
    }
    Focus::Other {
        hwnd: hwnd.0 as isize,
        title: window_title(hwnd),
    }
}

fn window_title(hwnd: windows::Win32::Foundation::HWND) -> String {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;

    let mut buffer = [0u16; 256];
    let len = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    if len <= 0 {
        return "окно без названия".to_owned();
    }
    let text = String::from_utf16_lossy(&buffer[..len as usize]);
    let text = text.trim();
    if text.is_empty() {
        "окно без названия".to_owned()
    } else {
        clip(text, 80)
    }
}

enum Script {
    Latin,
    Cyrillic,
    Other,
    None,
}

fn script_of(text: &str) -> Script {
    let mut latin = 0usize;
    let mut cyrillic = 0usize;
    let mut other = 0usize;
    for ch in text.chars() {
        if !ch.is_alphabetic() {
            continue;
        }
        if ch.is_ascii_alphabetic() {
            latin += 1;
        } else if ('\u{0400}'..='\u{04FF}').contains(&ch) {
            cyrillic += 1;
        } else {
            other += 1;
        }
    }
    if latin + cyrillic + other == 0 {
        Script::None
    } else if other > latin && other >= cyrillic {
        Script::Other
    } else if cyrillic > latin {
        Script::Cyrillic
    } else {
        Script::Latin
    }
}

fn has_cyrillic(text: &str) -> bool {
    text.chars().any(|ch| ('\u{0400}'..='\u{04FF}').contains(&ch))
}

fn clip(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        trimmed.to_owned()
    } else {
        format!("{}…", trimmed.chars().take(limit).collect::<String>())
    }
}

fn preview_data_url(frame: &Frame) -> String {
    let (width, height, scale) = preview_size(frame.width(), frame.height());
    let row_stride = ((width * 3 + 3) / 4) * 4;
    let pixel_bytes = row_stride * height;
    let file_size = 54u32 + pixel_bytes;
    let mut bmp = vec![0u8; file_size as usize];
    bmp[0] = b'B';
    bmp[1] = b'M';
    bmp[2..6].copy_from_slice(&file_size.to_le_bytes());
    bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
    bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
    bmp[18..22].copy_from_slice(&width.to_le_bytes());
    bmp[22..26].copy_from_slice(&height.to_le_bytes());
    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
    for y in 0..height {
        let source_y = ((y as f32 * scale) as u32).min(frame.height() - 1);
        let dest_y = height - 1 - y;
        let row = 54 + dest_y as usize * row_stride as usize;
        for x in 0..width {
            let source_x = ((x as f32 * scale) as u32).min(frame.width() - 1);
            let pixel = frame.pixel(source_x, source_y);
            let offset = row + x as usize * 3;
            bmp[offset] = pixel[0];
            bmp[offset + 1] = pixel[1];
            bmp[offset + 2] = pixel[2];
        }
    }
    format!("data:image/bmp;base64,{}", base64(&bmp))
}

fn preview_size(width: u32, height: u32) -> (u32, u32, f32) {
    let mut scale = 1.0f32;
    if width > 280 {
        scale = width as f32 / 280.0;
    }
    let mut out_w = ((width as f32 / scale).round() as u32).max(1);
    let mut out_h = ((height as f32 / scale).round() as u32).max(1);
    if out_h > 160 {
        scale *= out_h as f32 / 160.0;
        out_w = ((width as f32 / scale).round() as u32).max(1);
        out_h = 160;
    }
    (out_w, out_h, scale)
}

fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut index = 0;
    while index + 3 <= data.len() {
        let value = (u32::from(data[index]) << 16)
            | (u32::from(data[index + 1]) << 8)
            | u32::from(data[index + 2]);
        out.push(TABLE[((value >> 18) & 63) as usize] as char);
        out.push(TABLE[((value >> 12) & 63) as usize] as char);
        out.push(TABLE[((value >> 6) & 63) as usize] as char);
        out.push(TABLE[(value & 63) as usize] as char);
        index += 3;
    }
    let rest = data.len() - index;
    if rest == 1 {
        let value = u32::from(data[index]) << 16;
        out.push(TABLE[((value >> 18) & 63) as usize] as char);
        out.push(TABLE[((value >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rest == 2 {
        let value = (u32::from(data[index]) << 16) | (u32::from(data[index + 1]) << 8);
        out.push(TABLE[((value >> 18) & 63) as usize] as char);
        out.push(TABLE[((value >> 12) & 63) as usize] as char);
        out.push(TABLE[((value >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}

fn register_pause_hotkey() -> windows::core::Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_ALT, MOD_NOREPEAT, VK_T};
    unsafe { RegisterHotKey(None, 1, MOD_ALT | MOD_NOREPEAT, VK_T.0 as u32) }
}

fn init_winrt() {
    let mode = windows::Win32::System::WinRT::RO_INIT_MULTITHREADED;
    if let Err(error) = unsafe { windows::Win32::System::WinRT::RoInitialize(mode) } {
        let _ = log(&format!("winrt init: {error}"));
    }
}

fn is_overlay_class(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
    let mut buffer = [0u16; 64];
    let written = unsafe { GetClassNameW(hwnd, &mut buffer) };
    if written <= 0 {
        return false;
    }
    let name = String::from_utf16_lossy(&buffer[..written as usize]);
    name == "LumenOverlaySurface"
}

fn pump(control: &Control, app: &AppHandle) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_HOTKEY,
    };

    unsafe {
        let mut message = MSG::default();
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            if message.message == WM_HOTKEY {
                set_paused(control, app, !control.is_paused());
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

fn wait(control: &Control, app: &AppHandle, millis: u64) {
    let steps = (millis / 100).max(1);
    for _ in 0..steps {
        pump(control, app);
        std::thread::sleep(Duration::from_millis(100));
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
    scaled.clamp_to(&Rect::new(0, 0, width, height)).unwrap_or(scaled)
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
        let bounds = Rect::new(48, 48, 520, 56);
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

    fn hide(&mut self) {
        if let Some(surface) = self.surface.as_mut() {
            let _ = surface.clear();
        }
        self.surface = None;
    }
}

fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("DynoTranslate")
}

fn load_paused() -> bool {
    let Ok(text) = std::fs::read_to_string(data_dir().join("state.json")) else {
        return false;
    };
    text.contains("\"paused\":true") || text.contains("\"paused\": true")
}

fn save_paused(paused: bool) {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("state.json"), format!("{{\"paused\":{paused}}}\n"));
}

fn open_log() -> std::fs::File {
    let dir = data_dir();
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

fn log_once(file: &mut std::fs::File, last: &mut String, message: &str) {
    if last == message {
        return;
    }
    *last = message.to_owned();
    let _ = writeln!(file, "{message}");
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}
