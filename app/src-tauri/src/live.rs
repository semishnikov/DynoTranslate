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

use lumen_capture::{capture_window_picture, capture_window_screen, enumerate_targets};
use lumen_core::{Frame, Rect};
use lumen_overlay::windows::LayeredOverlay;
use lumen_overlay::{Compositor, OverlayBlock, OverlayLayout, OverlayStyle, OverlaySurface};
use lumen_render::FontWeight;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::model::Translator;
use crate::readtext::Reader;

const MAX_OCR_WIDTH: u32 = 1280;
/// Lines translated per tick: eight keeps a video-paced screen draining its queue while a
/// 30 ms recompose cadence streams the plates in instead of one long blocking batch.
const NEW_LINES_PER_TICK: usize = 8;
/// Reads below this confidence are the recogniser guessing at chrome, watermarks or stylised
/// art. The journal prints every skip with its number, so the threshold stays honest: in the
/// owner's log every garbage plate came from a read at 0.73 or below, every good translation
/// from 0.84 and up.
const MIN_LINE_CONFIDENCE: f32 = 0.75;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LiveStatus {
    pub phase: String,
    pub title: String,
    pub detail: String,
    pub watched: String,
    pub capture: String,
    pub sample: String,
    /// Where the stage-by-stage journal lives, so the owner can attach it to a bug report.
    pub log_path: String,
    pub paused: bool,
    pub translated: u32,
}

#[derive(Clone, Serialize)]
struct PreviewEvent {
    image: String,
}

#[derive(Clone)]
struct Chosen {
    id: u64,
    title: String,
    process: String,
}

struct Inner {
    paused: AtomicBool,
    translated: AtomicU32,
    restored: AtomicBool,
    status: Mutex<LiveStatus>,
    preview: Mutex<String>,
    choice: Mutex<Option<Chosen>>,
    sticky: Mutex<Option<Chosen>>,
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
            "waiting",
            "Выберите окно",
            "Нажмите на него в списке. Выводить вперёд не нужно.",
        )
    };
    Control {
        inner: Arc::new(Inner {
            paused: AtomicBool::new(paused),
            translated: AtomicU32::new(0),
            restored: AtomicBool::new(false),
            status: Mutex::new(status),
            preview: Mutex::new(String::new()),
            choice: Mutex::new(None),
            sticky: Mutex::new(None),
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
    if paused
        && !matches!(
            current.phase.as_str(),
            "downloading" | "preparing" | "starting" | "error"
        )
    {
        let mut status = view("paused", "Пауза", "Перевод спрятан. Нажмите «Продолжить» или Alt+T.");
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
                "Выберите окно",
                "Нажмите на него в списке. Выводить вперёд не нужно.",
            ),
        );
    } else {
        publish(control, app, current);
    }
}

