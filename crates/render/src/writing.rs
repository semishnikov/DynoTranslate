//! Writing direction and writing mode.
//!
//! Direction decides the base paragraph direction for bidirectional text; mode decides whether
//! lines run across the box or down it. Both are settled from evidence the pipeline already
//! has — the script of the text and the shape of the block — rather than from configuration.

use serde::{Deserialize, Serialize};

/// Base direction of a paragraph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    LeftToRight,
    RightToLeft,
}

/// How lines are arranged inside the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WritingMode {
    /// Lines stack top to bottom, reading across.
    #[default]
    Horizontal,
    /// Characters stack top to bottom in a column; columns run right to left, which is how
    /// vertical Japanese and Chinese interface text is set.
    Vertical,
}

/// Direction implied by the characters themselves. Arabic and Hebrew reverse the base
/// direction; everything else defaults to left to right.
pub fn dominant_direction(text: &str) -> Direction {
    for ch in text.chars() {
        match ch {
            '\u{0590}'..='\u{08FF}' => return Direction::RightToLeft,
            'A'..='z' | '\u{0400}'..='\u{04FF}' => return Direction::LeftToRight,
            _ => {}
        }
    }
    Direction::LeftToRight
}

/// Whether a block should be set vertically.
///
/// Vertical interface text is rare and always shaped the same way on screen: a tall narrow
/// box, several short units stacked inside it, and a script that is actually written that
/// way. Geometry alone would misread a narrow button list, so the script has to agree.
pub fn writing_mode_of(text: &str, width: u32, height: u32) -> WritingMode {
    if width == 0 || height < width.saturating_mul(3) / 2 {
        return WritingMode::Horizontal;
    }
    if has_vertical_script(text) {
        WritingMode::Vertical
    } else {
        WritingMode::Horizontal
    }
}

fn has_vertical_script(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(ch as u32,
            0x3040..=0x30FF   // hiragana, katakana
            | 0x3400..=0x4DBF // CJK extension A
            | 0x4E00..=0x9FFF // CJK unified
            | 0xF900..=0xFAFF // CJK compatibility
            | 0xFF66..=0xFF9D // halfwidth katakana
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_and_cyrillic_are_left_to_right() {
        assert_eq!(dominant_direction("Hello"), Direction::LeftToRight);
        assert_eq!(dominant_direction("Привет"), Direction::LeftToRight);
    }

    #[test]
    fn hebrew_and_arabic_are_right_to_left() {
        assert_eq!(dominant_direction("שלום"), Direction::RightToLeft);
        assert_eq!(dominant_direction("الإعدادات"), Direction::RightToLeft);
    }

    #[test]
    fn digits_alone_do_not_flip_the_direction() {
        assert_eq!(dominant_direction("42"), Direction::LeftToRight);
    }

    #[test]
    fn a_short_box_is_horizontal_even_with_japanese_text() {
        assert_eq!(
            writing_mode_of("メニュー", 200, 40),
            WritingMode::Horizontal
        );
    }

    #[test]
    fn a_tall_box_of_japanese_text_is_vertical() {
        assert_eq!(
            writing_mode_of("メニューを開く", 40, 200),
            WritingMode::Vertical
        );
    }

    #[test]
    fn a_tall_box_of_latin_text_stays_horizontal() {
        assert_eq!(
            writing_mode_of("Settings\npanel\nnotes", 40, 200),
            WritingMode::Horizontal
        );
    }
}
