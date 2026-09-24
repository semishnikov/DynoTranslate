//! Watches the foreground window and draws Russian over English text.
//!
//! The settings window used to be a separate mock. This loop is the product, and the window
//! only reports what the loop is actually doing.

use std::collections::{HashMap, VecDeque};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lumen_capture::{capture_window_picture, capture_window_screen, enumerate_targets};
use lumen_core::{Frame, Rect};
use lumen_ocr::Recognition;
use lumen_overlay::windows::LayeredOverlay;
use lumen_overlay::{Compositor, OverlayBlock, OverlayLayout, OverlaySurface};
use lumen_render::FontWeight;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::backends;
use crate::model::Translator;
use crate::readtext::{dark_bars, Reader};
use crate::settings::LiveSettingsHandle;

const MAX_OCR_WIDTH: u32 = 1280;

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

pub fn run(app: AppHandle, control: Control, bundled: Option<PathBuf>, settings: LiveSettingsHandle) {
    loop {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            loop_forever(app.clone(), control.clone(), bundled.clone(), settings.clone());
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
fn loop_forever(app: AppHandle, control: Control, bundled: Option<PathBuf>, settings: LiveSettingsHandle) {
    let _ = log("live loop started");
    init_winrt();
    if let Err(error) = register_pause_hotkey() {
        let _ = log(&format!("pause hotkey: {error}"));
    }

    let mut log_file = open_log();
    let boot = settings.read().expect("live settings").clone();
    let _ = writeln!(
        log_file,
        "{} session start version={} dir={:?} log={:?}",
        stamp(),
        env!("CARGO_PKG_VERSION"),
        crate::settings::data_dir(),
        log_file_path()
    );
    let _ = writeln!(
        log_file,
        "{} settings {}",
        stamp(),
        serde_json::to_string(&boot).unwrap_or_default()
    );
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
    let mut cache: HashMap<String, String> = load_memory();
    let mut saved_entries = cache.len();
    let mut bands: Vec<BandState> = Vec::new();
    // Recent source/translation pairs; the LLM backends read it for coherence.
    let mut context: VecDeque<(String, String)> = VecDeque::new();
    let mut last_settings_json = String::new();
    let mut frame_counter: u32 = 0;
    let (async_tx, async_rx) = std::sync::mpsc::channel::<(String, String)>();
    let mut dispatch_busy = false;
    let mut plates: HashMap<String, Plate> = HashMap::new();
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
                bands.clear();
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

            // The preview and the saved frames keep the small copy; recognition reads the
            // picture at its own size, because squeezing it is what glued words together.
            let (small, _scale) = downscale(&frame);
            let ocr_started = Instant::now();
            let (lines, bands_read) = read_bands(&mut reader, &frame, &mut bands, &mut log_file);
            if bands_read == 0 && lines.is_empty() && !held.is_empty() {
                quiet_until = Instant::now() + Duration::from_millis(800);
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }

            let settings = settings.read().expect("live settings").clone();
            let settings_json = serde_json::to_string(&settings).unwrap_or_default();
            if settings_json != last_settings_json {
                let _ = writeln!(log_file, "{} settings {}", stamp(), settings_json);
                last_settings_json = settings_json;
            }
            // Finished async translations land here; the tick itself never waits on the
            // network, which is what keeps the overlay at capture speed.
            while let Ok((key, value)) = async_rx.try_recv() {
                if key.is_empty() {
                    dispatch_busy = false;
                    continue;
                }
                let echo = echo_of_source(&key, &value);
                let _ = writeln!(
                    log_file,
                    "{} async backend={} echo={} {:?} -> {:?}",
                    stamp(),
                    settings.translator,
                    echo,
                    key,
                    value
                );
                cache.insert(key, if echo { String::new() } else { value });
            }
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
                    "{} tick window={:?} frame={}x{} ocr={}ms bands={}/{} lines={} style={:?} backend={} minconf={:.2}",
                    stamp(),
                    name,
                    frame.width(),
                    frame.height(),
                    ocr_started.elapsed().as_millis(),
                    bands_read,
                    BANDS,
                    lines.len(),
                    settings.overlay_style,
                    settings.translator,
                    settings.min_confidence
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
                if let Some(path) = save_frame(&frame, &mut frame_counter) {
                    let _ = writeln!(
                        log_file,
                        "{} frame saved {:?} (exactly what OCR saw; attach it instead of a screenshot)",
                        stamp(),
                        path
                    );
                }
            }

            // A bubble is the unit of meaning: stacked lines with overlapping columns are one
            // speech, translated together and covered by one plate. Translating line fragments
            // separately is exactly what produced the word salad the owner rejected.
            let mut candidates: Vec<(Rect, String, f32)> = Vec::new();
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
                    if dump {
                        let _ = writeln!(log_file, "{} skip reason=noise text={:?}", stamp(), source);
                    }
                    continue;
                }
                if chaotic(source) {
                    // Browser chrome the recogniser transliterated into case-flipping Latin
                    // ("veppyHuTOxnLEx+"): real English lines never look like that.
                    if dump {
                        let _ = writeln!(log_file, "{} skip reason=chaotic text={:?}", stamp(), source);
                    }
                    continue;
                }
                if line.confidence < settings.min_confidence.min(0.55) {
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
                let rect = line.bounds.clamp_to(&frame.bounds()).unwrap_or(line.bounds);
                if rect.height < 8 || rect.width < 8 {
                    continue;
                }
                if stray_token(source) || chrome_line(source) {
                    if dump {
                        let _ = writeln!(log_file, "{} skip reason=stray text={:?}", stamp(), source);
                    }
                    continue;
                }
                let cleaned = clean_line(source);
                if cleaned.is_empty() {
                    continue;
                }
                candidates.push((rect, cleaned, line.confidence));
            }
            let bubbles = merge_bubbles(candidates);
            if dump {
                for bubble in &bubbles {
                    let r = bubble.rect;
                    let _ = writeln!(
                        log_file,
                        "{} bubble conf={:.2} rect=({},{},{}x{}) lines={} text={:?}",
                        stamp(),
                        bubble.conf,
                        r.x,
                        r.y,
                        r.width,
                        r.height,
                        bubble.parts.len(),
                        bubble.parts.join(" ")
                    );
                }
            }

            // Whole bubbles go to the translator: cached ones draw now, the rest dispatch as
            // one batch and arrive through the channel without blocking the loop.
            let mut missing: Vec<String> = Vec::new();
            for bubble in &bubbles {
                let key = clip(&bubble.parts.join(" "), 400);
                if !cache.contains_key(&key) {
                    missing.push(key);
                }
            }
            // The setting can raise the batch, never cut the screen in half: a dense page needs
            // every line translated in the same pass, and the memory makes known lines free.
            let cap = settings.max_lines_per_tick.max(24);
            if missing.len() > cap {
                pending = true;
                missing.truncate(cap);
            }
            if !missing.is_empty() {
                if settings.translator == "local" {
                    let started = Instant::now();
                    for key in &missing {
                        match translate_fully(&mut translator, key) {
                            Ok(text) if has_cyrillic(&text) => {
                                let stored = if echo_of_source(key, &text) {
                                    String::new()
                                } else {
                                    text.clone()
                                };
                                cache.insert(key.clone(), stored);
                                control.inner.translated.fetch_add(1, Ordering::SeqCst);
                                if settings.context_lines > 0 {
                                    context.push_back((key.clone(), text));
                                    while context.len() > settings.context_lines {
                                        context.pop_front();
                                    }
                                }
                            }
                            Ok(_) => {}
                            Err(error) => {
                                let _ = writeln!(log_file, "{} translate error {:?}", stamp(), error);
                            }
                        }
                    }
                    let _ = writeln!(
                        log_file,
                        "{} translate backend=local ms={} bubbles={}",
                        stamp(),
                        started.elapsed().as_millis(),
                        missing.len()
                    );
                } else if !dispatch_busy {
                    dispatch_busy = true;
                    let pairs: Vec<(String, String)> = context.iter().cloned().collect();
                    let backend = settings.translator.clone();
                    let snapshot = settings.clone();
                    let sender = async_tx.clone();
                    let _ = writeln!(
                        log_file,
                        "{} dispatch backend={} bubbles={}",
                        stamp(),
                        backend,
                        missing.len()
                    );
                    std::thread::spawn(move || {
                        let result = backends::translate_batch(&backend, &missing, &pairs, &snapshot);
                        if let Ok(out) = result {
                            for (key, value) in missing.into_iter().zip(out) {
                                if has_cyrillic(&value) {
                                    let _ = sender.send((key, value));
                                }
                            }
                        }
                        let _ = sender.send((String::new(), String::new()));
                    });
                    pending = true;
                } else {
                    pending = true;
                }
            }
            if cache.len() >= saved_entries + 12 {
                save_memory(&cache, &mut log_file);
                saved_entries = cache.len();
            }
            if cache.len() > 4000 {
                cache.clear();
                saved_entries = 0;
            }

            // A translation becomes a persistent plate: it follows its source text with
            // interpolation and lives a moment after the source flickers out, so the overlay
            // glides with the picture instead of snapping and blinking.
            let now = Instant::now();
            for bubble in &bubbles {
                let key = clip(&bubble.parts.join(" "), 400);
                let Some(translated) = cache.get(&key) else {
                    pending = true;
                    continue;
                };
                if translated.is_empty() {
                    // The translator handed the source back unchanged, so this line is a logo or
                    // a transliterated title. Drawing it would put "СПУК" over the picture.
                    continue;
                }
                match plates.get_mut(&key) {
                    Some(plate) => {
                        plate.target = bubble.rect;
                        plate.line_h = bubble.line_h;
                        plate.conf = bubble.conf;
                        plate.seen = now;
                    }
                    None => {
                        let (background, foreground) = plate_colors(&frame, &bubble.rect);
                        plates.insert(
                            key,
                            Plate {
                                text: translated.clone(),
                                target: bubble.rect,
                                drawn: bubble.rect,
                                line_h: bubble.line_h,
                                conf: bubble.conf,
                                background,
                                foreground,
                                seen: now,
                            },
                        );
                    }
                }
            }
            plates.retain(|_, plate| now.duration_since(plate.seen) < Duration::from_millis(420));
            for plate in plates.values_mut() {
                plate.drawn = lerp_rect(&plate.drawn, &plate.target, 0.35);
                let score = plate.text.chars().count();
                if score > sample_score {
                    sample_score = score;
                    sample = clip(&plate.text, 120);
                }
            }
            let kept = plate_keys(&plates);
            if dump {
                for (key, plate) in &plates {
                    let r = plate.drawn;
                    let size = (plate.line_h as f32 * settings.font_scale).clamp(12.0, 32.0) as u32;
                    let _ = writeln!(
                        log_file,
                        "{} plate{} rect=({},{},{}x{}) font={} text={:?}",
                        stamp(),
                        if kept.contains(key) { "" } else { " skip=covered" },
                        r.x,
                        r.y,
                        r.width,
                        r.height,
                        size,
                        plate.text
                    );
                }
            }
            blocks.extend(plate_blocks(&plates, &kept, settings.font_scale));

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

            let layout = OverlayLayout::new(settings.overlay_style)
                .with_blocks(blocks)
                .with_opacity(settings.opacity);
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
                // Between two readings the plates keep sliding: seven 60 Hz steps towards the same
                // target, so a subtitle that moves on the screen looks animated instead of
                // jumping once per capture. The compositor damages both the old and the new
                // position, so nothing is left behind.
                let keys = plate_keys(&plates);
                for _ in 0..7 {
                    let mut moved = false;
                    for plate in plates.values_mut() {
                        let next = lerp_rect(&plate.drawn, &plate.target, 0.22);
                        if next != plate.drawn {
                            plate.drawn = next;
                            moved = true;
                        }
                    }
                    if !moved {
                        break;
                    }
                    let layout = OverlayLayout::new(settings.overlay_style)
                        .with_blocks(plate_blocks(&plates, &keys, settings.font_scale))
                        .with_opacity(settings.opacity);
                    let composition = compositor.compose(&frame, &layout);
                    if show_on(&mut overlay, bounds, &composition.frame, &composition.damage).is_err() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(16));
                }
                last_sig = sig.clone();
                last_hwnd = hwnd;
            }
            std::thread::sleep(Duration::from_millis(if pending { 30 } else { 60 }));
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