pub fn run(app: AppHandle, control: Control, bundled: Option<PathBuf>) {
    loop {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            loop_forever(app.clone(), control.clone(), bundled.clone());
        }));
        let Err(error) = result else {
            continue;
        };
        let text = panic_text(error.as_ref());
        let _ = log(&format!("loop panicked: {text}"));
        let mut status = view(
            "error",
            "Перевод споткнулся",
            "Список окон работает. Выберите нужное — попробую ещё раз.",
        );
        if !text.is_empty() {
            status.detail = format!("{}. {text}", status.detail);
        }
        publish(&control, &app, status);
        std::thread::sleep(Duration::from_secs(2));
    }
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn loop_forever(app: AppHandle, control: Control, bundled: Option<PathBuf>) {
    let _ = log("live loop started");
    init_winrt();
    if let Err(error) = register_pause_hotkey() {
        let _ = log(&format!("pause hotkey: {error}"));
    }

    let mut log_file = open_log();
    let mut translator = loop {
        let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut report = |note: &str| {
                let (phase, title, detail) = describe_progress(note);
                publish(&control, &app, view(&phase, &title, &detail));
            };
            Translator::load(bundled.as_deref(), &mut log_file, &mut report)
        }));
        match loaded {
            Ok(Ok(translator)) => break translator,
            Ok(Err(error)) => {
                let _ = writeln!(log_file, "model: {error}");
                let downloading = error.contains("download") || error.contains("http");
                let (title, detail) = if downloading {
                    (
                        "Перевод не скачался",
                        "Нужен интернет. Список окон уже можно нажимать. Пробую ещё раз.",
                    )
                } else {
                    (
                        "Перевод не открылся",
                        "Словарь перевода починю и открою снова. Список окон можно нажимать.",
                    )
                };
                publish(&control, &app, view("error", title, detail));
                wait(&control, &app, 8_000);
            }
            Err(error) => {
                let text = panic_text(error.as_ref());
                let _ = writeln!(log_file, "model panic: {text}");
                publish(
                    &control,
                    &app,
                    view(
                        "error",
                        "Перевод не запустился",
                        "Список окон работает. Пробую ещё раз, английский текст не пропал.",
                    ),
                );
                wait(&control, &app, 4_000);
            }
        }
    };
    let mut reader = loop {
        let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut report = |note: &str| {
                let (phase, title, detail) = describe_progress(note);
                publish(&control, &app, view(&phase, &title, &detail));
            };
            Reader::load(&mut log_file, &mut report)
        }));
        match loaded {
            Ok(Ok(reader)) => break reader,
            Ok(Err(error)) => {
                let _ = writeln!(log_file, "reader: {error}");
                let downloading = error.contains("download") || error.contains("http");
                let (title, detail) = if downloading {
                    (
                        "Распознавание не скачалось",
                        "Нужен интернет. Список окон уже можно нажимать. Пробую ещё раз.",
                    )
                } else {
                    ("Распознавание не открылось", "Список окон работает. Пробую ещё раз.")
                };
                publish(&control, &app, view("error", title, detail));
                wait(&control, &app, 8_000);
            }
            Err(error) => {
                let text = panic_text(error.as_ref());
                let _ = writeln!(log_file, "reader panic: {text}");
                publish(
                    &control,
                    &app,
                    view(
                        "error",
                        "Распознавание споткнулось",
                        "Список окон работает. Пробую ещё раз.",
                    ),
                );
                wait(&control, &app, 4_000);
            }
        }
    };

    let mut compositor = match std::panic::catch_unwind(Compositor::new) {
        Ok(compositor) => Some(compositor),
        Err(error) => {
            let text = panic_text(error.as_ref());
            let _ = writeln!(log_file, "overlay panic: {text}");
            publish(
                &control,
                &app,
                view(
                    "error",
                    "Не могу рисовать поверх окон",
                    "Текст всё равно покажу в этом окне. Выберите окно в списке.",
                ),
            );
            None
        }
    };
    let mut overlay: Option<LayeredOverlay> = None;
    let mut cache: HashMap<String, String> = HashMap::new();
    let mut last_sig: Vec<u8> = Vec::new();
    let mut last_hwnd = 0isize;
    let mut quiet_until = Instant::now();
    let mut held = String::new();
    let mut last_preview = Instant::now() - Duration::from_secs(2);
    let mut last_log = String::new();
    let mut last_scene_key = String::new();

    loop {
        pump(&control, &app);
        if control.is_paused() {
            clear(&mut overlay);
            let previous = control.status();
            let mut status = view("paused", "Пауза", "Перевод спрятан. Нажмите «Продолжить» или Alt+T.");
            status.watched = previous.watched;
            status.capture = previous.capture;
            status.sample = previous.sample;
            publish(&control, &app, status);
            std::thread::sleep(Duration::from_millis(150));
            continue;
        }

        let Some((hwnd, name)) = resolve_target(&control) else {
            clear(&mut overlay);
            publish(
                &control,
                &app,
                view(
                    "waiting",
                    "Выберите окно",
                    "Нажмите на него в списке. Выводить вперёд не нужно.",
                ),
            );
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        {
            let captured = match capture(hwnd) {
                Ok(captured) => captured,
                Err(error) => {
                    log_once(&mut log_file, &mut last_log, &format!("capture {name}: {error}"));
                    clear(&mut overlay);
                    set_preview(&control, &app, String::new());
                    let mut status = view(
                            "error",
                            "Не вижу это окно",
                            "Оно не отдаёт картинку. Выберите другое в списке или откройте его обычным окном, не на весь экран.",
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

            let sig = picture_sig(&frame);
            if hwnd != last_hwnd {
                held.clear();
                last_sig.clear();
            }
            let stable = hwnd == last_hwnd && picture_same(&last_sig, &sig);
            if stable && Instant::now() < quiet_until {
                std::thread::sleep(Duration::from_millis(180));
                continue;
            }
            if stable {
                if let Some(surface) = overlay.as_mut() {
                    if surface.size() == (bounds.width, bounds.height) {
                        let _ = surface.move_to(bounds);
                        std::thread::sleep(Duration::from_millis(180));
                        continue;
                    }
                }
            }

            if held.is_empty() {
                let mut looking = view("watching", "Читаю", "Своё распознавание, Windows для этого не нужна.");
                looking.watched = name.clone();
                looking.capture = method.to_owned();
                publish(&control, &app, looking);
            }

            let (small, scale) = downscale(&frame);
            let ocr_started = Instant::now();
            let lines = match reader.read(&small) {
                Ok(lines) => lines,
                Err(error) => {
                    log_once(&mut log_file, &mut last_log, &format!("ocr: {error}"));
                    if !held.is_empty() {
                        quiet_until = Instant::now() + Duration::from_millis(800);
                        std::thread::sleep(Duration::from_millis(200));
                        continue;
                    }
                    clear(&mut overlay);
                    let mut status = view(
                        "error",
                        "Не могу прочитать текст",
                        &format!("Своё распознавание сбилось. Пробую ещё раз. {}", clip(&error, 90)),
                    );
                    status.watched = name;
                    status.capture = method.to_owned();
                    publish(&control, &app, status);
                    std::thread::sleep(Duration::from_millis(400));
                    continue;
                }
            };

            let mut fresh = 0usize;
            let mut pending = false;
            let mut blocks = Vec::new();
            let mut sample = String::new();
            let mut sample_score = 0usize;
            let mut saw_other = false;
            let mut saw_cyrillic = false;
            let mut saw_latin = false;
            // The journal dumps every stage when the reading changes and stays quiet while the
            // same text is on screen, so the file the owner sends back is readable.
            let scene_key = lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("|");
            let dump = scene_key != last_scene_key;
            if dump {
                last_scene_key = scene_key;
                let _ = writeln!(
                    log_file,
                    "{} tick window={:?} frame={}x{} ocr={}ms lines={}",
                    stamp(),
                    name,
                    frame.width(),
                    frame.height(),
                    ocr_started.elapsed().as_millis(),
                    lines.len()
                );
                for line in &lines {
                    let b = line.bounds;
                    let _ = writeln!(
                        log_file,
                        "{}   line conf={:.2} rect=({},{},{}x{}) text={:?}",
                        stamp(),
                        line.confidence,
                        b.x,
                        b.y,
                        b.width,
                        b.height,
                        line.text
                    );
                }
            }
            for line in lines {
                let source = line.text.trim();
                if source.chars().all(|ch| !ch.is_alphabetic()) {
                    continue;
                }
                if has_cyrillic(source) {
                    // Window chrome of a Russian system — titles, menus, status lines — is
                    // already in the target language; translating it only draws garbage over
                    // text the user can read.
                    if dump {
                        let _ = writeln!(log_file, "{} skip reason=russian text={:?}", stamp(), source);
                    }
                    saw_cyrillic = true;
                    continue;
                }
                if !looks_like_words(source) {
                    // Recognition noise from borders and icons has no vowel-bearing words;
                    // never translate or cover something that is not really text.
                    if dump {
                        let _ = writeln!(log_file, "{} skip reason=noise text={:?}", stamp(), source);
                    }
                    continue;
                }
                if line.confidence < MIN_LINE_CONFIDENCE {
                    if dump {
                        let _ = writeln!(
                            log_file,
                            "{} skip reason=lowconf conf={:.2} text={:?}",
                            stamp(),
                            line.confidence,
                            source
                        );
                    }
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
                let key = clip(source, 180);
                let translated = if let Some(cached) = cache.get(&key) {
                    if dump {
                        let _ = writeln!(log_file, "{} reuse {:?} -> {:?}", stamp(), key, cached);
                    }
                    cached.clone()
                } else if fresh >= NEW_LINES_PER_TICK {
                    pending = true;
                    continue;
                } else {
                    fresh += 1;
                    let started = Instant::now();
                    match translate_fully(&mut translator, &key) {
                        Ok(text) if has_cyrillic(&text) => {
                            let _ = writeln!(
                                log_file,
                                "{} translate ms={} {:?} -> {:?}",
                                stamp(),
                                started.elapsed().as_millis(),
                                key,
                                text
                            );
                            cache.insert(key.clone(), text.clone());
                            control.inner.translated.fetch_add(1, Ordering::SeqCst);
                            text
                        }
                        Ok(_) => continue,
                        Err(error) => {
                            let _ = writeln!(log_file, "{} translate error {:?}", stamp(), error);
                            continue;
                        }
                    }
                };
                if cache.len() > 2000 {
                    cache.clear();
                }
                let score = translated.chars().count();
                if score > sample_score {
                    sample_score = score;
                    sample = format!("{} → {}", clip(&key, 120), clip(&translated, 120));
                }
                // The plate is exactly the box the original occupies: detection already grows
                // its boxes by a couple of pixels, so any extra padding or widening only made
                // plates overlap their neighbours and stick out of the bubble.
                let rect = scale_rect(line.bounds, scale, frame.width(), frame.height());
                if rect.height < 8 || rect.width < 8 {
                    continue;
                }
                let size = (rect.height as f32 * 0.72).clamp(12.0, 42.0) as u32;
                let (background, foreground) = plate_colors(&frame, &rect);
                if dump {
                    let _ = writeln!(
                        log_file,
                        "{} plate rect=({},{},{}x{}) font={} bg=[{},{},{}] fg=[{},{},{}] text={:?}",
                        stamp(),
                        rect.x,
                        rect.y,
                        rect.width,
                        rect.height,
                        size,
                        background[0],
                        background[1],
                        background[2],
                        foreground[0],
                        foreground[1],
                        foreground[2],
                        translated
                    );
                }
                blocks.push(
                    OverlayBlock::new(rect, translated)
                        .with_font(size, FontWeight::Regular, false)
                        .with_colors(background, foreground)
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
                last_sig = sig.clone();
                last_hwnd = hwnd;
                held.clear();
                std::thread::sleep(Duration::from_millis(180));
                continue;
            }

            let layout = OverlayLayout::new(OverlayStyle::Plate).with_blocks(blocks);
            let Some(compositor) = compositor.as_mut() else {
                let mut status = view(
                    "translating",
                    "Перевожу",
                    "Русский текст виден в этом окне. Поверх чужого окна нарисовать не получилось.",
                );
                status.watched = name;
                status.capture = method.to_owned();
                status.sample = sample.clone();
                publish(&control, &app, status);
                held = sample;
                last_sig = sig.clone();
                last_hwnd = hwnd;
                std::thread::sleep(Duration::from_millis(160));
                continue;
            };
            let present_started = Instant::now();
            let composition = compositor.compose(&frame, &layout);
            if let Err(error) = show_on(&mut overlay, bounds, &composition.frame, &composition.damage) {
                log_once(&mut log_file, &mut last_log, &format!("present: {error}"));
                overlay = None;
                let mut status = view(
                    "translating",
                    "Перевожу",
                    "Русский текст ниже. На само окно он ещё не лёг, пробую без мигания.",
                );
                status.watched = name;
                status.capture = method.to_owned();
                status.sample = sample.clone();
                publish(&control, &app, status);
                held = sample;
                last_sig = sig.clone();
                last_hwnd = hwnd;
                quiet_until = Instant::now() + Duration::from_secs(2);
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            let mut status = view("translating", "Перевожу", "Английский на экране становится русским.");
            status.watched = name;
            status.capture = method.to_owned();
            status.sample = sample.clone();
            publish(&control, &app, status);
            if dump {
                let _ = writeln!(
                    log_file,
                    "{} present ok blocks={} ms={}",
                    stamp(),
                    layout.blocks.len(),
                    present_started.elapsed().as_millis()
                );
            }
            held = sample;
            if !pending {
                last_sig = sig.clone();
                last_hwnd = hwnd;
            }
            std::thread::sleep(Duration::from_millis(if pending { 30 } else { 160 }));
        }
    }
}

fn describe_progress(note: &str) -> (String, String, String) {
    if let Some((name, mb)) = note.split_once(':') {
        if name.starts_with("ocr-") {
            return (
                "downloading".to_owned(),
                "Скачиваю распознавание".to_owned(),
                format!("Скачано {mb} МБ. Это один раз, Windows для этого не нужна."),
            );
        }
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
        "ocr-det.onnx" | "ocr-rec.onnx" => (
            "downloading",
            "Скачиваю распознавание",
            "Это один раз. Нужен интернет. Windows для этого не нужна.",
        ),
        "ocr-sessions" => (
            "preparing",
            "Готовлю распознавание",
            "Своё, не из Windows. Первый раз может занять минуту.",
        ),
        _ => ("downloading", "Скачиваю перевод", "Это один раз. Нужен интернет."),
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
        log_path: log_file_path().to_string_lossy().into_owned(),
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

/// Real text has at least one word of two or more letters with a vowel in it. Misreads of
/// icons, borders and Russian chrome ("HHBLM", "KT I") never do, so they stay untranslated.
fn looks_like_words(text: &str) -> bool {
    let mut run = 0usize;
    let mut vowel = false;
    let mut done = false;
    for ch in text.chars() {
        if ch.is_ascii_alphabetic() {
            run += 1;
            vowel |= matches!(ch.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u');
        } else {
            done |= run >= 2 && vowel;
            run = 0;
            vowel = false;
        }
    }
    done || run >= 2 && vowel
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
    if frame.width() == 0 || frame.height() == 0 {
        return String::new();
    }
    let (width, height, scale) = preview_size(frame.width(), frame.height());
    let row_stride = (width * 3).div_ceil(4) * 4;
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
        let value = (u32::from(data[index]) << 16) | (u32::from(data[index + 1]) << 8) | u32::from(data[index + 2]);
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
    if frame.width() == 0 || frame.height() == 0 || frame.width() <= MAX_OCR_WIDTH {
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

fn picture_sig(frame: &Frame) -> Vec<u8> {
    const COLS: u32 = 48;
    const ROWS: u32 = 32;
    if frame.width() == 0 || frame.height() == 0 {
        return Vec::new();
    }
    let mut sig = Vec::with_capacity((COLS * ROWS) as usize);
    for row in 0..ROWS {
        let y0 = row * frame.height() / ROWS;
        let y1 = ((row + 1) * frame.height() / ROWS).max(y0 + 1).min(frame.height());
        for col in 0..COLS {
            let x0 = col * frame.width() / COLS;
            let x1 = ((col + 1) * frame.width() / COLS).max(x0 + 1).min(frame.width());
            let mut sum = 0u32;
            let mut count = 0u32;
            let step_x = ((x1 - x0) / 3).max(1);
            let step_y = ((y1 - y0) / 3).max(1);
            let mut y = y0;
            while y < y1 {
                let mut x = x0;
                while x < x1 {
                    let pixel = frame.pixel(x, y);
                    sum += u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2]);
                    count += 1;
                    x += step_x;
                }
                y += step_y;
            }
            sig.push((sum / count.max(1) / 3) as u8);
        }
    }
    sig
}

/// A caret blink changes one or two cells. A new letter changes a cell by much more, and more
/// than a couple of cells. The old exact fingerprint treated the caret as a new screen.
fn picture_same(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() || left.is_empty() {
        return false;
    }
    let changed = left.iter().zip(right).filter(|(a, b)| a.abs_diff(**b) > 22).count();
    changed < 3
}

fn lum(pixel: [u8; 4]) -> u8 {
    ((u16::from(pixel[2]) * 77 + u16::from(pixel[1]) * 150 + u16::from(pixel[0]) * 29) / 256) as u8
}

/// The plate takes the colour the text actually sits on: a white bubble gets a white plate with
/// dark ink, a dark game HUD stays dark with light ink — never a black bar across the art.
fn plate_colors(frame: &Frame, rect: &Rect) -> ([u8; 4], [u8; 4]) {
    let mut buckets = [0u32; 64];
    let mut sums = [[0u32; 3]; 64];
    let mut counted = 0u32;
    let mut sample = |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= frame.width() as i32 || y >= frame.height() as i32 {
            return;
        }
        let pixel = frame.pixel(x as u32, y as u32);
        let bucket = (lum(pixel) / 4) as usize;
        buckets[bucket] += 1;
        sums[bucket][0] += u32::from(pixel[0]);
        sums[bucket][1] += u32::from(pixel[1]);
        sums[bucket][2] += u32::from(pixel[2]);
        counted += 1;
    };
    let left = rect.x.saturating_sub(6);
    let right = rect.right() + 6;
    for y in rect.y.saturating_sub(6)..rect.y.saturating_sub(2) {
        for x in (left..right).step_by(2) {
            sample(x, y);
        }
    }
    for y in rect.bottom() + 2..rect.bottom() + 6 {
        for x in (left..right).step_by(2) {
            sample(x, y);
        }
    }
    for x in rect.x.saturating_sub(6)..rect.x.saturating_sub(2) {
        for y in (rect.y..rect.bottom()).step_by(2) {
            sample(x, y);
        }
    }
    for x in rect.right() + 2..rect.right() + 6 {
        for y in (rect.y..rect.bottom()).step_by(2) {
            sample(x, y);
        }
    }
    if counted == 0 {
        return ([16, 16, 16, 255], [244, 244, 244, 255]);
    }
    let best = (0..64).max_by_key(|bucket| buckets[*bucket]).unwrap_or(0);
    let n = buckets[best].max(1);
    let background = [
        (sums[best][0] / n) as u8,
        (sums[best][1] / n) as u8,
        (sums[best][2] / n) as u8,
        255u8,
    ];
    let foreground = if lum(background) > 128 {
        [26, 26, 26, 255]
    } else {
        [246, 246, 246, 255]
    };
    (background, foreground)
}

/// Clock prefix for journal lines, so the owner's report shows when each stage happened.
fn stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format!("{:02}:{:02}:{:02}", secs / 3600 % 24, secs / 60 % 60, secs % 60)
}

fn show_on(overlay: &mut Option<LayeredOverlay>, bounds: Rect, frame: &Frame, damage: &[Rect]) -> Result<(), String> {
    let mismatched = overlay
        .as_ref()
        .map(|surface| surface.size() != (bounds.width, bounds.height))
        .unwrap_or(true);
    let moved = if mismatched {
        false
    } else {
        overlay
            .as_mut()
            .map(|surface| surface.move_to(bounds).is_ok())
            .unwrap_or(false)
    };
    if mismatched || !moved {
        *overlay = Some(LayeredOverlay::create(bounds).map_err(|error| error.to_string())?);
    }
    if let Some(surface) = overlay.as_mut() {
        if surface.size() != (frame.width(), frame.height()) {
            let _ = surface.resize(frame.width(), frame.height());
        }
        if surface.present(frame, damage).is_ok() {
            return Ok(());
        }
    }
    *overlay = Some(LayeredOverlay::create(bounds).map_err(|error| error.to_string())?);
    let surface = overlay
        .as_mut()
        .ok_or_else(|| "окно перевода не открылось".to_owned())?;
    if surface.size() != (frame.width(), frame.height()) {
        let _ = surface.resize(frame.width(), frame.height());
    }
    surface.present(frame, damage).map_err(|error| error.to_string())
}

fn translate_fully(translator: &mut Translator, text: &str) -> Result<String, String> {
    let pieces = sentence_pieces(text);
    if pieces.len() <= 1 {
        return translator.translate(text);
    }
    let mut joined = String::new();
    let mut last_error = None;
    for piece in &pieces {
        match translator.translate(piece) {
            Ok(part) => {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                if !joined.is_empty() {
                    joined.push(' ');
                }
                joined.push_str(part);
            }
            Err(error) => last_error = Some(error),
        }
    }
    if joined.is_empty() {
        Err(last_error.unwrap_or_else(|| "перевод пустой".to_owned()))
    } else {
        Ok(joined)
    }
}

#[cfg(test)]
mod tests {
    use super::sentence_pieces;

    #[test]
    fn russian_chrome_is_never_translated() {
        use super::{has_cyrillic, looks_like_words};
        assert!(has_cyrillic("Стр 1, Стлб 3 430% Windows (CRLF) UTF-8"));
        assert!(has_cyrillic("*Безымянный – Блокнот"));
        assert!(!has_cyrillic("When life gives you lemons, drink tequila"));
        // Recognition noise from icons and borders has no vowel-bearing words.
        assert!(!looks_like_words("HHBLM 5"));
        assert!(!looks_like_words("KT I"));
        assert!(looks_like_words("When life gives you lemons, drink tequila"));
        assert!(looks_like_words("Hi"));
        assert!(looks_like_words("New Game"));
    }

    #[test]
    fn a_glued_notepad_line_is_three_sentences() {
        let parts = sentence_pieces("Hello. Open the door. New game");
        assert_eq!(parts, vec!["Hello.", "Open the door.", "New game"]);
    }

    #[test]
    fn a_decimal_is_not_a_sentence_break() {
        let parts = sentence_pieces("Damage 1.5");
        assert_eq!(parts, vec!["Damage 1.5"]);
    }

    #[test]
    fn plates_take_the_background_and_readable_ink() {
        use super::{lum, plate_colors};
        use lumen_core::{Frame, Rect};
        // White page with dark text: light plate, dark ink.
        let mut frame = Frame::filled(80, 60, [255, 255, 255, 255]).unwrap();
        for x in 20..60 {
            for y in 28..36 {
                frame.set_pixel(x, y, [10, 10, 10, 255]);
            }
        }
        let (background, foreground) = plate_colors(&frame, &Rect::new(22, 26, 36, 12));
        assert!(lum(background) > 128, "white page should get a light plate");
        assert!(lum(foreground) < 128);
        // Dark background: dark plate, light ink.
        let dark = Frame::filled(80, 60, [24, 24, 24, 255]).unwrap();
        let (background, foreground) = plate_colors(&dark, &Rect::new(22, 26, 36, 12));
        assert!(lum(background) < 128, "dark art should get a dark plate");
        assert!(lum(foreground) > 128);
    }
}

fn sentence_pieces(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut parts = Vec::new();
    let mut start = 0usize;
    for index in 0..chars.len() {
        if !matches!(chars[index], '.' | '!' | '?') {
            continue;
        }
        let boundary = match chars.get(index + 1).copied() {
            None => true,
            Some(ch) if ch.is_whitespace() => true,
            Some(ch) if ch.is_uppercase() => true,
            _ => false,
        };
        if !boundary {
            continue;
        }
        let piece: String = chars[start..=index].iter().collect();
        let piece = piece.trim();
        if piece.chars().any(|ch| ch.is_alphabetic()) {
            parts.push(piece.to_owned());
        }
        start = index + 1;
    }
    let tail: String = chars[start..].iter().collect();
    let tail = tail.trim();
    if tail.chars().any(|ch| ch.is_alphabetic()) {
        parts.push(tail.to_owned());
    }
    if parts.is_empty() {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_owned());
        }
    }
    parts
}

#[derive(Clone, Serialize)]
pub struct ListedWindow {
    pub id: String,
    pub title: String,
    pub process: String,
    pub selected: bool,
}

pub fn list_windows(control: &Control) -> Vec<ListedWindow> {
    restore_choice_once(control);
    let selected = lock(&control.inner.choice).as_ref().map(|chosen| chosen.id);
    let ours = our_process_name();
    let mut windows: Vec<ListedWindow> = open_windows()
        .into_iter()
        .filter(|target| !is_ours(&target.title, &target.process, &ours))
        .map(|target| ListedWindow {
            id: target.id.to_string(),
            title: clip(&target.title, 80),
            process: friendly_process(&target.process),
            selected: selected == Some(target.id),
        })
        .collect();
    windows.sort_by_key(|window| !window.selected);
    windows.truncate(16);
    windows
}

pub fn choose_window(control: &Control, app: &AppHandle, id: &str) {
    if id.is_empty() {
        *lock(&control.inner.choice) = None;
        save_choice("", "");
        let current = control.status();
        if !current.paused && !matches!(current.phase.as_str(), "downloading" | "preparing" | "error") {
            publish(
                control,
                app,
                view(
                    "waiting",
                    "Само",
                    "Беру окно, которое было впереди. Надёжнее нажать нужное в списке.",
                ),
            );
        }
        return;
    }
    let Ok(parsed) = id.parse::<u64>() else {
        return;
    };
    let Some(found) = open_windows().into_iter().find(|target| target.id == parsed) else {
        return;
    };
    let chosen = Chosen {
        id: found.id,
        title: clip(&found.title, 80),
        process: found.process,
    };
    save_choice(&chosen.title, &chosen.process);
    *lock(&control.inner.choice) = Some(chosen.clone());
    let mut status = control.status();
    status.watched = chosen.title.clone();
    if status.paused {
        publish(control, app, status);
        return;
    }
    if status.phase == "downloading" || status.phase == "preparing" {
        status.detail = format!(
            "Выбрано «{}». Когда программа будет готова, возьму это окно.",
            chosen.title
        );
        publish(control, app, status);
        return;
    }
    if status.phase == "error" {
        status.detail = format!(
            "«{}» выбрано. Как только перевод поднимется, возьму это окно.",
            chosen.title
        );
        publish(control, app, status);
        return;
    }
    status.phase = "watching".to_owned();
    status.title = "Смотрю".to_owned();
    status.detail = "Это окно выбрано. Выводить его вперёд не нужно.".to_owned();
    publish(control, app, status);
}

fn resolve_target(control: &Control) -> Option<(isize, String)> {
    restore_choice_once(control);
    let chosen = lock(&control.inner.choice).clone();
    if let Some(chosen) = chosen {
        if window_alive(chosen.id) {
            return Some((chosen.id as isize, chosen.title));
        }
        if let Some(found) = find_match(&chosen.title, &chosen.process) {
            let title = found.title.clone();
            let id = found.id;
            *lock(&control.inner.choice) = Some(found);
            return Some((id as isize, title));
        }
        *lock(&control.inner.choice) = None;
        return None;
    }
    match focus() {
        Focus::Other { hwnd, title } => {
            *lock(&control.inner.sticky) = Some(Chosen {
                id: hwnd as u64,
                title: title.clone(),
                process: String::new(),
            });
            Some((hwnd, title))
        }
        Focus::Ours | Focus::None => {
            let sticky = lock(&control.inner.sticky).clone()?;
            if window_alive(sticky.id) {
                Some((sticky.id as isize, sticky.title))
            } else {
                *lock(&control.inner.sticky) = None;
                None
            }
        }
    }
}

fn restore_choice_once(control: &Control) {
    if control.inner.restored.load(Ordering::SeqCst) {
        return;
    }
    let saved = read_saved();
    if saved.title.is_empty() {
        control.inner.restored.store(true, Ordering::SeqCst);
        return;
    }
    let Some(found) = find_match(&saved.title, &saved.process) else {
        if open_windows().is_empty() {
            return;
        }
        control.inner.restored.store(true, Ordering::SeqCst);
        return;
    };
    control.inner.restored.store(true, Ordering::SeqCst);
    *lock(&control.inner.choice) = Some(found);
}

fn find_match(title: &str, process: &str) -> Option<Chosen> {
    open_windows()
        .into_iter()
        .find(|target| target.title == title && process_matches(&target.process, process))
        .map(|target| Chosen {
            id: target.id,
            title: clip(&target.title, 80),
            process: target.process,
        })
}

fn process_matches(current: &str, saved: &str) -> bool {
    saved.is_empty() || current.eq_ignore_ascii_case(saved)
}

fn open_windows() -> Vec<lumen_capture::CaptureTarget> {
    match std::panic::catch_unwind(enumerate_targets) {
        Ok(Ok(targets)) => targets,
        _ => Vec::new(),
    }
}

fn window_alive(id: u64) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::IsWindow;
    if id == 0 {
        return false;
    }
    unsafe { IsWindow(HWND(id as *mut std::ffi::c_void)) }.as_bool()
}

fn our_process_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "DynoTranslate.exe".to_owned())
}

fn is_ours(title: &str, process: &str, ours: &str) -> bool {
    title == "DynoTranslate"
        || title == "Program Manager"
        || process.eq_ignore_ascii_case(ours)
        || process.eq_ignore_ascii_case("DynoTranslate.exe")
}

fn friendly_process(process: &str) -> String {
    process.trim_end_matches(".exe").trim_end_matches(".EXE").to_owned()
}

fn panic_text(error: &(dyn std::any::Any + Send)) -> String {
    let raw = if let Some(text) = error.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = error.downcast_ref::<String>() {
        text.clone()
    } else {
        String::new()
    };
    clip(&raw.replace(['\n', '\r'], " "), 140)
}

fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("DynoTranslate")
}

#[derive(Clone, Serialize, Deserialize, Default)]
struct Saved {
    #[serde(default)]
    paused: bool,
    #[serde(default)]
    title: String,
    #[serde(default)]
    process: String,
}

fn load_paused() -> bool {
    read_saved().paused
}

fn save_paused(paused: bool) {
    let mut saved = read_saved();
    saved.paused = paused;
    write_saved(&saved);
}

fn save_choice(title: &str, process: &str) {
    let mut saved = read_saved();
    saved.title = title.to_owned();
    saved.process = process.to_owned();
    write_saved(&saved);
}

fn read_saved() -> Saved {
    let Ok(text) = std::fs::read_to_string(data_dir().join("state.json")) else {
        return Saved::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn write_saved(saved: &Saved) {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(text) = serde_json::to_string(saved) {
        let _ = std::fs::write(dir.join("state.json"), text);
    }
}

fn log_file_path() -> PathBuf {
    data_dir().join("live.log")
}

fn open_log() -> std::fs::File {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = log_file_path();
    // Keep the journal bounded: a previous run's tail is better than a 400 MB append-only file.
    if std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0) > 4 * 1024 * 1024 {
        let rotated = dir.join("live.log.prev");
        let _ = std::fs::remove_file(&rotated);
        let _ = std::fs::rename(&path, rotated);
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap_or_else(|_| fallback_log())
}

fn fallback_log() -> std::fs::File {
    let path = std::env::temp_dir().join("dynotranslate-live.log");
    if let Ok(file) = std::fs::File::create(&path) {
        return file;
    }
    OpenOptions::new()
        .write(true)
        .open("NUL")
        .unwrap_or_else(|_| std::fs::File::create(path).unwrap_or_else(|_| std::fs::File::open("NUL").expect("log")))
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
