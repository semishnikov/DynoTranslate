//! Bundled faces and the font system that loads them.
//!
//! Six DejaVu faces cover Latin, Cyrillic, Greek, Hebrew and Arabic with regular and bold
//! weights in sans, serif and monospace families — enough for font matching to have a real
//! choice, and enough coverage for the reference languages. The bytes live in the binary so a
//! clean machine and a build agent see the same database.

use std::sync::Arc;

use cosmic_text::FontSystem;

/// Why the font library could not be built.
#[derive(Debug, thiserror::Error)]
pub enum FontLibraryError {
    #[error("a bundled font file could not be read: {name}")]
    MissingFont { name: &'static str },
}

/// The bundled font files, in load order. Later faces do not shadow earlier ones: each file
/// registers its own family, weight and style for the matcher to choose from.
const BUNDLED: &[(&'static str, &[u8])] = &[
    ("DejaVuSans.ttf", include_bytes!("../fonts/DejaVuSans.ttf")),
    (
        "DejaVuSans-Bold.ttf",
        include_bytes!("../fonts/DejaVuSans-Bold.ttf"),
    ),
    ("DejaVuSerif.ttf", include_bytes!("../fonts/DejaVuSerif.ttf")),
    (
        "DejaVuSerif-Bold.ttf",
        include_bytes!("../fonts/DejaVuSerif-Bold.ttf"),
    ),
    (
        "DejaVuSansMono.ttf",
        include_bytes!("../fonts/DejaVuSansMono.ttf"),
    ),
    (
        "DejaVuSansMono-Bold.ttf",
        include_bytes!("../fonts/DejaVuSansMono-Bold.ttf"),
    ),
];

/// A `&'static [u8]` that satisfies fontdb's owned-source contract without copying the
/// embedded file.
struct StaticFont(&'static [u8]);

impl AsRef<[u8]> for StaticFont {
    fn as_ref(&self) -> &[u8] {
        self.0
    }
}

/// A font database holding only the bundled faces, with the generic families pointed at them
/// so `Family::SansSerif` and friends resolve deterministically.
pub struct FontLibrary {
    system: FontSystem,
}

impl FontLibrary {
    /// Builds the library from the embedded faces. Infallible today; the error is kept so a
    /// future face that fails to parse reports which file was at fault.
    pub fn bundled() -> Result<Self, FontLibraryError> {
        let sources = BUNDLED.iter().map(|(name, bytes)| {
            if bytes.is_empty() {
                return Err(FontLibraryError::MissingFont { name });
            }
            Ok(fontdb::Source::Binary(Arc::new(StaticFont(bytes))))
        })
        .collect::<Result<Vec<_>, _>>()?;

        let mut system = FontSystem::new_with_fonts(sources);
        // cosmic-text points the generic families at Open Sans and friends, which are not in
        // the bundle; retarget them so matching never depends on a font that is not there.
        system.db_mut().set_sans_serif_family("DejaVu Sans");
        system.db_mut().set_serif_family("DejaVu Serif");
        system.db_mut().set_monospace_family("DejaVu Sans Mono");
        Ok(Self { system })
    }

    pub fn system(&mut self) -> &mut FontSystem {
        &mut self.system
    }

    /// How many faces the database holds. The bundled set is six; the assertion in the tests
    /// uses this so a missing file fails as a count, not as blank ink.
    pub fn face_count(&self) -> usize {
        self.system.db().faces().count()
    }
}

impl std::fmt::Debug for FontLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontLibrary")
            .field("faces", &self.face_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic_text::{Attrs, Family, Shaping};

    #[test]
    fn the_bundled_library_loads_every_face() {
        let library = FontLibrary::bundled().expect("bundled fonts parse");
        assert_eq!(library.face_count(), 6);
    }

    #[test]
    fn the_generic_sans_family_resolves_to_a_bundled_face() {
        let mut library = FontLibrary::bundled().expect("bundled fonts parse");
        let query = fontdb::Query {
            families: &[fontdb::Family::SansSerif],
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        };
        assert!(library.system().db().query(&query).is_some());
    }

    #[test]
    fn bold_resolves_to_the_bold_face() {
        let mut library = FontLibrary::bundled().expect("bundled fonts parse");
        let query = fontdb::Query {
            families: &[fontdb::Family::SansSerif],
            weight: fontdb::Weight::BOLD,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        };
        let found = library.system().db().query(&query).expect("bold face");
        let face = library.system().db().face(found).expect("face row");
        assert_eq!(face.weight, fontdb::Weight::BOLD);
    }

    #[test]
    fn shaping_cyrillic_produces_glyphs() {
        let mut library = FontLibrary::bundled().expect("bundled fonts parse");
        let mut buffer = cosmic_text::Buffer::new(
            library.system(),
            cosmic_text::Metrics::new(16.0, 20.0),
        );
        buffer.set_size(Some(200.0), Some(40.0));
        buffer.set_text(
            "Настройки",
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(library.system(), false);
        let glyphs: usize = buffer.layout_runs().map(|run| run.glyphs.len()).sum();
        assert!(glyphs >= 9, "expected one glyph per letter, got {glyphs}");
    }
}
