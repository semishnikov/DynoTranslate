//! Reads English text with models that ship with the program.
//!
//! Stripped Windows builds do not include a system recogniser. These files are downloaded once
//! into the local data folder, the same way the translator is, and then run with the runtime that
//! is already inside the installer.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use lumen_core::{Frame, Rect};
use lumen_ocr::Recognition;
use ort::session::Session;
use ort::value::Tensor;

const DET_URLS: &[&str] = &[
    "https://huggingface.co/SWHL/RapidOCR/resolve/main/PP-OCRv4/en_PP-OCRv3_det_infer.onnx",
    "https://media.githubusercontent.com/media/e-supple/process-medical-records/main/ocr_models/models--SWHL--RapidOCR/snapshots/1cfba2e90fc938db55889873735088de210cc173/PP-OCRv4/en_PP-OCRv3_det_infer.onnx",
];
const REC_URLS: &[&str] = &[
    "https://huggingface.co/SWHL/RapidOCR/resolve/main/PP-OCRv3/en_PP-OCRv3_rec_infer.onnx",
    "https://media.githubusercontent.com/media/e-supple/process-medical-records/main/ocr_models/models--SWHL--RapidOCR/snapshots/1cfba2e90fc938db55889873735088de210cc173/PP-OCRv3/en_PP-OCRv3_rec_infer.onnx",
];

/// Character list for the English PP-OCRv3 recogniser, in file order. Model index 0 is the blank.
const ALPHABET: &str = r###"0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\]^_`abcdefghijklmnopqrstuvwxyz{|}~!"#$%&'()*+,-./ "###;

pub struct Reader {
    detect: Session,
    recognize: Session,
    rec_width: Option<u32>,
    det_failures: u8,
}

impl Reader {
    pub fn load(log: &mut dyn Write, mut report: impl FnMut(&str)) -> Result<Self, String> {
        let dir = data_dir()?;
        let detect_path = ensure(&dir, "ocr-det.onnx", DET_URLS, 1_800_000, log, &mut report)?;
        let recognize_path = ensure(&dir, "ocr-rec.onnx", REC_URLS, 7_000_000, log, &mut report)?;
        report("ocr-sessions");
        let _ = writeln!(log, "loading text reader");
        let detect = open_session(&detect_path).map_err(|error| format!("detect: {error}"))?;
        let recognize = open_session(&recognize_path).map_err(|error| format!("recognize: {error}"))?;
        let _ = writeln!(log, "detect inputs: {}", names(detect.inputs()));
        let _ = writeln!(log, "recognize inputs: {}", names(recognize.inputs()));
        Ok(Self {
            detect,
            recognize,
            rec_width: None,
            det_failures: 0,
        })
    }

    pub fn read(&mut self, frame: &Frame) -> Result<Vec<Recognition>, String> {
        if frame.width() < 8 || frame.height() < 8 {
            return Ok(Vec::new());
        }
        let detected = if self.det_failures >= 2 {
            Vec::new()
        } else {
            match self.detect_lines(frame) {
                Ok(lines) => {
                    self.det_failures = 0;
                    lines
                }
                Err(error) => {
                    self.det_failures = self.det_failures.saturating_add(1);
                    if self.det_failures >= 2 {
                        return self.recognize_lines(frame, &edge_lines(frame)).map_err(|_| error);
                    }
                    Vec::new()
                }
            }
        };
        let found = match self.recognize_lines(frame, &detected) {
            Ok(found) if letter_count(&found) >= 3 => return Ok(found),
            Ok(found) => found,
            Err(_) => Vec::new(),
        };
        let fallback = self.recognize_lines(frame, &edge_lines(frame))?;
        if letter_count(&fallback) > letter_count(&found) {
            Ok(fallback)
        } else {
            Ok(found)
        }
    }

