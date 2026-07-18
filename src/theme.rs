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
//! [`tokyo_night_night`] reproduces the previously-hardcoded values byte-for-byte
//! and serves as the base/fallback. Other themes are TOML files in
//! `~/.config/calki/themes/` (built-ins are embedded and seeded there on first
//! run); [`load_palette`] resolves a theme name into a [`Palette`], filling any
//! omitted role from the base.

use ratatui::style::Color;
use serde::Deserialize;
use std::collections::BTreeMap;

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

/// Built-in themes embedded in the binary: `(name, TOML source)`. Seeded to the
/// user's themes dir on first run and available even if that dir can't be read.
const BUILTIN_THEMES: &[(&str, &str)] = &[
    ("tokyo-night", include_str!("themes/tokyo-night.toml")),
    (
        "tokyo-night-storm",
        include_str!("themes/tokyo-night-storm.toml"),
    ),
    ("dracula", include_str!("themes/dracula.toml")),
    (
        "catppuccin-mocha",
        include_str!("themes/catppuccin-mocha.toml"),
    ),
    ("kemika-purple", include_str!("themes/kemika-purple.toml")),
];

/// Parse `"#rrggbb"` (or bare `"rrggbb"`) into a [`Color`]. `None` if malformed.
fn parse_hex(s: &str) -> Option<Color> {
    let h = s.trim();
    let h = h.strip_prefix('#').unwrap_or(h);
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

/// A deserialized theme file. Every role is optional and the category tables are
/// organizational only (a role name is unique across all of them), so a theme
/// need only list the roles it changes.
#[derive(Deserialize, Default)]
#[serde(default)]
struct ThemeFile {
    #[allow(dead_code)] // present for readability in files; not consumed
    name: Option<String>,
    surfaces: BTreeMap<String, String>,
    text: BTreeMap<String, String>,
    headings: BTreeMap<String, String>,
    markdown: BTreeMap<String, String>,
    math: BTreeMap<String, String>,
    help: BTreeMap<String, String>,
    cursor: BTreeMap<String, String>,
    editor: BTreeMap<String, String>,
    status: BTreeMap<String, String>,
}

impl ThemeFile {
    /// Apply this file's roles over `base`, returning the resulting palette plus
    /// any role keys that were unrecognized or had an unparseable color (so the
    /// caller can warn the user). Omitted / bad roles keep the `base` value.
    fn into_palette(self, base: Palette) -> (Palette, Vec<String>) {
        let mut p = base;
        let mut problems = Vec::new();
        let sections = [
            self.surfaces,
            self.text,
            self.headings,
            self.markdown,
            self.math,
            self.help,
            self.cursor,
            self.editor,
            self.status,
        ];
        for section in sections {
            for (role, hex) in section {
                match parse_hex(&hex) {
                    Some(color) if set_role(&mut p, &role, color) => {}
                    _ => problems.push(role),
                }
            }
        }
        (p, problems)
    }
}

/// Assign one palette role by name. Returns `false` for an unknown role.
fn set_role(p: &mut Palette, role: &str, c: Color) -> bool {
    match role {
        "bg" => p.bg = c,
        "surface" => p.surface = c,
        "panel_sel_bg" => p.panel_sel_bg = c,
        "border_focused" => p.border_focused = c,
        "border_dim" => p.border_dim = c,
        "selection_bg" => p.selection_bg = c,
        "selection_fg" => p.selection_fg = c,
        "fg" => p.fg = c,
        "fg_muted" => p.fg_muted = c,
        "h1" => p.h1 = c,
        "h2" => p.h2 = c,
        "h3" => p.h3 = c,
        "h4" => p.h4 = c,
        "h5" => p.h5 = c,
        "h6" => p.h6 = c,
        "link" => p.link = c,
        "blockquote" => p.blockquote = c,
        "hr" => p.hr = c,
        "comment" => p.comment = c,
        "list_marker" => p.list_marker = c,
        "math_lhs" => p.math_lhs = c,
        "math_operator" => p.math_operator = c,
        "math_number" => p.math_number = c,
        "math_result" => p.math_result = c,
        "math_unit" => p.math_unit = c,
        "math_fn" => p.math_fn = c,
        "config_key" => p.config_key = c,
        "keybind_label" => p.keybind_label = c,
        "help_section" => p.help_section = c,
        "cursor_normal" => p.cursor_normal = c,
        "cursor_insert" => p.cursor_insert = c,
        "cursor_visual" => p.cursor_visual = c,
        "cursor_search" => p.cursor_search = c,
        "editor_bg" => p.editor_bg = c,
        "editor_fg" => p.editor_fg = c,
        "line_number" => p.line_number = c,
        "error" => p.error = c,
        _ => return false,
    }
    true
}

/// The user's themes directory (`~/.config/calki/themes`).
#[cfg(not(test))]
fn themes_dir() -> Option<std::path::PathBuf> {
    let mut p = crate::currency::get_config_path()?;
    p.push("themes");
    Some(p)
}

/// Write the embedded built-in themes to the themes dir, creating it if needed.
/// Existing files are never overwritten, so user edits survive upgrades.
pub fn seed_builtin_themes() {
    #[cfg(not(test))]
    {
        let Some(dir) = themes_dir() else {
            return;
        };
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        for (name, src) in BUILTIN_THEMES {
            let path = dir.join(format!("{name}.toml"));
            if !path.exists() {
                let _ = std::fs::write(&path, src);
            }
        }
    }
}

/// Every available theme name: embedded built-ins ∪ `.toml` files on disk, sorted.
pub fn list_themes() -> Vec<String> {
    let mut names: std::collections::BTreeSet<String> =
        BUILTIN_THEMES.iter().map(|(n, _)| n.to_string()).collect();
    #[cfg(not(test))]
    {
        if let Some(dir) = themes_dir()
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            for e in entries.flatten() {
                let path = e.path();
                if path.extension().and_then(|s| s.to_str()) == Some("toml")
                    && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                {
                    names.insert(stem.to_string());
                }
            }
        }
    }
    names.into_iter().collect()
}

