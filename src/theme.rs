//! Semantic color palette.
//!
//! Every UI color flows through a [`Palette`], which maps *semantic roles*
//! (headings, math operands, borders, …) to concrete [`Color`]s. This decouples
//! "what a color means" from "what RGB it is", so a theme is just a different
//! table of role → color.
//!
//! Roles are intentionally finer-grained than the set of distinct RGB values:
//! several roles share one value today (e.g. `h2`, `border_focused`, `math_lhs`
//! and `config_key` are all cyan in Tokyo Night), but keeping them separate lets
//! a future theme differentiate them.
//!
//! The only constructor for now is [`tokyo_night_night`], which reproduces the
//! previously-hardcoded values byte-for-byte. Additional built-in themes and a
//! live picker are layered on top of this in follow-up work.

use ratatui::style::Color;

/// A full set of semantic color roles for the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    // Surfaces / structure
    pub bg: Color,
    pub surface: Color,
    pub panel_sel_bg: Color,
    pub border_focused: Color,
    pub border_dim: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,

    // Text
    pub fg: Color,
    pub fg_muted: Color,

    // Markdown headings
    pub h1: Color,
    pub h2: Color,
    pub h3: Color,
    pub h4: Color,
    pub h5: Color,
    pub h6: Color,

    // Markdown inline / blocks
    pub link: Color,
    pub blockquote: Color,
    pub hr: Color,
    pub comment: Color,
    pub list_marker: Color,

    // Math
    pub math_lhs: Color,
    pub math_operator: Color,
    pub math_number: Color,
    pub math_result: Color,
    pub math_unit: Color,
    pub math_fn: Color,

    // Help / config text accents
    pub config_key: Color,
    pub keybind_label: Color,
    pub help_section: Color,

    // Cursor color per edit mode
    pub cursor_normal: Color,
    pub cursor_insert: Color,
    pub cursor_visual: Color,
    pub cursor_search: Color,

    // Editor surface (drives the edtui EditorTheme)
    pub editor_bg: Color,
    pub editor_fg: Color,
    pub line_number: Color,

    // Status / semantic
    pub error: Color,
}

/// Tokyo Night (Night) — the historical default. Values reproduce the colors
/// that were previously hardcoded across the UI, so routing through this palette
/// is pixel-identical.
pub fn tokyo_night_night() -> Palette {
    Palette {
        // Surfaces / structure
        bg: Color::Rgb(26, 27, 38),
        surface: Color::Rgb(22, 22, 30),
        panel_sel_bg: Color::Rgb(59, 66, 97),
        border_focused: Color::Rgb(125, 207, 255),
        border_dim: Color::Rgb(86, 95, 137),
        selection_bg: Color::Rgb(167, 82, 142),
        selection_fg: Color::Rgb(224, 230, 242),

        // Text
        fg: Color::Rgb(169, 177, 214),
        fg_muted: Color::Rgb(86, 95, 137),

        // Markdown headings
        h1: Color::Rgb(187, 154, 247),
        h2: Color::Rgb(125, 207, 255),
        h3: Color::Rgb(122, 162, 247),
        h4: Color::Rgb(115, 218, 202),
        h5: Color::Rgb(158, 206, 106),
        h6: Color::Rgb(255, 158, 100),

        // Markdown inline / blocks
        link: Color::Rgb(187, 154, 247),
        blockquote: Color::Rgb(158, 206, 106),
        hr: Color::Rgb(86, 95, 137),
        comment: Color::Rgb(86, 95, 137),
        list_marker: Color::Rgb(255, 158, 100),

        // Math
        math_lhs: Color::Rgb(125, 207, 255),
        math_operator: Color::Rgb(255, 158, 100),
        math_number: Color::Rgb(115, 218, 202),
        math_result: Color::Rgb(115, 218, 202),
        math_unit: Color::Rgb(244, 143, 177),
        math_fn: Color::Rgb(122, 162, 247),

        // Help / config text accents
        config_key: Color::Rgb(125, 207, 255),
        keybind_label: Color::Rgb(158, 206, 106),
        help_section: Color::Rgb(255, 158, 100),

        // Cursor color per edit mode
        cursor_normal: Color::Rgb(122, 162, 247),
        cursor_insert: Color::Rgb(158, 206, 106),
        cursor_visual: Color::Rgb(187, 154, 247),
        cursor_search: Color::Rgb(255, 158, 100),

        // Editor surface (edtui defaults were BLACK / WHITE / GRAY)
        editor_bg: Color::Rgb(0, 0, 0),
        editor_fg: Color::Rgb(255, 255, 255),
        line_number: Color::Rgb(100, 100, 100),

        // Status / semantic
        error: Color::Rgb(247, 118, 142),
    }
}