    fn detect_lines(&mut self, frame: &Frame) -> Result<Vec<Rect>, String> {
        let (width, height) = det_size(frame.width(), frame.height());
        let data = resize_norm(frame, width, height, width);
        let (shape, map) = run_session(&mut self.detect, data, vec![1, 3, height as i64, width as i64])?;
        let (map_w, map_h, mut values) = probability_map(&shape, &map)?;
        squeeze_scores(&mut values);
        let boxes = components(&values, map_w, map_h);
        let scale_x = frame.width() as f32 / map_w as f32;
        let scale_y = frame.height() as f32 / map_h as f32;
        let frame_area = u64::from(frame.width()) * u64::from(frame.height());
        let mut scaled = Vec::new();
        for spot in boxes {
            let grown = unclip(spot);
            let x = (grown.x as f32 * scale_x).floor() as i32 - 2;
            let y = (grown.y as f32 * scale_y).floor() as i32 - 2;
            let w = ((grown.width as f32 * scale_x).ceil() as u32).saturating_add(4);
            let h = ((grown.height as f32 * scale_y).ceil() as u32).saturating_add(4);
            let Some(rect) = Rect::new(x, y, w.max(1), h.max(1)).clamp_to(&frame.bounds()) else {
                continue;
            };
            if rect.width < 8 || rect.height < 6 {
                continue;
            }
            if u64::from(rect.width) * u64::from(rect.height) * 3 > frame_area {
                continue;
            }
            if rect.height * 4 > frame.height() {
                continue;
            }
            scaled.push(rect);
        }
        Ok(group_lines(scaled))
    }

    fn recognize_lines(&mut self, frame: &Frame, lines: &[Rect]) -> Result<Vec<Recognition>, String> {
        let mut found = Vec::new();
        let mut last_error = None;
        for line in lines.iter().take(12) {
            let Some(crop) = frame.crop(*line) else {
                continue;
            };
            if crop.width() < 8 || crop.height() < 6 {
                continue;
            }
            match self.read_line(&crop) {
                Ok((text, confidence)) => {
                    let text = text.trim().to_owned();
                    if text.chars().any(|ch| ch.is_ascii_alphabetic()) {
                        found.push(Recognition {
                            text,
                            bounds: *line,
                            confidence,
                        });
                    }
                }
                Err(error) => last_error = Some(error),
            }
        }
        if found.is_empty() {
            if let Some(error) = last_error {
                return Err(error);
            }
        }
        Ok(found)
    }

    /// One visual line, with spaces put back between words.
    ///
    /// The recogniser often returns `Hello.Openthedoor.Newgame`. The picture still has the gaps,
    /// so each word is read on its own and joined. The translator then sees the whole sentence.
    fn read_line(&mut self, crop: &Frame) -> Result<(String, f32), String> {
        let (whole, confidence) = self.recognize_crop(crop)?;
        let whole = whole.trim();
        if whole.is_empty() {
            return Ok((String::new(), confidence));
        }
        if whole.contains(' ') {
            return Ok((whole.to_owned(), confidence));
        }
        let spans = word_spans(crop);
        if spans.len() < 2 || spans.len() > 16 {
            return Ok((loosen(whole), confidence));
        }
        let mut parts = Vec::new();
        let mut score = 0.0f32;
        let mut kept = 0u32;
        for (x, width) in spans {
            let Some(word) = crop.crop(Rect::new(x as i32, 0, width, crop.height())) else {
                continue;
            };
            if word.width() < 2 || word.height() < 6 {
                continue;
            }
            let Ok((text, part_score)) = self.recognize_crop(&word) else {
                continue;
            };
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            if text.chars().all(|ch| !ch.is_alphanumeric()) {
                if let Some(last) = parts.last_mut() {
                    last.push_str(text);
                    continue;
                }
            }
            parts.push(text.to_owned());
            score += part_score;
            kept += 1;
        }
        let joined = parts.join(" ");
        if kept >= 2 && letters(&joined) + 1 >= letters(whole) {
            let confidence = if kept == 0 { confidence } else { score / (kept as f32) };
            return Ok((joined, confidence));
        }
        Ok((loosen(whole), confidence))
    }

