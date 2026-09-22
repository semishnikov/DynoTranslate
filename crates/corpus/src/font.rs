//! The corpus font: every glyph as stroke skeletons on an integer grid.
//!
//! The corpus has to draw real text without depending on a font file, a licensing question or a
//! platform, so the glyphs here are polylines the renderer turns into antialiased capsules. The
//! design grid is doubled for precision: the baseline is `y = 0`, the x-height is `10`, the cap
//! height is `14` and descenders reach `-4`. Regular, bold and italic are the same skeletons with
//! a wider stroke and a shear, which is all the style variation the recognition gate needs.
//!
//! Letters that share a shape share a skeleton through [`ALIASES`]: Cyrillic `а` is Latin `a`,
//! Cyrillic `в` is Latin `B` scaled from the cap height down to the x-height, capital `Ж` is
//! lowercase `ж` scaled up. What the skeletons do not cover is honest about itself: [`glyph`]
//! returns nothing for a character the corpus cannot draw, and [`covers`] reports the first such
//! character in a phrase, so an unsupported script can never silently render as blank ink. Greek,
//! Han, kana, hangul, Arabic, Hebrew, Thai and Devanagari are not drawn yet; the corpus is built
//! so adding a script is a table addition, not a redesign.

use serde::{Deserialize, Serialize};

/// The cap height in design units. `size` at the call site means cap height in pixels.
pub const CAP_HEIGHT: f32 = 14.0;
/// The x-height in design units.
pub const X_HEIGHT: f32 = 10.0;
/// How far descenders reach below the baseline, in design units.
pub const DESCENDER: f32 = 4.0;
/// Italic shear: design units of x displacement per design unit of y.
pub const SLANT: f32 = 0.22;

/// Scaling a cap-height skeleton down to the x-height. Written out so no float arithmetic is
/// needed in a `static`, which keeps the crate inside its declared MSRV.
const CAP_TO_X: f32 = 0.7142857;
/// Scaling an x-height skeleton up to the cap height.
const X_TO_CAP: f32 = 1.4;

/// The stroke weight of a font style, in design units of radius.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontStyle {
    Regular,
    Bold,
    Italic,
}

impl FontStyle {
    pub fn all() -> &'static [FontStyle] {
        &[FontStyle::Regular, FontStyle::Bold, FontStyle::Italic]
    }

    /// Stroke radius in design units. Bold is roughly half again as heavy as the others; there is
    /// no bold-italic in the corpus because the gate does not need a fourth category.
    pub fn stroke_radius(self) -> f32 {
        match self {
            FontStyle::Bold => 1.7,
            FontStyle::Regular | FontStyle::Italic => 1.1,
        }
    }

    pub fn is_slanted(self) -> bool {
        matches!(self, FontStyle::Italic)
    }

    pub fn name(self) -> &'static str {
        match self {
            FontStyle::Regular => "regular",
            FontStyle::Bold => "bold",
            FontStyle::Italic => "italic",
        }
    }
}

/// One glyph: how far the pen moves after it, and the strokes to draw.
#[derive(Debug, Clone, Copy)]
pub struct GlyphDef {
    pub advance: f32,
    pub strokes: Strokes,
}

/// A polyline list: each stroke is a slice of `[x, y]` points on the design grid, y up.
pub type Strokes = &'static [&'static [[i16; 2]]];

const fn def(advance: f32, strokes: Strokes) -> GlyphDef {
    GlyphDef { advance, strokes }
}