/// Recognition clips stray glyphs of neighbouring lines onto a line's ends; a leading or
/// trailing one-letter token ("J ", "7 ") is never a word, so drop it before translation.
fn clean_line(text: &str) -> String {
    let mut words: Vec<&str> = text.split_whitespace().collect();
    loop {
        let tiny = |word: &&str| word.chars().filter(|ch| ch.is_alphabetic()).count() <= 1;
        if words.len() > 2 && tiny(&words[0]) {
            words.remove(0);
        } else if words.len() > 2 && tiny(words.last().expect("checked")) {
            words.pop();
        } else {
            break;
        }
    }
    words.join(" ")
}

/// One speech, not one line: the stacked lines of a subtitle or of a paragraph are one thought,
/// and translating their fragments separately is what produced the word salad.
struct Bubble {
    rect: Rect,
    parts: Vec<String>,
    conf: f32,
    line_h: u32,
}

/// Groups recognised lines into bubbles. Order-independent on purpose: the previous pass compared
/// each line with the bubble above it only, so one overlapping pair of boxes — the few pixels by
/// which OCR boxes of neighbouring subtitle lines overlap — broke the chain and one sentence
/// became four plates, each translated on its own.
fn merge_bubbles(mut lines: Vec<(Rect, String, f32)>) -> Vec<Bubble> {
    lines.sort_by_key(|(rect, _, _)| (rect.y, rect.x));
    let mut bubbles: Vec<Bubble> = Vec::new();
    for (rect, text, conf) in lines {
        let mut best: Option<usize> = None;
        let mut best_score = i32::MIN;
        for (index, bubble) in bubbles.iter().enumerate() {
            let height = bubble.line_h.min(rect.height) as i32;
            let gap = rect.y - bubble.rect.bottom();
            if gap > (height / 3).max(8) || gap < -(height * 2 / 5) {
                continue;
            }
            let overlap = (bubble.rect.x + bubble.rect.width as i32)
                .min(rect.x + rect.width as i32)
                - bubble.rect.x.max(rect.x);
            let narrower = bubble.rect.width.min(rect.width) as i32;
            if overlap * 4 < narrower {
                continue;
            }
            let score = -gap.abs();
            if score > best_score {
                best_score = score;
                best = Some(index);
            }
        }
        match best {
            Some(index) => {
                let bubble = &mut bubbles[index];
                bubble.rect = bubble.rect.union(&rect);
                bubble.parts.push(text);
                bubble.conf = bubble.conf.max(conf);
                bubble.line_h = (bubble.line_h + rect.height) / 2;
            }
            None => bubbles.push(Bubble {
                rect,
                parts: vec![text],
                conf,
                line_h: rect.height,
            }),
        }
    }
    bubbles
}