    fn recognize_crop(&mut self, crop: &Frame) -> Result<(String, f32), String> {
        let natural = ((48.0 * crop.width() as f32 / crop.height() as f32).ceil() as u32).clamp(8, 960);
        let aligned = ((natural + 7) / 8) * 8;
        if let Some(fixed) = self.rec_width {
            return self.recognize_at(crop, natural.min(fixed), fixed);
        }
        match self.recognize_at(crop, natural, aligned.min(960)) {
            Ok(read) => Ok(read),
            Err(error) => match self.recognize_at(crop, natural.min(320), 320) {
                Ok(read) => {
                    self.rec_width = Some(320);
                    Ok(read)
                }
                Err(_) => Err(error),
            },
        }
    }

    fn recognize_at(&mut self, crop: &Frame, used: u32, total: u32) -> Result<(String, f32), String> {
        let data = resize_norm(crop, used.max(8), 48, total.max(used).max(8));
        let width = total.max(used).max(8);
        let (shape, scores) = run_session(&mut self.recognize, data, vec![1, 3, 48, width as i64])?;
        Ok(decode_text(&shape, &scores))
    }
}

fn open_session(path: &Path) -> Result<Session, String> {
    Session::builder()
        .map_err(|error| error.to_string())?
        .with_intra_threads(2)
        .map_err(|error| error.to_string())?
        .commit_from_file(path)
        .map_err(|error| error.to_string())
}

fn run_session(session: &mut Session, data: Vec<f32>, shape: Vec<i64>) -> Result<(Vec<i64>, Vec<f32>), String> {
    let name = session
        .inputs()
        .iter()
        .next()
        .map(|input| input.name().to_owned())
        .unwrap_or_else(|| "x".to_owned());
    let tensor = Tensor::from_array((shape, data)).map_err(|error| error.to_string())?;
    let outputs = session
        .run(ort::inputs![name.as_str() => tensor])
        .map_err(|error| error.to_string())?;
    first_output(&outputs)
}

fn first_output(outputs: &ort::session::SessionOutputs<'_>) -> Result<(Vec<i64>, Vec<f32>), String> {
    let names: Vec<String> = outputs.iter().map(|(name, _)| name.to_owned()).collect();
    let name = names.first().ok_or_else(|| "reader returned no output".to_owned())?;
    let value = outputs
        .get(name.as_str())
        .ok_or_else(|| format!("missing output {name}"))?;
    let (shape, data) = value
        .try_extract_tensor::<f32>()
        .map_err(|error| format!("{name}: {error}"))?;
    Ok((shape.to_vec(), data.to_vec()))
}

fn names(outlets: &[ort::value::Outlet]) -> String {
    outlets.iter().map(|outlet| outlet.name()).collect::<Vec<_>>().join(", ")
}

fn det_size(width: u32, height: u32) -> (u32, u32) {
    let longest = width.max(height) as f32;
    let ratio = if longest > 960.0 { 960.0 / longest } else { 1.0 };
    let width = (((width as f32 * ratio) / 32.0).round() as u32 * 32).max(32);
    let height = (((height as f32 * ratio) / 32.0).round() as u32 * 32).max(32);
    (width, height)
}

fn resize_norm(frame: &Frame, used_w: u32, height: u32, total_w: u32) -> Vec<f32> {
    let total_w = total_w.max(used_w).max(1);
    let height = height.max(1);
    let plane = total_w as usize * height as usize;
    let mut data = vec![0f32; plane * 3];
    for y in 0..height {
        for x in 0..used_w.min(total_w) {
            let sx = (x as f32 + 0.5) * frame.width() as f32 / used_w as f32 - 0.5;
            let sy = (y as f32 + 0.5) * frame.height() as f32 / height as f32 - 0.5;
            let [b, g, r] = bilinear(frame, sx, sy);
            let index = y as usize * total_w as usize + x as usize;
            data[index] = norm(b);
            data[plane + index] = norm(g);
            data[plane * 2 + index] = norm(r);
        }
    }
    data
}

fn norm(channel: u8) -> f32 {
    (f32::from(channel) / 255.0 - 0.5) / 0.5
}