// Latin lowercase, x-height band 0..10, ascenders to 14, descenders to -4.
static LA: Strokes = &[
    &[[16, 8], [14, 10], [8, 10], [4, 8], [4, 4], [6, 0], [12, 0], [16, 2]],
    &[[16, 10], [16, 0]],
];
static LB: Strokes = &[
    &[[4, 14], [4, 0]],
    &[[4, 6], [6, 10], [12, 10], [16, 6], [16, 4], [12, 0], [6, 0], [4, 2]],
];
static LC: Strokes = &[
    &[[16, 8], [12, 10], [6, 10], [4, 6], [4, 4], [6, 0], [12, 0], [16, 2]],
];
static LD: Strokes = &[
    &[[16, 6], [14, 10], [8, 10], [4, 6], [4, 4], [8, 0], [14, 0], [16, 2]],
    &[[16, 14], [16, 0]],
];
static LE: Strokes = &[
    &[
        [16, 6],
        [4, 6],
        [4, 8],
        [8, 10],
        [12, 10],
        [16, 8],
        [14, 2],
        [10, 0],
        [6, 0],
        [4, 2],
    ],
];
static LF: Strokes = &[&[[16, 14], [12, 14], [10, 12], [10, 0]], &[[6, 8], [14, 8]]];
static LG: Strokes = &[
    &[[16, 8], [14, 10], [8, 10], [4, 8], [4, 4], [6, 2], [12, 2], [16, 3]],
    &[[16, 10], [16, -2], [12, -4], [8, -4]],
];
static LH: Strokes = &[&[[4, 14], [4, 0]], &[[4, 8], [8, 10], [12, 10], [16, 8], [16, 0]]];
static LI: Strokes = &[&[[10, 13], [10, 13]], &[[10, 10], [10, 0]]];
static LJ: Strokes = &[&[[12, 13], [12, 13]], &[[12, 10], [12, -2], [8, -4], [4, -3]]];
static LK: Strokes = &[&[[4, 14], [4, 0]], &[[14, 10], [4, 4]], &[[7, 6], [14, 0]]];
static LL: Strokes = &[&[[8, 14], [8, 0]]];
static LM: Strokes = &[
    &[[4, 10], [4, 0]],
    &[[4, 8], [6, 10], [10, 9], [10, 0]],
    &[[10, 8], [12, 10], [16, 9], [16, 0]],
];
static LN: Strokes = &[&[[4, 10], [4, 0]], &[[4, 8], [8, 10], [12, 10], [16, 8], [16, 0]]];
static LO: Strokes = &[
    &[
        [6, 10],
        [12, 10],
        [16, 6],
        [16, 4],
        [12, 0],
        [6, 0],
        [4, 4],
        [4, 6],
        [6, 10],
    ],
];
static LP: Strokes = &[
    &[[4, 14], [4, -4]],
    &[[4, 6], [6, 10], [12, 10], [16, 6], [16, 4], [12, 0], [6, 0], [4, 2]],
];
static LQ: Strokes = &[
    &[[16, 6], [14, 10], [8, 10], [4, 6], [4, 4], [8, 0], [14, 0], [16, 2]],
    &[[16, 10], [16, -4]],
];
static LR: Strokes = &[&[[4, 10], [4, 0]], &[[4, 6], [8, 10], [14, 10]]];
static LS: Strokes = &[
    &[
        [16, 8],
        [12, 10],
        [6, 10],
        [4, 8],
        [6, 6],
        [12, 4],
        [14, 2],
        [12, 0],
        [6, 0],
        [4, 2],
    ],
];
static LT: Strokes = &[&[[8, 14], [8, 2], [11, 0], [14, 0]], &[[4, 9], [14, 9]]];
static LU: Strokes = &[&[[4, 10], [4, 2], [6, 0], [12, 0], [16, 2]], &[[16, 10], [16, 0]]];
static LV: Strokes = &[&[[4, 10], [10, 0], [16, 10]]];
static LW: Strokes = &[&[[4, 10], [6, 0], [10, 7], [14, 0], [16, 10]]];
static LX: Strokes = &[&[[4, 10], [16, 0]], &[[16, 10], [4, 0]]];
static LY: Strokes = &[&[[4, 10], [10, 2]], &[[16, 10], [8, -4], [4, -4]]];
static LZ: Strokes = &[&[[4, 10], [16, 10], [4, 0], [16, 0]]];