/// A lone read of two letters ("ev]", "PI") is a fragment of an icon or of a neighbouring line,
/// never a phrase worth a plate.
fn stray_token(text: &str) -> bool {
    text.split_whitespace().count() == 1 && text.chars().filter(|ch| ch.is_alphabetic()).count() < 3
}

/// Addresses and web chrome: a browser paints the page title, the address and its buttons on every
/// frame, and none of that is part of the picture the owner is watching.
fn chrome_line(text: &str) -> bool {
    let lower = text.to_lowercase();
    ["www.", "http", ".com", ".ru", ".net", ".org"]
        .iter()
        .any(|needle| lower.contains(needle))
}

/// Recognition runs over horizontal bands, and only over the bands whose pixels moved.
///
/// The whole frame used to be read again for every tick — a full detector pass over the browser
/// chrome, the page and the video, twelve hundred milliseconds of it, to notice that one subtitle
/// line had changed. A dialogue line now costs the band it sits in, and every other band keeps the
/// reading it already had. Lines are attributed to the band that owns their middle, so a band can
/// be re-read on its own without duplicating or losing its neighbours.
struct BandState {
    sig: Vec<u8>,
    lines: Vec<Recognition>,
}

const BANDS: u32 = 12;
/// A band is read with a margin above and below, because a line's box can start just outside it;
/// the reading is then filed with the band that owns the line's middle.
const BAND_PAD: u32 = 18;

