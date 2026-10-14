//! Syntax highlighting for code blocks.
//!
//! Uses syntect for highlighting with Sublime Text syntax definitions.

use std::sync::{LazyLock, Mutex, OnceLock};

use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color, FontStyle, StyleModifier, Theme, ThemeItem, ThemeSet, ThemeSettings,
};
use syntect::parsing::SyntaxSet;

use crate::document::{InlineColor, InlineSpan, InlineStyle};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_rust_produces_colored_spans() {
        let code = "fn main() {\n    let x = 1;\n}\n";
        let lines = highlight_code(Some("rust"), code);

        assert_eq!(lines.len(), 3);
        let has_color = lines.iter().flatten().any(|span| span.style().fg.is_some());
        assert!(has_color, "Expected at least one colored span for Rust");
    }

    #[test]
    fn test_highlight_unknown_language_falls_back_to_plain() {
        let code = "just text";
        let lines = highlight_code(Some("nope"), code);

        assert_eq!(lines.len(), 1);
        let has_color = lines.iter().flatten().any(|span| span.style().fg.is_some());
        assert!(!has_color, "Unknown language should not colorize");
    }

    #[test]
    fn test_highlight_plain_code_sets_block_code_style() {
        for language in [None, Some("nope")] {
            let lines = highlight_code(language, "plain");
            assert!(lines[0].iter().all(|span| {
                let style = span.style();
                style.code && style.code_block && !style.strong && !style.emphasis
            }));
        }
    }

    #[test]
    fn test_highlight_continues_after_line_comment() {
        // Regression: highlighting stopped after the first // comment because
        // syntect's newlines-mode parser never saw the \n that ends the
        // comment scope, leaving all subsequent lines inside the comment.
        let code = "#include <stdio.h>\n// comment\nint main() {}\n";
        let lines = highlight_code(Some("c"), code);

        assert_eq!(lines.len(), 3, "expected 3 lines");
        // The third line ("int main() {}") must have coloured spans,
        // not plain comment grey.
        let has_color_after_comment = lines[2].iter().any(|span| {
            let fg = span.style().fg;
            // A real keyword like "int" should have a distinct colour;
            // if everything is the same grey as the comment, the bug is back.
            fg.is_some()
                && fg
                    != lines[1]
                        .first()
                        .and_then(|s| s.style().fg)
                        .as_ref()
                        .copied()
        });
        assert!(
            has_color_after_comment,
            "Code after a // comment must have different highlighting than the comment itself.\n\
             Line 2 (comment) spans: {:?}\nLine 3 (code) spans: {:?}",
            lines[1], lines[2]
        );
    }

    #[test]
    fn test_highlight_does_not_set_background_color() {
        let code = "fn main() {}";
        let lines = highlight_code(Some("rust"), code);
        let has_bg = lines.iter().flatten().any(|span| span.style().bg.is_some());
        assert!(!has_bg, "Highlighting should not override background");
    }

    #[test]
    fn test_colorfgbg_dark_background() {
        let mode = background_mode_from_colorfgbg(Some("15;0"));
        assert_eq!(mode, BackgroundMode::Dark);
    }

    #[test]
    fn test_colorfgbg_light_background() {
        let mode = background_mode_from_colorfgbg(Some("0;15"));
        assert_eq!(mode, BackgroundMode::Light);
    }

    #[test]
    fn test_background_override_light() {
        set_background_mode(Some(HighlightBackground::Light));
        assert_eq!(background_mode(), BackgroundMode::Light);
        set_background_mode(None);
    }

    #[test]
    fn test_background_override_dark() {
        set_background_mode(Some(HighlightBackground::Dark));
        assert_eq!(background_mode(), BackgroundMode::Dark);
        set_background_mode(None);
    }

    #[test]
    fn test_light_mode_darkens_bright_fg() {
        let bright = InlineColor {
            r: 240,
            g: 230,
            b: 120,
        };
        let adjusted = adjust_fg_for_background(bright, BackgroundMode::Light);
        assert!(adjusted.r < bright.r);
        assert!(adjusted.g < bright.g);
        assert!(adjusted.b < bright.b);
    }

    #[test]
    fn test_language_for_file_returns_rust_for_rs() {
        let lang = language_for_file(std::path::Path::new("foo.rs"));
        assert_eq!(lang, Some("Rust"));
    }

    #[test]
    fn test_language_for_file_returns_none_for_md() {
        let lang = language_for_file(std::path::Path::new("README.md"));
        assert!(lang.is_none(), "Markdown files should return None");
    }

    #[test]
    fn test_language_for_file_returns_none_for_unknown() {
        let lang = language_for_file(std::path::Path::new("foo.xyz"));
        assert!(lang.is_none(), "Unknown extensions should return None");
    }

    #[test]
    fn test_tokyo_night_semantic_colors() {
        let syntax_set = syntax_set();
        let syntax = syntax_set.find_syntax_by_token("rust").unwrap();
        let mut highlighter = HighlightLines::new(syntax, theme_for_mode(BackgroundMode::Dark));
        let ranges = highlighter
            .highlight_line(
                "fn main() { let x: u32 = 42; let s = \"hello\"; } // comment\n",
                syntax_set,
            )
            .unwrap();
        for (token, expected) in [
            ("fn", (0xc0, 0x99, 0xff)),
            ("main", (0x82, 0xaa, 0xff)),
            ("u32", (0x65, 0xbc, 0xff)),
            ("42", (0xff, 0x96, 0x6c)),
            ("hello", (0xc3, 0xe8, 0x8d)),
            ("=", (0x89, 0xdd, 0xff)),
            ("comment", (0x63, 0x6d, 0xa6)),
        ] {
            let (style, _) = ranges
                .iter()
                .find(|(_, text)| text.contains(token))
                .unwrap();
            assert_eq!(
                (style.foreground.r, style.foreground.g, style.foreground.b),
                expected,
                "Unexpected Tokyo Night color for {token}"
            );
            assert_eq!(
                style.font_style.contains(FontStyle::ITALIC),
                token == "comment",
                "Unexpected Tokyo Night font style for {token}"
            );
        }
    }

    #[test]
    fn test_moon_semantic_scope_colors_and_font_styles() {
        use syntect::highlighting::Highlighter;
        use syntect::parsing::Scope;

        let highlighter = Highlighter::new(theme_for_mode(BackgroundMode::Dark));
        for (scope, expected, italic) in [
            ("keyword.other.rust", (0xfc, 0xa7, 0xea), true),
            ("storage.modifier.rust", (0xfc, 0xa7, 0xea), true),
            ("keyword.control.rust", (0xc0, 0x99, 0xff), false),
            ("storage.type.function.rust", (0xc0, 0x99, 0xff), false),
            ("entity.name.function.rust", (0x82, 0xaa, 0xff), false),
            ("entity.name.type.rust", (0x65, 0xbc, 0xff), false),
            ("storage.type.rust", (0x65, 0xbc, 0xff), false),
            ("variable.parameter.rust", (0xff, 0xc7, 0x77), false),
            ("string.quoted.double.rust", (0xc3, 0xe8, 0x8d), false),
            ("comment.line.double-slash.rust", (0x63, 0x6d, 0xa6), true),
            ("constant.numeric.rust", (0xff, 0x96, 0x6c), false),
            ("constant.other.rust", (0xff, 0x96, 0x6c), false),
            ("keyword.operator.rust", (0x89, 0xdd, 0xff), false),
            (
                "punctuation.section.parens.begin.rust",
                (0x82, 0x8b, 0xb8),
                false,
            ),
            ("punctuation.separator.rust", (0x89, 0xdd, 0xff), false),
            ("constant.character.escape.rust", (0xc0, 0x99, 0xff), false),
            ("keyword.control.import.rust", (0x86, 0xe1, 0xfc), false),
            ("entity.name.namespace.rust", (0x86, 0xe1, 0xfc), false),
            ("variable.other.member.rust", (0x4f, 0xd6, 0xbe), false),
            ("variable.other.rust", (0xc8, 0xd3, 0xf5), false),
            ("variable.language.rust", (0xff, 0x75, 0x7f), false),
            ("invalid.illegal.rust", (0xc5, 0x3b, 0x53), false),
        ] {
            let style = highlighter.style_for_stack(&[Scope::new(scope).unwrap()]);
            assert_eq!(
                (style.foreground.r, style.foreground.g, style.foreground.b),
                expected,
                "Unexpected Moon color for {scope}"
            );
            assert_eq!(
                style.font_style.contains(FontStyle::ITALIC),
                italic,
                "Unexpected Moon italic style for {scope}"
            );
            assert!(!style.font_style.contains(FontStyle::BOLD));
        }
    }

    #[test]
    fn test_highlight_preserves_block_and_comment_metadata() {
        let lines = highlight_code_for_mode(
            Some("rust"),
            "// comment\nfn main() {}\n",
            BackgroundMode::Dark,
        );
        assert!(lines.iter().flatten().all(|span| span.style().code_block));
        assert!(lines[0].iter().all(|span| span.style().emphasis));
        assert!(lines.iter().flatten().all(|span| !span.style().strong));
    }

    #[test]
    fn test_light_mode_preserves_existing_color_conversion() {
        let code = "fn main() {} // comment\n";
        let syntax_set = syntax_set();
        let syntax = syntax_set.find_syntax_by_token("rust").unwrap();
        let mut highlighter = HighlightLines::new(syntax, theme_for_mode(BackgroundMode::Light));
        let expected = highlighter.highlight_line(code, syntax_set).unwrap();
        let lines = highlight_code_for_mode(Some("rust"), code, BackgroundMode::Light);
        let expected: Vec<_> = expected
            .into_iter()
            .filter(|(_, text)| !text.trim_end_matches('\n').is_empty())
            .collect();
        assert_eq!(lines[0].len(), expected.len());
        for (span, (style, text)) in lines[0].iter().zip(expected) {
            assert_eq!(span.text(), text.trim_end_matches('\n'));
            assert_eq!(
                span.style().fg,
                Some(adjust_fg_for_background(
                    InlineColor {
                        r: style.foreground.r,
                        g: style.foreground.g,
                        b: style.foreground.b,
                    },
                    BackgroundMode::Light,
                ))
            );
            assert!(!span.style().emphasis && !span.style().strong);
        }
    }

    #[test]
    fn test_dark_mode_preserves_palette_colors() {
        for color in [
            InlineColor {
                r: 0x63,
                g: 0x6d,
                b: 0xa6,
            },
            InlineColor {
                r: 0xc3,
                g: 0xe8,
                b: 0x8d,
            },
        ] {
            assert_eq!(adjust_fg_for_background(color, BackgroundMode::Dark), color);
        }
    }

    #[test]
    fn test_theme_selection_tracks_background_mode() {
        let dark = theme_for_mode(BackgroundMode::Dark);
        let light = theme_for_mode(BackgroundMode::Light);
        assert_eq!(dark.name.as_deref(), Some("Tokyo Night Moon"));
        assert_ne!(light.name, dark.name);
        assert!(std::ptr::eq(dark, theme_for_mode(BackgroundMode::Dark)));
        assert!(std::ptr::eq(light, theme_for_mode(BackgroundMode::Light)));
    }

    #[test]
    fn test_light_mode_caps_luma_for_readability() {
        let bright = InlineColor {
            r: 240,
            g: 230,
            b: 120,
        };
        let adjusted = adjust_fg_for_background(bright, BackgroundMode::Light);
        let luma = (0.2126 * adjusted.r as f32)
            + (0.7152 * adjusted.g as f32)
            + (0.0722 * adjusted.b as f32);
        assert!(luma < 120.0, "Adjusted color still too bright: {luma}");
    }
}