// Latin lowercase with diacritics.
static L_O_UMLAUT: Strokes = &[
    &[[6, 13], [6, 13]],
    &[[12, 13], [12, 13]],
    &[
        [6, 10],
        [12, 10],
        [16, 6],
        [16, 4],
        [12, 0],
        [6, 0],
        [4, 4],
        [4, 6],
        [6, 10],
    ],
];
static L_U_UMLAUT: Strokes = &[
    &[[6, 13], [6, 13]],
    &[[12, 13], [12, 13]],
    &[[4, 10], [4, 2], [6, 0], [12, 0], [16, 2]],
    &[[16, 10], [16, 0]],
];
static L_SHARP_S: Strokes = &[
    &[
        [6, -4],
        [6, 12],
        [10, 14],
        [14, 13],
        [14, 11],
        [10, 9],
        [14, 7],
        [14, 2],
        [10, 0],
        [8, 0],
    ],
];
static L_OE: Strokes = &[
    &[
        [3, 10],
        [7, 10],
        [9, 8],
        [9, 2],
        [7, 0],
        [3, 0],
        [1, 2],
        [1, 8],
        [3, 10],
    ],
    &[
        [19, 6],
        [11, 6],
        [11, 8],
        [14, 10],
        [17, 10],
        [19, 8],
        [18, 2],
        [15, 0],
        [12, 0],
        [11, 2],
    ],
];
static L_E_GRAVE: Strokes = &[
    &[[8, 15], [12, 13]],
    &[
        [16, 6],
        [4, 6],
        [4, 8],
        [8, 10],
        [12, 10],
        [16, 8],
        [14, 2],
        [10, 0],
        [6, 0],
        [4, 2],
    ],
];
static L_E_ACUTE: Strokes = &[
    &[[8, 13], [12, 15]],
    &[
        [16, 6],
        [4, 6],
        [4, 8],
        [8, 10],
        [12, 10],
        [16, 8],
        [14, 2],
        [10, 0],
        [6, 0],
        [4, 2],
    ],
];
static L_U_GRAVE: Strokes = &[
    &[[8, 15], [12, 13]],
    &[[4, 10], [4, 2], [6, 0], [12, 0], [16, 2]],
    &[[16, 10], [16, 0]],
];
static L_N_TILDE: Strokes = &[
    &[[5, 14], [8, 15], [11, 13], [14, 14]],
    &[[4, 10], [4, 0]],
    &[[4, 8], [8, 10], [12, 10], [16, 8], [16, 0]],
];
static L_A_TILDE: Strokes = &[
    &[[5, 14], [8, 15], [11, 13], [14, 14]],
    &[[16, 8], [14, 10], [8, 10], [4, 8], [4, 4], [6, 0], [12, 0], [16, 2]],
    &[[16, 10], [16, 0]],
];
static L_O_TILDE: Strokes = &[
    &[[5, 14], [8, 15], [11, 13], [14, 14]],
    &[
        [6, 10],
        [12, 10],
        [16, 6],
        [16, 4],
        [12, 0],
        [6, 0],
        [4, 4],
        [4, 6],
        [6, 10],
    ],
];
static L_L_STROKE: Strokes = &[&[[8, 14], [8, 0]], &[[5, 7], [11, 9]]];
static L_E_OGONEK: Strokes = &[
    &[
        [16, 6],
        [4, 6],
        [4, 8],
        [8, 10],
        [12, 10],
        [16, 8],
        [14, 2],
        [10, 0],
        [6, 0],
        [4, 2],
    ],
    &[[11, 0], [13, -1], [12, -3]],
];
static L_Z_DOT: Strokes = &[&[[10, 13], [10, 13]], &[[4, 10], [16, 10], [4, 0], [16, 0]]];
static L_S_ACUTE: Strokes = &[
    &[[8, 13], [12, 15]],
    &[
        [16, 8],
        [12, 10],
        [6, 10],
        [4, 8],
        [6, 6],
        [12, 4],
        [14, 2],
        [12, 0],
        [6, 0],
        [4, 2],
    ],
];
static L_C_ACUTE: Strokes = &[
    &[[8, 13], [12, 15]],
    &[[16, 8], [12, 10], [6, 10], [4, 6], [4, 4], [6, 0], [12, 0], [16, 2]],
];
static L_A_ACUTE: Strokes = &[
    &[[8, 13], [12, 15]],
    &[[16, 8], [14, 10], [8, 10], [4, 8], [4, 4], [6, 0], [12, 0], [16, 2]],
    &[[16, 10], [16, 0]],
];
static L_C_CARON: Strokes = &[
    &[[6, 15], [10, 12], [14, 15]],
    &[[16, 8], [12, 10], [6, 10], [4, 6], [4, 4], [6, 0], [12, 0], [16, 2]],
];
static L_E_CARON: Strokes = &[
    &[[6, 15], [10, 12], [14, 15]],
    &[
        [16, 6],
        [4, 6],
        [4, 8],
        [8, 10],
        [12, 10],
        [16, 8],
        [14, 2],
        [10, 0],
        [6, 0],
        [4, 2],
    ],
];
static L_I_ACUTE: Strokes = &[&[[8, 13], [12, 15]], &[[10, 10], [10, 0]]];
static L_R_CARON: Strokes = &[
    &[[4, 15], [8, 12], [12, 15]],
    &[[4, 10], [4, 0]],
    &[[4, 6], [8, 10], [14, 10]],
];
static L_Z_CARON: Strokes = &[&[[6, 15], [10, 12], [14, 15]], &[[4, 10], [16, 10], [4, 0], [16, 0]]];
static L_O_ACUTE: Strokes = &[
    &[[8, 13], [12, 15]],
    &[
        [6, 10],
        [12, 10],
        [16, 6],
        [16, 4],
        [12, 0],
        [6, 0],
        [4, 4],
        [4, 6],
        [6, 10],
    ],
];
static L_C_CEDILLA: Strokes = &[
    &[[16, 8], [12, 10], [6, 10], [4, 6], [4, 4], [6, 0], [12, 0], [16, 2]],
    &[[9, 0], [8, -2], [6, -3]],
];

// Latin capitals with diacritics, for the sentence-case first letters real menus have.
static U_L_STROKE: Strokes = &[&[[9, 14], [9, 0], [18, 0]], &[[5, 8], [13, 11]]];
static U_S_ACUTE: Strokes = &[
    &[[8, 17], [12, 18]],
    &[
        [16, 12],
        [12, 14],
        [7, 14],
        [4, 12],
        [6, 8],
        [12, 7],
        [15, 6],
        [16, 3],
        [13, 0],
        [7, 0],
        [4, 2],
    ],
];
static U_Z_DOT: Strokes = &[&[[10, 17], [10, 17]], &[[4, 14], [18, 14], [4, 0], [18, 0]]];
static U_C_CARON: Strokes = &[
    &[[7, 17], [11, 15], [15, 17]],
    &[[16, 12], [12, 14], [8, 14], [4, 11], [4, 3], [8, 0], [12, 0], [16, 2]],
];