fn band_sig(frame: &Frame, top: u32, bottom: u32) -> Vec<u8> {
    const COLS: u32 = 40;
    const ROWS: u32 = 5;
    if frame.width() == 0 || bottom <= top {
        return Vec::new();
    }
    let mut sig = Vec::with_capacity((COLS * ROWS) as usize);
    for row in 0..ROWS {
        let y0 = top + row * (bottom - top) / ROWS;
        let y1 = (top + (row + 1) * (bottom - top) / ROWS).max(y0 + 1).min(bottom);
        for col in 0..COLS {
            let x0 = col * frame.width() / COLS;
            let x1 = ((col + 1) * frame.width() / COLS).max(x0 + 1).min(frame.width());
            let mut sum = 0u32;
            let mut count = 0u32;
            let step_x = ((x1 - x0) / 3).max(1);
            let step_y = ((y1 - y0) / 2).max(1);
            let mut y = y0;
            while y < y1 {
                let mut x = x0;
                while x < x1 {
                    sum += u32::from(lum(frame.pixel(x, y)));
                    count += 1;
                    x += step_x;
                }
                y += step_y;
            }
            sig.push((sum / count.max(1)) as u8);
        }
    }
    sig
}

fn read_bands(
    reader: &mut Reader,
    frame: &Frame,
    bands: &mut Vec<BandState>,
    log_file: &mut std::fs::File,
) -> (Vec<Recognition>, u32) {
    let height = frame.height();
    if height == 0 {
        return (Vec::new(), 0);
    }
    if bands.len() != BANDS as usize {
        bands.clear();
        bands.resize_with(BANDS as usize, || BandState {
            sig: Vec::new(),
            lines: Vec::new(),
        });
    }
    let bounds_of = |index: usize| -> (u32, u32) {
        let top = index as u32 * height / BANDS;
        let bottom = (((index as u32 + 1) * height / BANDS).max(top + 1)).min(height);
        (top, bottom)
    };
    let mut changed = vec![false; BANDS as usize];
    let mut fresh: Vec<Vec<u8>> = Vec::with_capacity(BANDS as usize);
    for index in 0..BANDS as usize {
        let (top, bottom) = bounds_of(index);
        let sig = band_sig(frame, top, bottom);
        changed[index] = !picture_same(&bands[index].sig, &sig);
        fresh.push(sig);
    }
    // Neighbouring changes become one reading, with a band of slack on each side.
    let mut regions: Vec<(usize, usize)> = Vec::new();
    for index in 0..BANDS as usize {
        if !changed[index] {
            continue;
        }
        let start = index.saturating_sub(1);
        let end = (index + 1).min(BANDS as usize - 1);
        match regions.last_mut() {
            Some(last) if start <= last.1 + 1 => last.1 = last.1.max(end),
            _ => regions.push((start, end)),
        }
    }
    // Letterbox rows are measured once on the whole picture, where the bars really are.
    let bars = dark_bars(frame);
    let mut found: Vec<Vec<Recognition>> = vec![Vec::new(); BANDS as usize];
    let mut dirty = vec![false; BANDS as usize];
    let mut spent = 0u32;
    let mut read_count = 0u32;
    for (start, end) in regions {
        let (top, _) = bounds_of(start);
        let (_, bottom) = bounds_of(end);
        let top = top.saturating_sub(BAND_PAD);
        let bottom = (bottom + BAND_PAD).min(height);
        let band = Rect::new(0, top as i32, frame.width(), bottom - top);
        let Some(region) = band.clamp_to(&frame.bounds()) else {
            continue;
        };
        let Some(crop) = frame.crop(region) else {
            continue;
        };
        let local: Vec<(u32, u32)> = bars
            .iter()
            .filter_map(|(top, bottom)| {
                let from = (*top as i32 - region.y).max(0) as u32;
                let to = (*bottom as i32 - region.y).min(region.height as i32).max(0) as u32;
                (to > from).then_some((from, to))
            })
            .collect();
        let started = Instant::now();
        match reader.read(&crop, &local) {
            Ok(lines) => {
                for mut line in lines {
                    line.bounds.x += region.x;
                    line.bounds.y += region.y;
                    let center = line.bounds.y + line.bounds.height as i32 / 2;
                    let owner = (((center.max(0) as u32) * BANDS) / height) as usize;
                    let owner = owner.min(BANDS as usize - 1);
                    if owner < start || owner > end {
                        // The margin reaches into a band this reading does not own; that band
                        // keeps the lines it already has.
                        continue;
                    }
                    found[owner].push(line);
                }
                for index in start..=end {
                    dirty[index] = true;
                }
                read_count += (end - start + 1) as u32;
            }
            Err(error) => {
                let _ = writeln!(
                    log_file,
                    "{} ocr region ({},{})x{}: {}",
                    stamp(),
                    region.x,
                    region.y,
                    region.height,
                    error
                );
            }
        }
        spent += started.elapsed().as_millis() as u32;
    }
    for index in 0..BANDS as usize {
        bands[index].sig = std::mem::take(&mut fresh[index]);
        if dirty[index] {
            bands[index].lines = std::mem::take(&mut found[index]);
        }
    }
    let mut lines = Vec::new();
    for state in bands.iter() {
        lines.extend(state.lines.iter().cloned());
    }
    let _ = writeln!(
        log_file,
        "{} ocr bands={}/{} ms={} lines={}",
        stamp(),
        read_count,
        BANDS,
        spent,
        lines.len()
    );
    (lines, read_count)
}