/// Read a theme's TOML source: a file on disk wins over the embedded built-in of
/// the same name (so a user can override a built-in), else the embedded source.
fn read_theme_source(name: &str) -> Option<String> {
    #[cfg(not(test))]
    {
        if let Some(dir) = themes_dir() {
            let path = dir.join(format!("{name}.toml"));
            if let Ok(s) = std::fs::read_to_string(&path) {
                return Some(s);
            }
        }
    }
    BUILTIN_THEMES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| s.to_string())
}

/// Resolve a theme name into a [`Palette`] (omitted roles inherit Tokyo Night),
/// plus any unrecognized/bad role keys. `Err` if the theme is missing or its TOML
/// fails to parse.
pub fn load_palette(name: &str) -> Result<(Palette, Vec<String>), String> {
    let src = read_theme_source(name).ok_or_else(|| format!("theme '{name}' not found"))?;
    let tf: ThemeFile = toml::from_str(&src).map_err(|e| format!("theme '{name}': {e}"))?;
    Ok(tf.into_palette(tokyo_night_night()))
}

/// Convenience for startup: load a theme or silently fall back to Tokyo Night.
pub fn load_palette_or_default(name: &str) -> Palette {
    load_palette(name)
        .map(|(p, _)| p)
        .unwrap_or_else(|_| tokyo_night_night())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_variants() {
        assert_eq!(parse_hex("#7dcfff"), Some(Color::Rgb(125, 207, 255)));
        assert_eq!(parse_hex("7dcfff"), Some(Color::Rgb(125, 207, 255)));
        assert_eq!(parse_hex("#000000"), Some(Color::Rgb(0, 0, 0)));
        assert_eq!(parse_hex("#fff"), None); // wrong length
        assert_eq!(parse_hex("#gggggg"), None); // non-hex
    }

    #[test]
    fn tokyo_night_toml_roundtrips_to_base() {
        // The embedded tokyo-night.toml must reproduce the hardcoded base exactly.
        let (p, problems) = load_palette("tokyo-night").unwrap();
        assert!(
            problems.is_empty(),
            "unexpected role problems: {problems:?}"
        );
        assert_eq!(p, tokyo_night_night());
    }

    #[test]
    fn builtin_themes_all_parse_and_differ() {
        for (name, _) in BUILTIN_THEMES {
            let (p, problems) = load_palette(name).unwrap();
            assert!(
                problems.is_empty(),
                "{name} has role problems: {problems:?}"
            );
            if *name != "tokyo-night" {
                assert_ne!(p, tokyo_night_night(), "{name} should differ from default");
            }
        }
    }

    #[test]
    fn missing_roles_inherit_base_and_unknown_reported() {
        let toml = r##"
            name = "partial"
            [headings]
            h1 = "#ff0000"
            [bogus]
            not_a_role = "#00ff00"
        "##;
        let tf: ThemeFile = toml::from_str(toml).unwrap();
        let (p, problems) = tf.into_palette(tokyo_night_night());
        assert_eq!(p.h1, Color::Rgb(255, 0, 0)); // overridden
        assert_eq!(p.h2, tokyo_night_night().h2); // inherited
        // `[bogus]` is an unknown table -> ignored entirely by serde, so no problem
        // is reported; a bad role under a *known* table would be.
        assert!(problems.is_empty());
    }

    #[test]
    fn unknown_role_under_known_table_is_reported() {
        let toml = r##"
            [headings]
            h7 = "#ff0000"
        "##;
        let tf: ThemeFile = toml::from_str(toml).unwrap();
        let (_, problems) = tf.into_palette(tokyo_night_night());
        assert_eq!(problems, vec!["h7".to_string()]);
    }

    #[test]
    fn missing_theme_is_err() {
        assert!(load_palette("does-not-exist").is_err());
    }
}