// Latin capitals, cap band 0..14.
static UA: Strokes = &[&[[4, 0], [10, 14], [16, 0]], &[[7, 5], [13, 5]]];
static UB: Strokes = &[
    &[[4, 0], [4, 14]],
    &[[4, 14], [12, 14], [16, 12], [16, 9], [12, 7], [4, 7]],
    &[[4, 7], [13, 7], [16, 5], [16, 2], [12, 0], [4, 0]],
];
static UC: Strokes = &[
    &[[16, 12], [12, 14], [8, 14], [4, 11], [4, 3], [8, 0], [12, 0], [16, 2]],
];
static UD: Strokes = &[
    &[[4, 0], [4, 14]],
    &[[4, 14], [10, 14], [16, 10], [16, 4], [10, 0], [4, 0]],
];
static UE: Strokes = &[&[[16, 14], [4, 14], [4, 0], [16, 0]], &[[4, 7], [14, 7]]];
static UF: Strokes = &[&[[16, 14], [4, 14], [4, 0]], &[[4, 7], [14, 7]]];
static UG: Strokes = &[
    &[
        [16, 12],
        [12, 14],
        [8, 14],
        [4, 11],
        [4, 3],
        [8, 0],
        [12, 0],
        [16, 3],
        [16, 7],
        [11, 7],
    ],
];
static UH: Strokes = &[&[[4, 0], [4, 14]], &[[16, 0], [16, 14]], &[[4, 7], [16, 7]]];
static UI: Strokes = &[&[[10, 0], [10, 14]]];
static UJ: Strokes = &[&[[16, 14], [16, 3], [13, 0], [8, 0], [4, 3]]];
static UK: Strokes = &[&[[4, 0], [4, 14]], &[[16, 14], [4, 6]], &[[7, 8], [16, 0]]];
static UL: Strokes = &[&[[4, 14], [4, 0], [16, 0]]];
static UM: Strokes = &[&[[4, 0], [4, 14], [11, 6], [18, 14], [18, 0]]];
static UN: Strokes = &[&[[4, 0], [4, 14], [18, 0], [18, 14]]];
static UO: Strokes = &[
    &[
        [7, 14],
        [14, 14],
        [18, 10],
        [18, 4],
        [14, 0],
        [7, 0],
        [4, 4],
        [4, 10],
        [7, 14],
    ],
];
static UP: Strokes = &[
    &[[4, 0], [4, 14]],
    &[[4, 14], [12, 14], [16, 12], [16, 8], [12, 6], [4, 6]],
];
static UQ: Strokes = &[
    &[
        [7, 14],
        [14, 14],
        [18, 10],
        [18, 4],
        [14, 0],
        [7, 0],
        [4, 4],
        [4, 10],
        [7, 14],
    ],
    &[[13, 4], [18, -2]],
];
static UR: Strokes = &[
    &[[4, 0], [4, 14]],
    &[[4, 14], [12, 14], [16, 12], [16, 8], [12, 6], [4, 6]],
    &[[10, 6], [16, 0]],
];
static US: Strokes = &[
    &[
        [16, 12],
        [12, 14],
        [7, 14],
        [4, 12],
        [6, 8],
        [12, 7],
        [15, 6],
        [16, 3],
        [13, 0],
        [7, 0],
        [4, 2],
    ],
];
static UT: Strokes = &[&[[4, 14], [18, 14]], &[[11, 14], [11, 0]]];
static UU: Strokes = &[&[[4, 14], [4, 4], [8, 0], [14, 0], [18, 4], [18, 14]]];
static UV: Strokes = &[&[[4, 14], [11, 0], [18, 14]]];
static UW: Strokes = &[&[[4, 14], [7, 0], [11, 9], [15, 0], [18, 14]]];
static UX: Strokes = &[&[[4, 14], [18, 0]], &[[18, 14], [4, 0]]];
static UY: Strokes = &[&[[4, 14], [11, 7]], &[[18, 14], [11, 7], [11, 0]]];
static UZ: Strokes = &[&[[4, 14], [18, 14], [4, 0], [18, 0]]];

// Digits, cap band 0..14.
static D0: Strokes = &[
    &[
        [7, 14],
        [14, 14],
        [18, 10],
        [18, 4],
        [14, 0],
        [7, 0],
        [4, 4],
        [4, 10],
        [7, 14],
    ],
];
static D1: Strokes = &[&[[6, 10], [11, 14], [11, 0]]];
static D2: Strokes = &[&[[4, 12], [7, 14], [13, 14], [16, 11], [16, 9], [4, 0], [16, 0]]];
static D3: Strokes = &[
    &[
        [4, 14],
        [14, 14],
        [8, 8],
        [14, 8],
        [16, 6],
        [16, 2],
        [13, 0],
        [7, 0],
        [4, 2],
    ],
];
static D4: Strokes = &[&[[14, 0], [14, 14], [4, 5], [18, 5]]];
static D5: Strokes = &[
    &[
        [16, 14],
        [6, 14],
        [5, 8],
        [11, 8],
        [16, 6],
        [16, 2],
        [13, 0],
        [7, 0],
        [4, 2],
    ],
];
static D6: Strokes = &[
    &[
        [15, 13],
        [12, 14],
        [8, 14],
        [4, 10],
        [4, 2],
        [7, 0],
        [13, 0],
        [16, 2],
        [16, 6],
        [13, 8],
        [4, 8],
    ],
];
static D7: Strokes = &[&[[4, 14], [16, 14], [8, 0]]];
static D8: Strokes = &[
    &[
        [10, 14],
        [6, 14],
        [4, 12],
        [4, 10],
        [7, 8],
        [13, 8],
        [16, 10],
        [16, 12],
        [14, 14],
        [10, 14],
    ],
    &[[7, 8], [4, 6], [4, 2], [7, 0], [13, 0], [16, 2], [16, 6], [13, 8]],
];
static D9: Strokes = &[
    &[
        [5, 1],
        [8, 0],
        [12, 0],
        [16, 4],
        [16, 12],
        [13, 14],
        [7, 14],
        [4, 12],
        [4, 8],
        [7, 6],
        [16, 6],
    ],
];