/// Russian letters as a Latin keyboard would type them; used only to compare a translation with
/// the text it came from.
fn transliterate(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        let low = ch.to_lowercase().next().unwrap_or(ch);
        match low {
            'а' => out.push('a'),
            'б' => out.push('b'),
            'в' => out.push('v'),
            'г' => out.push('g'),
            'д' => out.push('d'),
            'е' | 'ё' | 'э' => out.push('e'),
            'ж' => out.push_str("zh"),
            'з' => out.push('z'),
            'и' | 'й' => out.push('i'),
            'к' => out.push('k'),
            'л' => out.push('l'),
            'м' => out.push('m'),
            'н' => out.push('n'),
            'о' => out.push('o'),
            'п' => out.push('p'),
            'р' => out.push('r'),
            'с' => out.push('s'),
            'т' => out.push('t'),
            'у' => out.push('u'),
            'ф' => out.push('f'),
            'х' => out.push_str("kh"),
            'ц' => out.push_str("ts"),
            'ч' => out.push_str("ch"),
            'ш' => out.push_str("sh"),
            'щ' => out.push_str("shch"),
            'ы' => out.push('y'),
            'ю' => out.push_str("yu"),
            'я' => out.push_str("ya"),
            'ъ' | 'ь' => {}
            other => out.push(other),
        }
    }
    out
}

