//! Windows.Media.Ocr. Reads pixels the capture adapter already copied. No language pack is
//! downloaded here: English is requested first, then whatever recognisers the PC already has.
//! A Russian Windows install usually still reads Latin letters with its own pack.

use lumen_core::{Frame, Rect};

use crate::{OcrEngine, OcrError, Recognition};

pub struct WindowsOcr {
    engine: windows::Media::Ocr::OcrEngine,
}

impl WindowsOcr {
    pub fn new() -> Result<Self, OcrError> {
        let engine = create_engine().map_err(|detail| OcrError::EngineFailed {
            engine: "windows-ocr".to_owned(),
            detail,
        })?;
        Ok(Self { engine })
    }
}

impl OcrEngine for WindowsOcr {
    fn name(&self) -> &str {
        "windows-ocr"
    }

    fn recognize(&mut self, frame: &Frame, regions: &[Rect]) -> Result<Vec<Recognition>, OcrError> {
        let regions = if regions.is_empty() {
            vec![frame.bounds()]
        } else {
            regions.to_vec()
        };
        let mut found = Vec::new();
        for region in regions {
            let Some(crop) = frame.crop(region) else {
                continue;
            };
            match recognise_crop(&self.engine, &crop) {
                Ok(lines) => {
                    for mut line in lines {
                        line.bounds.x += region.x;
                        line.bounds.y += region.y;
                        if !line.text.trim().is_empty() {
                            found.push(line);
                        }
                    }
                }
                Err(detail) => {
                    return Err(OcrError::EngineFailed {
                        engine: "windows-ocr".to_owned(),
                        detail,
                    });
                }
            }
        }
        Ok(found)
    }
}

fn create_engine() -> Result<windows::Media::Ocr::OcrEngine, String> {
    use windows::Globalization::Language;
    use windows::Media::Ocr::OcrEngine;
    use windows::core::HSTRING;

    for tag in ["en-US", "en"] {
        let language = Language::CreateLanguage(&HSTRING::from(tag)).map_err(|error| error.message())?;
        if OcrEngine::IsLanguageSupported(&language).unwrap_or(false) {
            return OcrEngine::TryCreateFromLanguage(&language).map_err(|error| error.message());
        }
    }
    OcrEngine::TryCreateFromUserProfileLanguages().map_err(|error| error.message())
}

fn recognise_crop(
    engine: &windows::Media::Ocr::OcrEngine,
    frame: &Frame,
) -> Result<Vec<Recognition>, String> {
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
    use windows::Storage::Streams::DataWriter;

    let writer = DataWriter::new().map_err(|error| error.message())?;
    writer
        .WriteBytes(frame.as_bytes())
        .map_err(|error| error.message())?;
    let buffer = writer.DetachBuffer().map_err(|error| error.message())?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        frame.width() as i32,
        frame.height() as i32,
        BitmapAlphaMode::Ignore,
    )
    .map_err(|error| error.message())?;

    let result = engine
        .RecognizeAsync(&bitmap)
        .map_err(|error| error.message())?
        .get()
        .map_err(|error| error.message())?;
    let lines = result.Lines().map_err(|error| error.message())?;
    let mut recognised = Vec::new();
    for index in 0..lines.Size().map_err(|error| error.message())? {
        let line = lines.GetAt(index).map_err(|error| error.message())?;
        let text = line.Text().map_err(|error| error.message())?.to_string();
        let words = line.Words().map_err(|error| error.message())?;
        let mut left = i32::MAX;
        let mut top = i32::MAX;
        let mut right = 0i32;
        let mut bottom = 0i32;
        let count = words.Size().map_err(|error| error.message())?;
        for word_index in 0..count {
            let word = words.GetAt(word_index).map_err(|error| error.message())?;
            let bounds = word.BoundingRect().map_err(|error| error.message())?;
            left = left.min(bounds.X.floor() as i32);
            top = top.min(bounds.Y.floor() as i32);
            right = right.max((bounds.X + bounds.Width).ceil() as i32);
            bottom = bottom.max((bounds.Y + bounds.Height).ceil() as i32);
        }
        if count == 0 || right <= left || bottom <= top {
            continue;
        }
        recognised.push(Recognition {
            text,
            bounds: Rect::new(left, top, (right - left) as u32, (bottom - top) as u32),
            // WinRT OCR does not report a confidence. A constant keeps the overlay from
            // treating every line as unread.
            confidence: 0.85,
        });
    }
    Ok(recognised)
}