// Punctuation.
static SP: Strokes = &[];
static PDOT: Strokes = &[&[[10, 1], [10, 1]]];
static PCOMMA: Strokes = &[&[[10, 1], [8, -3]]];
static PBANG: Strokes = &[&[[10, 14], [10, 5]], &[[10, 1], [10, 1]]];
static PQUEST: Strokes = &[
    &[[4, 12], [7, 14], [13, 14], [16, 12], [16, 10], [10, 7], [10, 5]],
    &[[10, 1], [10, 1]],
];
static PCOLON: Strokes = &[&[[10, 7], [10, 7]], &[[10, 1], [10, 1]]];
static PSEMI: Strokes = &[&[[10, 7], [10, 7]], &[[10, 1], [8, -3]]];
static PDASH: Strokes = &[&[[6, 5], [14, 5]]];
static PAPOS: Strokes = &[&[[10, 14], [9, 10]]];
static PQUEST_INV: Strokes = &[
    &[[16, 2], [13, 0], [7, 0], [4, 2], [4, 4], [10, 7], [10, 9]],
    &[[10, 13], [10, 13]],
];
static PBANG_INV: Strokes = &[&[[10, 0], [10, 9]], &[[10, 13], [10, 13]]];

// Cyrillic lowercase shapes of their own; the shared shapes are aliases below.
static CB: Strokes = &[
    &[[3, 4], [3, 8], [5, 10], [13, 10]],
    &[
        [3, 4],
        [5, 6],
        [11, 6],
        [14, 4],
        [14, 2],
        [11, 0],
        [5, 0],
        [3, 2],
        [3, 4],
    ],
];
static CG: Strokes = &[&[[4, 0], [4, 10], [15, 10]]];
static CD: Strokes = &[
    &[
        [6, 10],
        [12, 10],
        [16, 6],
        [16, 4],
        [12, 0],
        [6, 0],
        [4, 4],
        [4, 6],
        [6, 10],
    ],
    &[[16, 11], [16, 0]],
];
static CYO: Strokes = &[
    &[[6, 13], [6, 13]],
    &[[12, 13], [12, 13]],
    &[
        [16, 6],
        [4, 6],
        [4, 8],
        [8, 10],
        [12, 10],
        [16, 8],
        [14, 2],
        [10, 0],
        [6, 0],
        [4, 2],
    ],
];
static CZH: Strokes = &[&[[10, 0], [10, 10]], &[[4, 10], [16, 0]], &[[16, 10], [4, 0]]];
static CZE: Strokes = &[
    &[
        [4, 9],
        [8, 10],
        [13, 10],
        [15, 8],
        [15, 7],
        [10, 6],
        [15, 5],
        [15, 2],
        [13, 0],
        [8, 0],
        [4, 1],
    ],
];
static CI: Strokes = &[&[[4, 10], [4, 0]], &[[4, 0], [16, 10]], &[[16, 10], [16, 0]]];
static CJI: Strokes = &[
    &[[7, 14], [10, 12], [13, 14]],
    &[[4, 10], [4, 0]],
    &[[4, 0], [16, 10]],
    &[[16, 10], [16, 0]],
];
static CL: Strokes = &[&[[4, 0], [6, 10], [16, 10], [16, 0]]];
static CM: Strokes = &[&[[4, 0], [4, 10], [16, 10], [16, 0]], &[[10, 10], [10, 5]]];
static CP: Strokes = &[&[[4, 0], [4, 10], [16, 10], [16, 0]]];
static CF: Strokes = &[
    &[[10, 14], [10, -4]],
    &[
        [6, 8],
        [14, 8],
        [16, 6],
        [16, 2],
        [14, 0],
        [6, 0],
        [4, 2],
        [4, 6],
        [6, 8],
    ],
];
static CC: Strokes = &[&[[4, 10], [4, 0], [16, 0], [16, 10]], &[[16, 0], [18, -4]]];
static CCH: Strokes = &[&[[4, 10], [4, 4], [9, 2], [16, 2]], &[[16, 0], [16, 10]]];
static CSH: Strokes = &[&[[4, 10], [4, 0], [16, 0], [16, 10]], &[[10, 10], [10, 0]]];
static CSCH: Strokes = &[
    &[[4, 10], [4, 0], [16, 0], [16, 10]],
    &[[10, 10], [10, 0]],
    &[[16, 0], [18, -4]],
];
static CHARD: Strokes = &[
    &[[4, 10], [8, 10]],
    &[[8, 10], [8, 0]],
    &[[8, 4], [14, 4], [16, 3], [15, 1], [11, 0], [8, 0]],
];
static CYERY: Strokes = &[
    &[[4, 10], [4, 0]],
    &[[4, 4], [10, 4], [12, 3], [11, 1], [7, 0], [4, 0]],
    &[[17, 0], [17, 10]],
];
static CSOFT: Strokes = &[
    &[[4, 10], [4, 0]],
    &[[4, 4], [10, 4], [12, 3], [11, 1], [7, 0], [4, 0]],
];
static CEREV: Strokes = &[
    &[[4, 8], [8, 10], [13, 10], [16, 7], [16, 3], [13, 0], [8, 0], [4, 2]],
    &[[9, 5], [16, 5]],
];
static CYU: Strokes = &[
    &[[4, 0], [4, 10]],
    &[[4, 5], [9, 5]],
    &[
        [10, 7],
        [13, 10],
        [17, 8],
        [18, 5],
        [17, 2],
        [13, 0],
        [10, 2],
        [9, 5],
        [10, 7],
    ],
];
static CYA: Strokes = &[
    &[[16, 0], [16, 10]],
    &[[16, 10], [8, 10], [4, 8], [4, 6], [8, 5], [16, 5]],
    &[[11, 5], [4, 0]],
];
static CI_DOTLESS: Strokes = &[&[[10, 10], [10, 0]]];
static CYI_DIAERESIS: Strokes = &[&[[7, 13], [7, 13]], &[[13, 13], [13, 13]], &[[10, 10], [10, 0]]];
static CE_UKR: Strokes = &[
    &[
        [4, 6],
        [16, 6],
        [16, 8],
        [12, 10],
        [8, 10],
        [4, 8],
        [6, 2],
        [10, 0],
        [14, 0],
        [16, 2],
    ],
];
static CG_UP: Strokes = &[&[[4, 0], [4, 10], [15, 10]], &[[11, 10], [11, 14]]];