fn letters_only(text: &str) -> String {
    text.chars().filter(|ch| ch.is_alphanumeric()).collect()
}

fn skeleton(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_alphabetic() && !matches!(ch, 'a' | 'e' | 'i' | 'o' | 'u' | 'y'))
        .collect()
}

fn distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    if left.is_empty() {
        return right.len();
    }
    if right.is_empty() {
        return left.len();
    }
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0usize; right.len() + 1];
    for (i, left_char) in left.iter().enumerate() {
        current[0] = i + 1;
        for (j, right_char) in right.iter().enumerate() {
            let cost = usize::from(left_char != right_char);
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

/// A stylised logo, a transliterated title and Russian chrome all come back from the translator as
/// their own letters in Cyrillic: "Makap" gives "Макап", "SPUK" gives "СПУК", the browser's
/// "TeKCT" gives "Текст". Drawing those puts Russian noise over art nobody asked to cover, which
/// is exactly the junk plates the owner saw ("СПУК", "АЭТо: Макап", "ПИ"). A translation that is
/// only the source spelled in Cyrillic is thrown away instead.
fn echo_of_source(source: &str, translation: &str) -> bool {
    let source = letters_only(&source.to_lowercase());
    let reply = letters_only(&transliterate(translation));
    if source.chars().count() < 2 || reply.chars().count() < 2 {
        return false;
    }
    let source_skeleton = skeleton(&source);
    if !source_skeleton.is_empty() && source_skeleton == skeleton(&reply) {
        return true;
    }
    // One letter in five may differ: "HOLDINGABOMB" comes back as "ХОЛДИНГАБОМБА", while a real
    // translation of a loanword ("computer" -> "компьютер") is further away than that.
    let longest = source.chars().count().max(reply.chars().count());
    longest >= 4 && distance(&source, &reply) * 5 <= longest
}

/// Two plates for the same text, or a plate where a larger plate already covers the source, are
/// the doubled Russian the owner watched drift over one line. The larger plate wins.
fn plate_keys(plates: &HashMap<String, Plate>) -> Vec<String> {
    let mut candidates: Vec<(&String, Rect, &str)> = plates
        .iter()
        .map(|(key, plate)| (key, plate.drawn, plate.text.as_str()))
        .collect();
    candidates.sort_by_key(|(_, rect, _)| std::cmp::Reverse(rect.width.saturating_mul(rect.height)));
    let mut kept: Vec<(&String, Rect, &str)> = Vec::new();
    for (key, rect, text) in candidates {
        if kept.iter().any(|(_, _, other)| *other == text) {
            continue;
        }
        if kept.iter().any(|(_, roof, _)| covered_part(rect, *roof) >= 0.55) {
            continue;
        }
        kept.push((key, rect, text));
    }
    kept.into_iter().map(|(key, _, _)| key.clone()).collect()
}

fn covered_part(rect: Rect, roof: Rect) -> f32 {
    let Some(overlap) = rect.intersection(&roof) else {
        return 0.0;
    };
    let area = u64::from(rect.width) * u64::from(rect.height);
    if area == 0 {
        return 0.0;
    }
    (u64::from(overlap.width) * u64::from(overlap.height)) as f32 / area as f32
}

fn plate_blocks(plates: &HashMap<String, Plate>, kept: &[String], font_scale: f32) -> Vec<OverlayBlock> {
    let mut ordered: Vec<&Plate> = kept.iter().filter_map(|key| plates.get(key)).collect();
    ordered.sort_by_key(|plate| (plate.drawn.y, plate.drawn.x));
    ordered
        .into_iter()
        .map(|plate| {
            let size = (plate.line_h as f32 * font_scale).clamp(12.0, 32.0) as u32;
            OverlayBlock::new(plate.drawn, plate.text.clone())
                .with_font(size, FontWeight::Regular, false)
                .with_colors(plate.background, plate.foreground)
                .with_confidence(plate.conf)
        })
        .collect()
}

fn memory_path() -> PathBuf {
    crate::settings::data_dir().join("translations.json")
}

/// Translations stay on disk between runs. A game, a film or a lesson repeats its lines, and a
/// known line costs nothing: no request, no wait, the plate is on the screen on the first frame
/// the text appears.
fn load_memory() -> HashMap<String, String> {
    std::fs::read_to_string(memory_path())
        .ok()
        .and_then(|text| serde_json::from_str::<HashMap<String, String>>(&text).ok())
        .unwrap_or_default()
}

fn save_memory(memory: &HashMap<String, String>, log_file: &mut std::fs::File) {
    let Ok(text) = serde_json::to_string(memory) else {
        return;
    };
    let path = memory_path();
    let partial = path.with_extension("json.partial");
    if std::fs::write(&partial, text).is_err() {
        return;
    }
    if std::fs::rename(&partial, &path).is_ok() {
        let _ = writeln!(log_file, "{} memory saved entries={}", stamp(), memory.len());
    }
}

struct Plate {
    text: String,
    target: Rect,
    drawn: Rect,
    line_h: u32,
    conf: f32,
    background: [u8; 4],
    foreground: [u8; 4],
    seen: Instant,
}

fn lerp_rect(from: &Rect, to: &Rect, t: f32) -> Rect {
    let mix = |a: f32, b: f32| (a + (b - a) * t).round();
    Rect::new(
        mix(from.x as f32, to.x as f32) as i32,
        mix(from.y as f32, to.y as f32) as i32,
        mix(from.width as f32, to.width as f32) as u32,
        mix(from.height as f32, to.height as f32) as u32,
    )
}

/// Browser chrome arrives as case-flipping Latin transliteration ("veppyHuTOxnLEx+",
/// "YHH4TOKHR"); living English never does. A line is skipped when a third of its words
/// flip case twice or more or carry a digit inside.
fn chaotic(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return false;
    }
    let bad = words.iter().filter(|word| chaotic_word(word)).count();
    bad * 3 >= words.len()
}

fn chaotic_word(word: &str) -> bool {
    let chars: Vec<char> = word.chars().filter(|ch| ch.is_alphabetic()).collect();
    if chars.len() < 4 {
        return false;
    }
    let mut switches = 0;
    for pair in chars.windows(2) {
        if pair[0].is_uppercase() != pair[1].is_uppercase() {
            switches += 1;
        }
    }
    let digit_inside = word.len() >= 5 && word.chars().any(|ch| ch.is_ascii_digit());
    // No length rule: the recogniser glues small subtitle words into one long token
    // ("NOBODYLIKESITDARKER"); that is real text and must reach the translator.
    switches >= 2 || digit_inside
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
    format!("data:image/bmp;base64,{}", base64(&bmp_bytes(frame, 280, 160)))
}

/// A BMP snapshot of the OCR input, kept inside the app folder and pruned to the newest few,
/// so a bug report is the journal plus these files — no screenshots required.
fn save_frame(frame: &Frame, counter: &mut u32) -> Option<PathBuf> {
    if frame.width() == 0 || frame.height() == 0 {
        return None;
    }
    let dir = crate::settings::frames_dir();
    std::fs::create_dir_all(&dir).ok()?;
    *counter = counter.wrapping_add(1);
    let path = dir.join(format!("{:06}.bmp", counter));
    std::fs::write(&path, bmp_bytes(frame, 960, 720)).ok()?;
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|name| name.extension().is_some_and(|ext| ext == "bmp"))
        .collect();
    names.sort();
    for stale in names.iter().rev().skip(8) {
        let _ = std::fs::remove_file(stale);
    }
    Some(path)
}