fn bilinear(frame: &Frame, x: f32, y: f32) -> [u8; 3] {
    let max_x = frame.width().saturating_sub(1) as f32;
    let max_y = frame.height().saturating_sub(1) as f32;
    let x = x.clamp(0.0, max_x);
    let y = y.clamp(0.0, max_y);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(frame.width().saturating_sub(1));
    let y1 = (y0 + 1).min(frame.height().saturating_sub(1));
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let p00 = frame.pixel(x0, y0);
    let p10 = frame.pixel(x1, y0);
    let p01 = frame.pixel(x0, y1);
    let p11 = frame.pixel(x1, y1);
    let mut out = [0u8; 3];
    for channel in 0..3 {
        let top = f32::from(p00[channel]) * (1.0 - fx) + f32::from(p10[channel]) * fx;
        let bottom = f32::from(p01[channel]) * (1.0 - fx) + f32::from(p11[channel]) * fx;
        out[channel] = (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8;
    }
    out
}

fn probability_map(shape: &[i64], values: &[f32]) -> Result<(u32, u32, Vec<f32>), String> {
    let dims: Vec<usize> = shape.iter().copied().map(|dim| dim.max(0) as usize).collect();
    let dims = if dims.first() == Some(&1) { &dims[1..] } else { &dims[..] };
    match dims {
        [1, height, width] if *height > 0 && *width > 0 => {
            Ok((*width as u32, *height as u32, values.to_vec()))
        }
        [height, width] if *height > 0 && *width > 0 => {
            Ok((*width as u32, *height as u32, values.to_vec()))
        }
        [channels, height, width] if *channels <= 4 && *height > 8 && *width > 8 => {
            let plane = height * width;
            Ok((*width as u32, *height as u32, values[..plane.min(values.len())].to_vec()))
        }
        [height, width, channels] if *channels <= 4 && *height > 8 && *width > 8 => {
            let mut planar = vec![0f32; height * width];
            for index in 0..planar.len() {
                planar[index] = values.get(index * channels).copied().unwrap_or(0.0);
            }
            Ok((*width as u32, *height as u32, planar))
        }
        _ => Err(format!("detect map shape {shape:?}")),
    }
}

fn squeeze_scores(values: &mut [f32]) {
    let mut highest = 0.0f32;
    let mut lowest = 0.0f32;
    for value in values.iter().copied() {
        highest = highest.max(value);
        lowest = lowest.min(value);
    }
    if highest <= 1.2 && lowest >= -0.05 {
        return;
    }
    for value in values {
        let exp = (-*value).exp();
        *value = 1.0 / (1.0 + exp);
    }
}

fn components(map: &[f32], width: u32, height: u32) -> Vec<Rect> {
    if width == 0 || height == 0 || map.len() < (width as usize) * (height as usize) {
        return Vec::new();
    }
    let mut seen = vec![false; (width as usize) * (height as usize)];
    let mut boxes = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let index = (y * width + x) as usize;
            if seen[index] || map[index] < 0.3 {
                continue;
            }
            let mut queue = vec![(x, y)];
            seen[index] = true;
            let mut min_x = x;
            let mut min_y = y;
            let mut max_x = x;
            let mut max_y = y;
            let mut area = 0u32;
            let mut score = 0f32;
            while let Some((cx, cy)) = queue.pop() {
                area += 1;
                score += map[(cy * width + cx) as usize];
                min_x = min_x.min(cx);
                min_y = min_y.min(cy);
                max_x = max_x.max(cx);
                max_y = max_y.max(cy);
                let mut around = [(0u32, 0u32); 4];
                let count = neighbor_count(cx, cy, width, height, &mut around);
                for (nx, ny) in around.into_iter().take(count) {
                    let next = (ny * width + nx) as usize;
                    if !seen[next] && map[next] >= 0.3 {
                        seen[next] = true;
                        queue.push((nx, ny));
                    }
                }
            }
            let box_w = max_x - min_x + 1;
            let box_h = max_y - min_y + 1;
            if area < 12 || box_w < 3 || box_h < 3 || (score / area as f32) < 0.4 {
                continue;
            }
            boxes.push(Rect::new(min_x as i32, min_y as i32, box_w, box_h));
            if boxes.len() >= 80 {
                break;
            }
        }
    }
    boxes.sort_by_key(|spot| std::cmp::Reverse(spot.width.saturating_mul(spot.height)));
    boxes.truncate(40);
    boxes
}