// Cyrillic capitals of their own.
static CBE: Strokes = &[
    &[[4, 0], [4, 14]],
    &[[4, 14], [15, 14]],
    &[[4, 7], [12, 7], [16, 5], [16, 2], [12, 0], [4, 0]],
];
static CGE: Strokes = &[&[[4, 0], [4, 14], [16, 14]]];
static CDE: Strokes = &[&[[2, 0], [4, 0], [8, 14], [13, 14], [17, 0], [19, 0]]];
static CYO_CAP: Strokes = &[
    &[[7, 17], [7, 17]],
    &[[13, 17], [13, 17]],
    &[[16, 14], [4, 14], [4, 0], [16, 0]],
    &[[4, 7], [14, 7]],
];
static CIB: Strokes = &[&[[4, 14], [4, 0]], &[[4, 0], [18, 14]], &[[18, 14], [18, 0]]];
static CLB: Strokes = &[&[[3, 0], [7, 14], [17, 14], [17, 0]]];
static CPB: Strokes = &[&[[4, 0], [4, 14], [17, 14], [17, 0]]];
static CUB: Strokes = &[&[[4, 14], [11, 6]], &[[18, 14], [11, 6], [8, 0]]];
static CCB: Strokes = &[&[[4, 14], [4, 0], [18, 0], [18, 14]], &[[18, 0], [20, -4]]];
static CSHB: Strokes = &[
    &[[4, 0], [4, 14]],
    &[[11, 0], [11, 14]],
    &[[18, 0], [18, 14]],
    &[[4, 14], [18, 14]],
];
static CSCHB: Strokes = &[
    &[[4, 0], [4, 14]],
    &[[11, 0], [11, 14]],
    &[[18, 0], [18, 14]],
    &[[4, 14], [18, 14]],
    &[[18, 0], [20, -4]],
];
static CYAB: Strokes = &[
    &[[16, 0], [16, 14]],
    &[[16, 14], [8, 14], [4, 12], [4, 9], [8, 7], [16, 7]],
    &[[10, 7], [4, 0]],
];
static CYI_CAP: Strokes = &[&[[7, 17], [7, 17]], &[[13, 17], [13, 17]], &[[10, 0], [10, 14]]];