fn bmp_bytes(frame: &Frame, max_width: u32, max_height: u32) -> Vec<u8> {
    let (width, height, scale) = preview_size(frame.width(), frame.height(), max_width, max_height);
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
    bmp
}

fn preview_size(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32, f32) {
    let mut scale = 1.0f32;
    if width > max_width {
        scale = width as f32 / max_width as f32;
    }
    let mut out_w = ((width as f32 / scale).round() as u32).max(1);
    let mut out_h = ((height as f32 / scale).round() as u32).max(1);
    if out_h > max_height {
        scale *= out_h as f32 / max_height as f32;
        out_w = ((width as f32 / scale).round() as u32).max(1);
        out_h = max_height;
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
    fn glued_subtitle_words_are_not_chaos() {
        // The recogniser glues small subtitle words together; the length rule used to
        // throw these real lines away and the owner counted it as "translates only half".
        assert!(!super::chaotic("NOBODYLIKESITDARKER"));
        assert!(!super::chaotic("LKEEPTHINGSTICKINGALONG."));
        assert!(super::chaotic("veppyHuTOxnLEx+"));
    }

    #[test]
    fn a_transliterated_logo_is_not_a_translation() {
        use super::echo_of_source;
        assert!(echo_of_source("Makap", "Макап"));
        assert!(echo_of_source("SPUK", "СПУК"));
        assert!(echo_of_source("PI", "ПИ"));
        assert!(echo_of_source("5cc BAC", "5cc БАК"));
        assert!(!echo_of_source("HORSE!", "ЛОШАДЬ!"));
        assert!(!echo_of_source("WHAT THE", "ЧТО"));
        assert!(!echo_of_source("computer", "компьютер"));
        assert!(!echo_of_source("JERRY", "ДЖЕРРИ"));
    }

    #[test]
    fn overlapping_subtitle_lines_are_one_bubble() {
        use super::merge_bubbles;
        use lumen_core::Rect;
        let merged = merge_bubbles(vec![
            (Rect::new(762, 522, 293, 29), "LOSE YOUR WHOLE".to_owned(), 0.80),
            (Rect::new(772, 546, 276, 77), "DAMN FAMILY".to_owned(), 0.99),
            (Rect::new(120, 900, 300, 26), "ANOTHER COLUMN".to_owned(), 0.90),
        ]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].parts.len(), 2);
        assert_eq!(merged[1].parts.len(), 1);
    }

    #[test]
    fn a_letterbox_floor_is_not_text() {
        use super::{band_sig, chrome_line, picture_same, stray_token};
        use lumen_core::Frame;
        assert!(stray_token("ev]"));
        assert!(stray_token("PI"));
        assert!(!stray_token("HORSE!"));
        assert!(chrome_line("0 www.youtube.com"));
        assert!(chrome_line("https://example.org/lesson"));
        assert!(!chrome_line("HORSE!"));
        let frame = Frame::filled(64, 64, [0, 0, 0, 255]).expect("frame");
        let sig = band_sig(&frame, 0, 64);
        assert_eq!(sig.len(), 200);
        assert!(sig.iter().all(|value| *value == 0));
        let other = Frame::filled(64, 64, [255, 255, 255, 255]).expect("frame");
        assert!(!picture_same(&sig, &band_sig(&other, 0, 64)));
    }

    #[test]
    fn plates_glide_towards_their_source() {
        let from = lumen_core::Rect::new(0, 0, 100, 20);
        let to = lumen_core::Rect::new(40, 20, 120, 24);
        let mid = super::lerp_rect(&from, &to, 0.5);
        assert_eq!((mid.x, mid.y, mid.width, mid.height), (20, 10, 110, 22));
        let end = super::lerp_rect(&from, &to, 1.0);
        assert_eq!(end, to);
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
    crate::settings::data_dir()
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
    // A fresh journal per launch: the owner reads one run at a time, and yesterday's run has
    // no bearing on today's bug. Re-opens inside the same run (after a loop panic) append, so
    // the panic trail is not lost.
    static FRESH: AtomicBool = AtomicBool::new(false);
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = log_file_path();
    // Keep the journal bounded inside the app folder.
    if std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0) > 4 * 1024 * 1024 {
        let rotated = dir.join("live.log.prev");
        let _ = std::fs::remove_file(&rotated);
        let _ = std::fs::rename(&path, rotated);
    }
    let fresh = !FRESH.swap(true, Ordering::SeqCst);
    let mut options = OpenOptions::new();
    options.create(true);
    if fresh {
        options.write(true).truncate(true);
    } else {
        options.append(true);
    }
    options.open(path).unwrap_or_else(|_| fallback_log())
}

/// The app never writes outside its own folder: if even that is unavailable, the journal goes
/// to the null device instead of some stray temp file.
fn fallback_log() -> std::fs::File {
    OpenOptions::new().write(true).open("NUL").expect("log")
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