fn neighbor_count(x: u32, y: u32, width: u32, height: u32, out: &mut [(u32, u32); 4]) -> usize {
    let mut count = 0;
    if x > 0 {
        out[count] = (x - 1, y);
        count += 1;
    }
    if y > 0 {
        out[count] = (x, y - 1);
        count += 1;
    }
    if x + 1 < width {
        out[count] = (x + 1, y);
        count += 1;
    }
    if y + 1 < height {
        out[count] = (x, y + 1);
        count += 1;
    }
    count
}

fn unclip(spot: Rect) -> Rect {
    let cx = spot.x as f32 + spot.width as f32 / 2.0;
    let cy = spot.y as f32 + spot.height as f32 / 2.0;
    let width = (spot.width as f32 * 1.4).max(1.0);
    let height = (spot.height as f32 * 1.5).max(1.0);
    Rect::new(
        (cx - width / 2.0).floor() as i32,
        (cy - height / 2.0).floor() as i32,
        width.ceil() as u32,
        height.ceil() as u32,
    )
}

fn group_lines(mut boxes: Vec<Rect>) -> Vec<Rect> {
    boxes.sort_by_key(|spot| spot.y);
    let mut lines = Vec::new();
    for spot in boxes {
        if let Some(line) = lines.last_mut() {
            let overlap = vertical_overlap(*line, spot);
            let slim = line.height.min(spot.height) as i32;
            if slim > 0 && overlap * 2 > slim {
                *line = line.union(&spot);
                continue;
            }
        }
        lines.push(spot);
    }
    lines
}

fn vertical_overlap(left: Rect, right: Rect) -> i32 {
    (left.bottom().min(right.bottom()) - left.y.max(right.y)).max(0)
}

fn edge_lines(frame: &Frame) -> Vec<Rect> {
    let width = frame.width();
    let height = frame.height();
    if width < 8 || height < 8 {
        return Vec::new();
    }
    let mut rows = vec![0u32; height as usize];
    for y in 0..height {
        let mut edges = 0u32;
        let mut previous = luma(frame.pixel(0, y));
        for x in 1..width {
            let current = luma(frame.pixel(x, y));
            if previous.abs_diff(current) > 28 {
                edges += 1;
            }
            previous = current;
        }
        rows[y as usize] = edges;
    }
    let threshold = (width / 90).max(4);
    let mut lines = Vec::new();
    let mut y = 0u32;
    while y < height {
        if rows[y as usize] < threshold {
            y += 1;
            continue;
        }
        let start = y;
        let mut gap = 0u32;
        while y < height {
            if rows[y as usize] >= threshold {
                gap = 0;
                y += 1;
            } else if gap < 2 {
                gap += 1;
                y += 1;
            } else {
                y = y.saturating_sub(gap);
                break;
            }
        }
        let end = y;
        if end.saturating_sub(start) < 4 {
            continue;
        }
        let top = start.saturating_sub(2);
        let bottom = (end + 2).min(height);
        if bottom.saturating_sub(top) * 4 > height {
            continue;
        }
        let mut left = width;
        let mut right = 0u32;
        for row in start..end {
            let mut previous = luma(frame.pixel(0, row));
            for x in 1..width {
                let current = luma(frame.pixel(x, row));
                if previous.abs_diff(current) > 28 {
                    left = left.min(x.saturating_sub(2));
                    right = right.max((x + 2).min(width));
                }
                previous = current;
            }
        }
        if right > left + 8 {
            lines.push(Rect::new(left as i32, top as i32, right - left, bottom - top));
        }
        if lines.len() >= 8 {
            break;
        }
    }
    lines
}