/// Every glyph with its own skeleton, in a fixed order.
static TABLE: &[(char, GlyphDef)] = &[
    ('a', def(18.0, LA)),
    ('b', def(18.0, LB)),
    ('c', def(18.0, LC)),
    ('d', def(18.0, LD)),
    ('e', def(18.0, LE)),
    ('f', def(14.0, LF)),
    ('g', def(18.0, LG)),
    ('h', def(18.0, LH)),
    ('i', def(10.0, LI)),
    ('j', def(10.0, LJ)),
    ('k', def(16.0, LK)),
    ('l', def(10.0, LL)),
    ('m', def(22.0, LM)),
    ('n', def(18.0, LN)),
    ('o', def(18.0, LO)),
    ('p', def(18.0, LP)),
    ('q', def(18.0, LQ)),
    ('r', def(14.0, LR)),
    ('s', def(16.0, LS)),
    ('t', def(14.0, LT)),
    ('u', def(18.0, LU)),
    ('v', def(18.0, LV)),
    ('w', def(24.0, LW)),
    ('x', def(16.0, LX)),
    ('y', def(18.0, LY)),
    ('z', def(16.0, LZ)),
    ('ö', def(18.0, L_O_UMLAUT)),
    ('ü', def(18.0, L_U_UMLAUT)),
    ('ß', def(18.0, L_SHARP_S)),
    ('œ', def(22.0, L_OE)),
    ('è', def(18.0, L_E_GRAVE)),
    ('é', def(18.0, L_E_ACUTE)),
    ('ù', def(18.0, L_U_GRAVE)),
    ('ñ', def(18.0, L_N_TILDE)),
    ('ã', def(18.0, L_A_TILDE)),
    ('õ', def(18.0, L_O_TILDE)),
    ('ł', def(10.0, L_L_STROKE)),
    ('ę', def(18.0, L_E_OGONEK)),
    ('ż', def(16.0, L_Z_DOT)),
    ('ś', def(16.0, L_S_ACUTE)),
    ('ć', def(18.0, L_C_ACUTE)),
    ('á', def(18.0, L_A_ACUTE)),
    ('č', def(18.0, L_C_CARON)),
    ('ě', def(18.0, L_E_CARON)),
    ('í', def(10.0, L_I_ACUTE)),
    ('ř', def(14.0, L_R_CARON)),
    ('ž', def(16.0, L_Z_CARON)),
    ('ó', def(18.0, L_O_ACUTE)),
    ('ç', def(18.0, L_C_CEDILLA)),
    ('Ł', def(18.0, U_L_STROKE)),
    ('Ś', def(20.0, U_S_ACUTE)),
    ('Ż', def(20.0, U_Z_DOT)),
    ('Č', def(20.0, U_C_CARON)),
    ('A', def(20.0, UA)),
    ('B', def(20.0, UB)),
    ('C', def(20.0, UC)),
    ('D', def(20.0, UD)),
    ('E', def(18.0, UE)),
    ('F', def(18.0, UF)),
    ('G', def(20.0, UG)),
    ('H', def(20.0, UH)),
    ('I', def(12.0, UI)),
    ('J', def(18.0, UJ)),
    ('K', def(20.0, UK)),
    ('L', def(18.0, UL)),
    ('M', def(24.0, UM)),
    ('N', def(22.0, UN)),
    ('O', def(22.0, UO)),
    ('P', def(20.0, UP)),
    ('Q', def(22.0, UQ)),
    ('R', def(20.0, UR)),
    ('S', def(20.0, US)),
    ('T', def(20.0, UT)),
    ('U', def(22.0, UU)),
    ('V', def(20.0, UV)),
    ('W', def(26.0, UW)),
    ('X', def(20.0, UX)),
    ('Y', def(20.0, UY)),
    ('Z', def(20.0, UZ)),
    ('0', def(20.0, D0)),
    ('1', def(16.0, D1)),
    ('2', def(20.0, D2)),
    ('3', def(20.0, D3)),
    ('4', def(20.0, D4)),
    ('5', def(20.0, D5)),
    ('6', def(20.0, D6)),
    ('7', def(20.0, D7)),
    ('8', def(20.0, D8)),
    ('9', def(20.0, D9)),
    (' ', def(10.0, SP)),
    ('.', def(10.0, PDOT)),
    (',', def(10.0, PCOMMA)),
    ('!', def(10.0, PBANG)),
    ('?', def(18.0, PQUEST)),
    (':', def(10.0, PCOLON)),
    (';', def(10.0, PSEMI)),
    ('-', def(14.0, PDASH)),
    ('\'', def(8.0, PAPOS)),
    ('¿', def(18.0, PQUEST_INV)),
    ('¡', def(10.0, PBANG_INV)),
    ('б', def(18.0, CB)),
    ('г', def(16.0, CG)),
    ('д', def(18.0, CD)),
    ('ё', def(18.0, CYO)),
    ('ж', def(22.0, CZH)),
    ('з', def(16.0, CZE)),
    ('и', def(18.0, CI)),
    ('й', def(18.0, CJI)),
    ('л', def(18.0, CL)),
    ('м', def(20.0, CM)),
    ('п', def(18.0, CP)),
    ('ф', def(20.0, CF)),
    ('ц', def(20.0, CC)),
    ('ч', def(18.0, CCH)),
    ('ш', def(22.0, CSH)),
    ('щ', def(22.0, CSCH)),
    ('ъ', def(18.0, CHARD)),
    ('ы', def(24.0, CYERY)),
    ('ь', def(16.0, CSOFT)),
    ('э', def(18.0, CEREV)),
    ('ю', def(24.0, CYU)),
    ('я', def(18.0, CYA)),
    ('і', def(10.0, CI_DOTLESS)),
    ('ї', def(10.0, CYI_DIAERESIS)),
    ('є', def(18.0, CE_UKR)),
    ('ґ', def(16.0, CG_UP)),
    ('Б', def(20.0, CBE)),
    ('Г', def(18.0, CGE)),
    ('Д', def(22.0, CDE)),
    ('Ё', def(18.0, CYO_CAP)),
    ('И', def(22.0, CIB)),
    ('Л', def(20.0, CLB)),
    ('П', def(21.0, CPB)),
    ('У', def(20.0, CUB)),
    ('Ц', def(22.0, CCB)),
    ('Ш', def(24.0, CSHB)),
    ('Щ', def(24.0, CSCHB)),
    ('Я', def(20.0, CYAB)),
    ('Ї', def(12.0, CYI_CAP)),
];