/// Returns the syntax language name for a file path, or `None` if the file
/// is markdown or has no recognized syntax.
pub fn language_for_file(path: &std::path::Path) -> Option<&'static str> {
    let ss = syntax_set();
    let syntax = ss.find_syntax_for_file(path).ok().flatten()?;
    if syntax.name == "Markdown" {
        return None;
    }
    // Leak the name so we get a 'static str — the SyntaxSet is 'static anyway.
    Some(leak_str(&syntax.name))
}

/// Cache interned strings to avoid leaking duplicates.
fn leak_str(s: &str) -> &'static str {
    use std::collections::HashSet;

    static INTERNED: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let lock = INTERNED.get_or_init(|| Mutex::new(HashSet::new()));
    let mut set = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(existing) = set.get(s) {
        return existing;
    }
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    set.insert(leaked);
    leaked
}

pub fn highlight_code(language: Option<&str>, code: &str) -> Vec<Vec<InlineSpan>> {
    highlight_code_for_mode(language, code, background_mode())
}

fn highlight_code_for_mode(
    language: Option<&str>,
    code: &str,
    mode: BackgroundMode,
) -> Vec<Vec<InlineSpan>> {
    let mut lines = Vec::new();
    let syntax_set = syntax_set();
    let syntax = language
        .and_then(|lang| syntax_set.find_syntax_by_token(lang))
        .or_else(|| language.and_then(|lang| syntax_set.find_syntax_by_name(lang)));

    let Some(syntax) = syntax else {
        for line in code.lines() {
            let style = InlineStyle {
                code: true,
                code_block: true,
                ..InlineStyle::default()
            };
            lines.push(vec![InlineSpan::new(line.to_string(), style)]);
        }
        return lines;
    };

    let mut highlighter = HighlightLines::new(syntax, theme_for_mode(mode));
    for line in code.lines() {
        // SyntaxSet::load_defaults_newlines() expects each line to end
        // with '\n'.  str::lines() strips newlines, so we must re-add it;
        // otherwise the parser never closes scopes that terminate at EOL
        // (e.g. `//` comments), and the unclosed scope bleeds into every
        // subsequent line.
        let line_nl = format!("{line}\n");
        let ranges = highlighter
            .highlight_line(&line_nl, syntax_set)
            .unwrap_or_default();
        let mut spans = Vec::new();
        for (style, text) in ranges {
            // Strip the trailing '\n' we added — it must not appear in the
            // rendered output.
            let text = text.trim_end_matches('\n');
            if text.is_empty() {
                continue;
            }
            let mut inline_style = InlineStyle {
                code: true,
                code_block: true,
                emphasis: mode == BackgroundMode::Dark
                    && style.font_style.contains(FontStyle::ITALIC),
                strong: mode == BackgroundMode::Dark && style.font_style.contains(FontStyle::BOLD),
                ..InlineStyle::default()
            };
            let fg = InlineColor {
                r: style.foreground.r,
                g: style.foreground.g,
                b: style.foreground.b,
            };
            inline_style.fg = Some(adjust_fg_for_background(fg, mode));
            spans.push(InlineSpan::new(text.to_string(), inline_style));
        }
        lines.push(spans);
    }

    lines
}

fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(|| {
        let _scope = crate::perf::scope("highlight.syntax_set.load_defaults");
        SyntaxSet::load_defaults_newlines()
    })
}

fn theme_for_mode(mode: BackgroundMode) -> &'static Theme {
    static DARK_THEME: LazyLock<Theme> = LazyLock::new(tokyo_night_theme);
    static LIGHT_THEME: LazyLock<Theme> = LazyLock::new(|| {
        let _scope = crate::perf::scope("highlight.theme.load_defaults");
        let theme_set = ThemeSet::load_defaults();
        for name in ["InspiredGitHub", "Solarized (light)", "base16-ocean.light"] {
            if let Some(theme) = theme_set.themes.get(name) {
                return theme.clone();
            }
        }
        theme_set
            .themes
            .values()
            .next()
            .cloned()
            .unwrap_or_default()
    });
    match mode {
        BackgroundMode::Dark => &DARK_THEME,
        BackgroundMode::Light => &LIGHT_THEME,
    }
}

fn tokyo_night_theme() -> Theme {
    // Match the effective Neovim Tokyo Night Moon groups using TextMate scopes.
    // The shared UI palette is the only source of RGB values.
    use crate::ui::style::tokyo_night;

    let rgb = |color| {
        let ratatui::style::Color::Rgb(r, g, b) = color else {
            unreachable!("Tokyo Night palette colors must be RGB");
        };
        Color { r, g, b, a: 255 }
    };
    let scopes = [
        ("comment", tokyo_night::COMMENT, FontStyle::ITALIC),
        ("keyword, storage", tokyo_night::PURPLE, FontStyle::ITALIC),
        (
            "keyword.control, storage.type.function",
            tokyo_night::MAGENTA,
            FontStyle::empty(),
        ),
        (
            "keyword.control.import, keyword.control.include, entity.name.namespace",
            tokyo_night::BRIGHT_CYAN,
            FontStyle::empty(),
        ),
        (
            "string, constant.character",
            tokyo_night::GREEN,
            FontStyle::empty(),
        ),
        (
            "entity.name.function, support.function",
            tokyo_night::BLUE,
            FontStyle::empty(),
        ),
        (
            "entity.name.type, entity.name.class, support.type, support.class, storage.type",
            tokyo_night::BLUE1,
            FontStyle::empty(),
        ),
        ("variable.parameter", tokyo_night::YELLOW, FontStyle::empty()),
        ("constant", tokyo_night::ORANGE, FontStyle::empty()),
        (
            "keyword.operator, punctuation",
            tokyo_night::OPERATOR,
            FontStyle::empty(),
        ),
        (
            "punctuation.section, punctuation.definition.brackets, punctuation.definition.array, punctuation.definition.parameters, punctuation.definition.arguments",
            tokyo_night::DARK_FG,
            FontStyle::empty(),
        ),
        (
            "punctuation.definition.string",
            tokyo_night::GREEN,
            FontStyle::empty(),
        ),
        (
            "punctuation.definition.comment",
            tokyo_night::COMMENT,
            FontStyle::ITALIC,
        ),
        (
            "constant.character.escape",
            tokyo_night::MAGENTA,
            FontStyle::empty(),
        ),
        (
            "entity.name.tag, entity.name.label",
            tokyo_night::BLUE,
            FontStyle::empty(),
        ),
        ("variable.language", tokyo_night::RED, FontStyle::empty()),
        (
            "entity.other.attribute-name, variable.other.member",
            tokyo_night::CYAN,
            FontStyle::empty(),
        ),
        ("invalid", tokyo_night::ERROR, FontStyle::empty()),
    ]
    .into_iter()
    .map(|(scope, foreground, font_style)| ThemeItem {
        scope: scope.parse().expect("valid Tokyo Night Moon scope selector"),
        style: StyleModifier {
            foreground: Some(rgb(foreground)),
            font_style: Some(font_style),
            ..StyleModifier::default()
        },
    })
    .collect();
    Theme {
        name: Some("Tokyo Night Moon".to_string()),
        settings: ThemeSettings {
            foreground: Some(rgb(tokyo_night::FG)),
            background: Some(rgb(tokyo_night::BG)),
            selection: Some(rgb(tokyo_night::SELECTION)),
            ..ThemeSettings::default()
        },
        scopes,
        ..Theme::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackgroundMode {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightBackground {
    Light,
    Dark,
}

static BACKGROUND_OVERRIDE: OnceLock<Mutex<Option<HighlightBackground>>> = OnceLock::new();

/// Sets the background mode override for syntax highlighting.
///
pub fn set_background_mode(mode: Option<HighlightBackground>) {
    let lock = BACKGROUND_OVERRIDE.get_or_init(|| Mutex::new(None));
    let mut guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = mode;
}

pub fn set_background_mode_from_rgb(r: u8, g: u8, b: u8) {
    let luma = 0.2126f32.mul_add(
        f32::from(r),
        0.7152f32.mul_add(f32::from(g), 0.0722 * f32::from(b)),
    );
    let mode = if luma >= 140.0 {
        HighlightBackground::Light
    } else {
        HighlightBackground::Dark
    };
    set_background_mode(Some(mode));
}

fn background_mode() -> BackgroundMode {
    let lock = BACKGROUND_OVERRIDE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = lock.lock()
        && let Some(mode) = *guard
    {
        return match mode {
            HighlightBackground::Light => BackgroundMode::Light,
            HighlightBackground::Dark => BackgroundMode::Dark,
        };
    }
    background_mode_from_colorfgbg(std::env::var("COLORFGBG").ok().as_deref())
}

pub fn is_light_background() -> bool {
    background_mode() == BackgroundMode::Light
}

fn background_mode_from_colorfgbg(colorfgbg: Option<&str>) -> BackgroundMode {
    let Some(value) = colorfgbg else {
        return BackgroundMode::Dark;
    };
    let bg_str = value.rsplit(';').next().unwrap_or(value);
    let Ok(bg) = bg_str.parse::<u8>() else {
        return BackgroundMode::Dark;
    };

    if bg >= 7 {
        BackgroundMode::Light
    } else {
        BackgroundMode::Dark
    }
}

fn adjust_fg_for_background(color: InlineColor, mode: BackgroundMode) -> InlineColor {
    match mode {
        BackgroundMode::Dark => color,
        BackgroundMode::Light => {
            let luma = 0.2126f32.mul_add(
                f32::from(color.r),
                0.7152f32.mul_add(f32::from(color.g), 0.0722 * f32::from(color.b)),
            );
            if luma < 155.0 {
                return color;
            }

            // Clamped to [0, 255]
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            InlineColor {
                r: (f32::from(color.r) * 0.42).round().clamp(0.0, 255.0) as u8,
                g: (f32::from(color.g) * 0.42).round().clamp(0.0, 255.0) as u8,
                b: (f32::from(color.b) * 0.42).round().clamp(0.0, 255.0) as u8,
            }
        }
    }
}