fn luma(pixel: [u8; 4]) -> u8 {
    ((u16::from(pixel[2]) * 77 + u16::from(pixel[1]) * 150 + u16::from(pixel[0]) * 29) / 256) as u8
}

fn decode_text(shape: &[i64], scores: &[f32]) -> (String, f32) {
    let Some((steps, class_count, classes_last)) = text_layout(shape) else {
        return (String::new(), 0.0);
    };
    if class_count == 0 || steps == 0 {
        return (String::new(), 0.0);
    }
    let mut text = String::new();
    let mut previous = 0usize;
    let mut confidence = 0.0f32;
    let mut kept = 0u32;
    for step in 0..steps {
        let mut best = 0usize;
        let mut best_score = f32::NEG_INFINITY;
        for class in 0..class_count {
            let index = if classes_last {
                step * class_count + class
            } else {
                class * steps + step
            };
            let score = scores.get(index).copied().unwrap_or(f32::NEG_INFINITY);
            if score > best_score {
                best_score = score;
                best = class;
            }
        }
        if best != 0 && best != previous {
            if let Some(ch) = ALPHABET.chars().nth(best - 1) {
                text.push(ch);
                confidence += best_score.clamp(0.0, 1.0);
                kept += 1;
            }
        }
        previous = best;
    }
    let confidence = if kept == 0 { 0.0 } else { confidence / kept as f32 };
    (text, confidence)
}

fn text_layout(shape: &[i64]) -> Option<(usize, usize, bool)> {
    if shape.is_empty() {
        return None;
    }
    let dims: Vec<i64> = if shape[0] == 1 && shape.len() > 2 {
        shape[1..].to_vec()
    } else {
        shape.to_vec()
    };
    if dims.len() != 2 || dims[0] <= 0 || dims[1] <= 0 {
        return None;
    }
    let classes = ALPHABET.chars().count() as i64 + 1;
    let classes_last = (dims[1] - classes).abs() <= (dims[0] - classes).abs();
    if classes_last {
        Some((dims[0] as usize, dims[1] as usize, true))
    } else {
        Some((dims[1] as usize, dims[0] as usize, false))
    }
}

fn letter_count(lines: &[Recognition]) -> usize {
    lines
        .iter()
        .map(|line| line.text.chars().filter(|ch| ch.is_ascii_alphabetic()).count())
        .sum()
}

fn data_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("DynoTranslate").join("ocr");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn ensure(
    dir: &Path,
    name: &str,
    urls: &[&str],
    minimum: u64,
    log: &mut dyn Write,
    report: &mut dyn FnMut(&str),
) -> Result<PathBuf, String> {
    let path = dir.join(name);
    if model_ok(&path, minimum) {
        return Ok(path);
    }
    let _ = writeln!(log, "downloading {name}");
    report(name);
    let partial = dir.join(format!("{name}.partial"));
    let mut last = format!("download {name} failed");
    for url in urls {
        if let Err(error) = download(url, &partial, &mut |bytes| {
            let mb = bytes / (1024 * 1024);
            if mb > 0 {
                report(&format!("{name}:{mb}"));
            }
        }) {
            last = format!("download {name}: {error}");
            let _ = writeln!(log, "{last}");
            let _ = fs::remove_file(&partial);
            continue;
        }
        if model_ok(&partial, minimum) {
            fs::rename(&partial, &path).map_err(|error| error.to_string())?;
            return Ok(path);
        }
        last = format!("{name} download was too small");
        let _ = fs::remove_file(&partial);
    }
    Err(last)
}