/// Letters that are drawn from another letter's skeleton, with the scale that takes one band to
/// the other. Shapes shared across the supported alphabets are stored once.
static ALIASES: &[(char, char, f32)] = &[
    ('а', 'a', 1.0),
    ('е', 'e', 1.0),
    ('о', 'o', 1.0),
    ('р', 'p', 1.0),
    ('с', 'c', 1.0),
    ('у', 'y', 1.0),
    ('х', 'x', 1.0),
    ('к', 'k', 1.0),
    ('в', 'B', CAP_TO_X),
    ('т', 'T', CAP_TO_X),
    ('н', 'H', CAP_TO_X),
    ('А', 'A', 1.0),
    ('В', 'B', 1.0),
    ('Е', 'E', 1.0),
    ('К', 'K', 1.0),
    ('М', 'M', 1.0),
    ('Н', 'H', 1.0),
    ('О', 'O', 1.0),
    ('Р', 'P', 1.0),
    ('С', 'C', 1.0),
    ('Т', 'T', 1.0),
    ('Х', 'X', 1.0),
    ('І', 'I', 1.0),
    ('Ж', 'ж', X_TO_CAP),
    ('З', 'з', X_TO_CAP),
    ('Й', 'й', X_TO_CAP),
    ('Ф', 'ф', X_TO_CAP),
    ('Ч', 'ч', X_TO_CAP),
    ('Ъ', 'ъ', X_TO_CAP),
    ('Ы', 'ы', X_TO_CAP),
    ('Ь', 'ь', X_TO_CAP),
    ('Э', 'э', X_TO_CAP),
    ('Ю', 'ю', X_TO_CAP),
    ('Є', 'є', X_TO_CAP),
    ('Ґ', 'ґ', X_TO_CAP),
];

/// A glyph with its alias scale resolved: `scale` maps the skeleton's own band onto the band the
/// character is read at, and applies to strokes and advance alike.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedGlyph {
    pub def: &'static GlyphDef,
    pub scale: f32,
}

impl ResolvedGlyph {
    /// How far the pen moves after this glyph, in design units.
    pub fn advance(&self) -> f32 {
        self.def.advance * self.scale
    }
}

fn own(character: char) -> Option<&'static GlyphDef> {
    TABLE.iter().find(|(known, _)| *known == character).map(|(_, def)| def)
}

/// The glyph a character is drawn from, or nothing when the corpus font does not cover it.
pub fn glyph(character: char) -> Option<ResolvedGlyph> {
    if let Some((_, base, scale)) = ALIASES.iter().find(|(from, _, _)| *from == character) {
        return own(*base).map(|def| ResolvedGlyph { def, scale: *scale });
    }
    own(character).map(|def| ResolvedGlyph { def, scale: 1.0 })
}

/// The first character the corpus font cannot draw, so a phrase pack can be checked before any
/// pixels are rendered.
pub fn covers(text: &str) -> Option<char> {
    text.chars().find(|character| glyph(*character).is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_character_resolves_to_itself() {
        for (character, def) in TABLE {
            let resolved = glyph(*character).expect("a table entry always resolves");
            assert_eq!(resolved.scale, 1.0, "{character}");
            assert_eq!(resolved.def.advance, def.advance, "{character}");
        }
    }

    #[test]
    fn every_alias_points_at_a_glyph_that_exists() {
        for (from, base, scale) in ALIASES {
            let resolved = glyph(*from).unwrap_or_else(|| panic!("{from} did not resolve"));
            assert!(own(*base).is_some(), "{from} aliases {base}, which has no skeleton");
            assert_eq!(resolved.scale, *scale, "{from}");
            assert_eq!(resolved.def.advance, own(*base).unwrap().advance, "{from}");
        }
    }

    #[test]
    fn shared_shapes_really_are_shared() {
        assert_eq!(glyph('а').unwrap().advance(), glyph('a').unwrap().advance());
        // Cyrillic в is Latin B worn at the x-height.
        let scaled = glyph('в').unwrap().advance();
        let original = glyph('B').unwrap().advance();
        assert!((scaled - original * CAP_TO_X).abs() < 1e-4);
        assert!(scaled < original);
    }

    #[test]
    fn advances_are_positive_and_proportionate() {
        for (_, def) in TABLE {
            assert!(def.advance > 0.0);
        }
        assert!(glyph('i').unwrap().advance() < glyph('m').unwrap().advance());
        assert!(glyph('w').unwrap().advance() > glyph('a').unwrap().advance());
    }

    #[test]
    fn space_has_an_advance_and_no_ink() {
        let space = glyph(' ').expect("space is part of the font");
        assert!(space.def.strokes.is_empty());
        assert!(space.advance() > 0.0);
    }

    #[test]
    fn skeleton_points_stay_inside_the_design_envelope() {
        for (character, def) in TABLE {
            for stroke in def.strokes {
                for [x, y] in *stroke {
                    assert!((-3..=22).contains(&x), "{character}: x={x} outside the grid");
                    assert!((-6..=19).contains(&y), "{character}: y={y} outside the grid");
                }
            }
        }
    }

    #[test]
    fn uncovered_scripts_report_no_glyph() {
        assert!(glyph('漢').is_none());
        assert!(glyph('あ').is_none());
        assert!(glyph('α').is_none());
        assert!(glyph('ع').is_none());
        assert!(covers("hello world").is_none());
        assert_eq!(covers("hello 世界"), Some('世'));
    }

    #[test]
    fn styles_carry_their_measured_weight() {
        assert!(FontStyle::Bold.stroke_radius() > FontStyle::Regular.stroke_radius());
        assert_eq!(FontStyle::Italic.stroke_radius(), FontStyle::Regular.stroke_radius());
        assert!(FontStyle::Italic.is_slanted());
        assert!(!FontStyle::Regular.is_slanted());
        assert_eq!(FontStyle::all().len(), 3);
    }
}
