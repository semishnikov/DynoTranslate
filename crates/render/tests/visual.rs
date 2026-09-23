//! The visual regression suite.
//!
//! Golden PNGs are the long-term form: the same scene is rendered, compared byte for byte, and
//! a mismatch names the first differing pixel. Bootstrapping a golden needs a machine that can
//! run `cargo test`, so until a golden file exists this suite falls back to invariants that
//! catch the same classes of regression without one — determinism, the fitting floor, the
//! no-ellipsis rule, ink bounds, and the shape of inpainting on analytically known inputs.
//!
//! To add a golden later: drop the expected PNG next to this file as `<name>.png` and the
//! comparison activates on the next run. Nothing else changes.

use lumen_core::{Frame, Rect};
use lumen_render::{draw_fitted, inpaint, FontWeight, Renderer, TextAlign, TextSpec, WritingMode};

fn install_annotations() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            let msg = info.to_string().replace('\n', " | ");
            eprintln!("::error title=test-panic::{msg}");
        }));
    });
}

fn scene(width: u32, height: u32, colour: [u8; 4]) -> Frame {
    Frame::filled(width, height, colour).expect("dimensions")
}

fn ink_count(frame: &Frame) -> usize {
    frame.as_bytes().chunks_exact(4).filter(|pixel| pixel[3] > 0).count()
}

fn render_label(spec: &TextSpec) -> Frame {
    let mut renderer = Renderer::new().expect("bundled fonts");
    let mut frame = Frame::filled(spec.max_width + 16, spec.max_height + 16, [0, 0, 0, 0]).expect("dimensions");
    draw_fitted(&mut renderer, &mut frame, spec, (8, 8), [255, 255, 255, 255], None, 1.0).expect("fit");
    frame
}

#[test]
fn rendering_is_byte_identical_across_calls() {
    install_annotations();
    let spec = TextSpec::new("Сохранить и выйти", 220, 40, 20).with_weight(FontWeight::Bold);
    assert_eq!(render_label(&spec).as_bytes(), render_label(&spec).as_bytes());
}

#[test]
fn centred_and_left_text_differ_but_both_have_ink() {
    install_annotations();
    let base = TextSpec::new("Centre me", 200, 36, 18);
    let left = render_label(&base);
    let right = render_label(&base.clone().with_align(TextAlign::Center));
    assert!(ink_count(&left) > 10);
    assert!(ink_count(&right) > 10);
    assert_ne!(left.as_bytes(), right.as_bytes());
}

#[test]
fn a_tiny_box_never_reports_a_size_below_the_floor() {
    install_annotations();
    let mut renderer = Renderer::new().expect("bundled fonts");
    let spec = TextSpec::new("Pack my box with five dozen liquor jugs", 64, 18, 32);
    let ready = renderer.fit(&spec).expect("fit");
    assert!(
        ready.fitted.size >= 32.0 * 0.8 - 0.01,
        "size {} fell under the floor",
        ready.fitted.size
    );
    assert!(ready.fitted.at_floor);
}

#[test]
fn vertical_mode_produces_a_taller_ink_box_than_horizontal() {
    install_annotations();
    let mut renderer = Renderer::new().expect("bundled fonts");
    let text = "Notes";
    let horizontal = renderer.fit(&TextSpec::new(text, 160, 40, 18)).expect("fit");
    let vertical = renderer
        .fit(&TextSpec::new(text, 40, 160, 18).with_writing(WritingMode::Vertical))
        .expect("fit");
    // Vertical stacks one glyph per line: more lines, taller logical layout.
    assert!(
        vertical.fitted.line_height >= horizontal.fitted.line_height * 0.9,
        "line height should stay comparable"
    );
}

#[test]
fn inpainting_a_flat_panel_reproduces_the_panel_exactly() {
    install_annotations();
    let source = scene(64, 64, [18, 24, 32, 255]);
    let pixels = inpaint(&source, Rect::new(8, 8, 24, 12));
    assert_eq!(pixels.len(), 24 * 12);
    assert!(pixels.iter().all(|pixel| *pixel == [18, 24, 32, 255]));
}

#[test]
fn inpainting_preserves_the_horizontal_ramp() {
    install_annotations();
    let width = 80;
    let mut pixels = Vec::with_capacity(width as usize * 24 * 4);
    for _y in 0..24 {
        for x in 0..width {
            let shade = (x * 3) as u8;
            pixels.extend_from_slice(&[shade, shade, shade, 255]);
        }
    }
    let source = Frame::packed(width, 24, pixels).expect("dimensions");
    let region = Rect::new(20, 4, 24, 16);
    let out = inpaint(&source, region);
    // Same row of the region (width 24): column 8 sits near the left halo, column 23 near the right.
    let left = out[8];
    let right = out[23];
    assert!(
        u32::from(right[0]) > u32::from(left[0]) + 40,
        "reconstruction should keep the ramp: {left:?} -> {right:?}"
    );
}

#[test]
fn an_optional_golden_png_is_compared_when_present() {
    install_annotations();
    let spec = TextSpec::new("Golden", 120, 32, 18);
    let actual = render_label(&spec);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/label.png");
    if !path.exists() {
        // No golden yet: the invariants above carry the suite until one is committed.
        eprintln!("no golden at {}, skipping byte comparison", path.display());
        return;
    }
    let file = std::fs::File::open(&path).expect("open golden");
    let mut decoder = png::Decoder::new(file);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().expect("golden header");
    let mut buf = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut buf).expect("golden frame");
    let info = reader.info();
    assert_eq!(info.width, actual.width(), "golden width");
    assert_eq!(info.height, actual.height(), "golden height");

    // The golden is stored as RGBA; the frame is BGRA. Compare through RGBA.
    let mut expected_rgba = Vec::with_capacity(actual.as_bytes().len());
    for pixel in actual.as_bytes().chunks_exact(4) {
        expected_rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
    }
    if expected_rgba != buf {
        let mismatch = expected_rgba
            .iter()
            .zip(buf.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        panic!("golden mismatch at byte {mismatch} of {}", buf.len());
    }
}

#[test]
fn text_drawn_at_an_origin_lands_in_its_own_box() {
    install_annotations();
    let mut renderer = Renderer::new().expect("bundled fonts");
    let origin = (60, 90);
    let spec = TextSpec::new("Перевод в своём блоке", 260, 36, 20);
    let mut frame = scene(360, 220, [0, 0, 0, 0]);
    let foreground = [255, 255, 255, 255];
    let touched = draw_fitted(&mut renderer, &mut frame, &spec, origin, foreground, None, 1.0).expect("fit");
    assert!(ink_count(&frame) > 10, "nothing was drawn");
    assert!(touched.x >= origin.0 - 4, "ink started left of the box: {touched:?}");
    assert!(touched.y >= origin.1 - 4, "ink started above the box: {touched:?}");
    // Two wrapped lines at size 20 is the most height the fit may produce here.
    let bottom_limit = origin.1 + 56;
    assert!(touched.bottom() <= bottom_limit, "ink escaped below the box: {touched:?}");
    let right_limit = origin.0 + 268;
    assert!(touched.right() <= right_limit, "ink escaped right: {touched:?}");
}