fn download(url: &str, path: &Path, progress: &mut dyn FnMut(u64)) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(180))
        .redirects(10)
        .build();
    let response = agent
        .get(url)
        .set("User-Agent", "DynoTranslate")
        .call()
        .map_err(|error| error.to_string())?;
    let mut reader = response.into_reader();
    let mut file = File::create(path).map_err(|error| error.to_string())?;
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    let mut reported = 0u64;
    loop {
        let read = reader.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(|error| error.to_string())?;
        total += read as u64;
        if total - reported >= 1024 * 1024 {
            reported = total;
            progress(total);
        }
    }
    file.flush().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paint(frame: &mut Frame, x: u32, width: u32) {
        for dx in 0..width {
            for y in 4..16 {
                frame.set_pixel(x + dx, y, [20, 20, 20, 255]);
            }
        }
    }

    #[test]
    fn a_notepad_line_splits_on_word_gaps() {
        let mut frame = Frame::filled(140, 22, [250, 250, 250, 255]).unwrap();
        let mut x = 4u32;
        for letters in [[3u32, 3, 1, 1, 3], [3, 3, 3, 1], [1, 3, 3], [3, 3, 3, 2], [3, 3, 3], [3, 1, 3, 3]] {
            for width in letters {
                paint(&mut frame, x, width);
                x += width + 1;
            }
            x += 5;
        }
        let spans = word_spans(&frame);
        assert_eq!(spans.len(), 6, "{spans:?}");
    }

    #[test]
    fn glued_punctuation_gets_a_space() {
        assert_eq!(loosen("Hello.Openthedoor.Newgame"), "Hello. Openthedoor. Newgame");
    }
}

/// Columns that belong to each word. A one- or two-pixel gap is letter spacing; a wider gap is a space.
fn word_spans(crop: &Frame) -> Vec<(u32, u32)> {
    let width = crop.width();
    let height = crop.height();
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let background = background_luminance(crop);
    let mut ink = vec![false; width as usize];
    for x in 0..width {
        for y in 0..height {
            if luminance(crop.pixel(x, y)).abs_diff(background) >= 48 {
                ink[x as usize] = true;
                break;
            }
        }
    }
    let mut runs = Vec::new();
    let mut index = 0usize;
    while index < ink.len() {
        if !ink[index] {
            index += 1;
            continue;
        }
        let start = index;
        while index < ink.len() && ink[index] {
            index += 1;
        }
        runs.push((start as u32, (index - start) as u32));
    }
    if runs.is_empty() {
        return runs;
    }
    let mut words = Vec::new();
    let (mut start, mut span) = runs[0];
    for &(next, next_span) in &runs[1..] {
        let gap = next.saturating_sub(start + span);
        if gap < 3 {
            span = next + next_span - start;
        } else {
            words.push((start, span));
            start = next;
            span = next_span;
        }
    }
    words.push((start, span));
    words
}

fn background_luminance(crop: &Frame) -> u8 {
    let mut histogram = [0u32; 256];
    let mut total = 0u32;
    for y in 0..crop.height() {
        for x in 0..crop.width() {
            histogram[luminance(crop.pixel(x, y)) as usize] += 1;
            total += 1;
        }
    }
    let mut seen = 0u32;
    for (value, count) in histogram.iter().enumerate() {
        seen += count;
        if seen * 2 >= total {
            return value as u8;
        }
    }
    255
}

fn luminance(pixel: [u8; 4]) -> u8 {
    let blue = pixel[0] as u16;
    let green = pixel[1] as u16;
    let red = pixel[2] as u16;
    ((red * 3 + green * 6 + blue) / 10) as u8
}

fn letters(text: &str) -> usize {
    text.chars().filter(|ch| ch.is_alphanumeric()).count()
}

/// `Hello.Openthedoor.Newgame` still has the sentence dots. Put the spaces back so each sentence can be translated.
fn loosen(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(chars.len() + 4);
    for (index, ch) in chars.iter().copied().enumerate() {
        out.push(ch);
        if matches!(ch, '.' | '!' | '?' | ',' | ':' | ';') {
            if let Some(next) = chars.get(index + 1).copied() {
                if !next.is_whitespace() {
                    out.push(' ');
                }
            }
        }
    }
    out
}

fn model_ok(path: &Path, minimum: u64) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if meta.len() < minimum {
        return false;
    }
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 8];
    file.read(&mut magic).ok() == Some(8) && magic[0] != b'<' && magic[0] != b'{' && &magic != b"version "
}
