//! Theming and color definitions.
//!
//! This module defines the visual styling for rendered markdown elements.
//! Dark mode matches the effective Neovim Tokyo Night Moon highlights.

use ratatui::style::{Color, Modifier, Style};

use crate::document::{InlineStyle, LineType};

/// Authoritative Tokyo Night Moon colors.
pub(crate) mod tokyo_night {
    use ratatui::style::Color;

    pub const BG: Color = Color::Rgb(0x22, 0x24, 0x36);
    pub const DARK_BG: Color = Color::Rgb(0x1e, 0x20, 0x30);
    pub const LIGHTER_BG: Color = Color::Rgb(0x2f, 0x33, 0x4d);
    pub const FG: Color = Color::Rgb(0xc8, 0xd3, 0xf5);
    pub const BRIGHT_FG: Color = FG;
    pub const MUTED: Color = Color::Rgb(0x3b, 0x42, 0x61);
    pub const DARK_FG: Color = Color::Rgb(0x82, 0x8b, 0xb8);
    pub const SELECTION: Color = Color::Rgb(0x2d, 0x3f, 0x76);
    pub const RED: Color = Color::Rgb(0xff, 0x75, 0x7f);
    pub const YELLOW: Color = Color::Rgb(0xff, 0xc7, 0x77);
    pub const GREEN: Color = Color::Rgb(0xc3, 0xe8, 0x8d);
    pub const CYAN: Color = Color::Rgb(0x4f, 0xd6, 0xbe);
    pub const BLUE: Color = Color::Rgb(0x82, 0xaa, 0xff);
    pub const MAGENTA: Color = Color::Rgb(0xc0, 0x99, 0xff);
    pub const ORANGE: Color = Color::Rgb(0xff, 0x96, 0x6c);
    pub const BRIGHT_CYAN: Color = Color::Rgb(0x86, 0xe1, 0xfc);
    pub const BRIGHT_MAGENTA: Color = Color::Rgb(0xc0, 0x99, 0xff);
    pub const BLUE1: Color = Color::Rgb(0x65, 0xbc, 0xff);
    pub const PURPLE: Color = Color::Rgb(0xfc, 0xa7, 0xea);
    pub const OPERATOR: Color = Color::Rgb(0x89, 0xdd, 0xff);
    pub const COMMENT: Color = Color::Rgb(0x63, 0x6d, 0xa6);
    pub const ERROR: Color = Color::Rgb(0xc5, 0x3b, 0x53);
    pub const INFO: Color = Color::Rgb(0x0d, 0xb9, 0xd7);
    pub const FLOAT_BORDER: Color = Color::Rgb(0x58, 0x9e, 0xd7);
    pub const POPUP_SELECTION: Color = Color::Rgb(0x36, 0x3c, 0x58);
    pub const SEARCH: Color = Color::Rgb(0x3e, 0x68, 0xd7);
    /// Moon `bg_dark1`: darker than the page so inline code keeps blue text readable.
    pub const INLINE_CODE_BG: Color = Color::Rgb(0x19, 0x1b, 0x29);
    pub const H1_BG: Color = Color::Rgb(0x2c, 0x31, 0x4a);
    pub const H2_BG: Color = Color::Rgb(0x38, 0x34, 0x3d);
    pub const H3_BG: Color = Color::Rgb(0x32, 0x38, 0x3f);
    pub const H4_BG: Color = Color::Rgb(0x27, 0x36, 0x44);
    pub const H5_BG: Color = Color::Rgb(0x32, 0x30, 0x4a);
    pub const H6_BG: Color = Color::Rgb(0x38, 0x31, 0x48);
}

pub(crate) fn themed_color(light: Color, dark: Color) -> Color {
    if crate::highlight::is_light_background() {
        light
    } else {
        dark
    }
}

pub(crate) fn themed_style(light: Style, dark: Style) -> Style {
    if crate::highlight::is_light_background() {
        light
    } else {
        dark
    }
}

pub(crate) fn content_style() -> Style {
    themed_style(
        Style::default(),
        Style::default().fg(tokyo_night::FG).bg(tokyo_night::BG),
    )
}

/// Get the style for a given line type.
pub fn style_for_line_type(line_type: &LineType) -> Style {
    let light_bg = crate::highlight::is_light_background();
    let base = if light_bg {
        Style::default()
    } else {
        Style::default().fg(tokyo_night::FG).bg(tokyo_night::BG)
    };
    match line_type {
        LineType::Heading(level) => base.patch(heading_style(*level, light_bg)),

        // Plain fenced code uses Markdown's raw-block highlight.
        LineType::CodeBlock => base.fg(if light_bg {
            Color::Indexed(238)
        } else {
            tokyo_night::GREEN
        }),

        LineType::BlockQuote => {
            if light_bg {
                base.fg(Color::Indexed(24)).add_modifier(Modifier::ITALIC)
            } else {
                base
            }
        }
        LineType::HorizontalRule => {
            let style = base.fg(if light_bg {
                Color::Indexed(241)
            } else {
                tokyo_night::MUTED
            });
            if light_bg {
                style.add_modifier(Modifier::DIM)
            } else {
                style
            }
        }

        LineType::Image => {
            if light_bg {
                base.fg(Color::Indexed(90)).add_modifier(Modifier::ITALIC)
            } else {
                base.fg(tokyo_night::BLUE1)
            }
        }

        LineType::Math => {
            if light_bg {
                base.fg(Color::Indexed(22)).add_modifier(Modifier::ITALIC)
            } else {
                base.fg(tokyo_night::BLUE1)
            }
        }

        // List items, tables, paragraphs, empty lines - normal style
        LineType::ListItem(_) | LineType::Table | LineType::Paragraph | LineType::Empty => base,
    }
}

fn heading_style(level: u8, light: bool) -> Style {
    if light {
        let fg = match level {
            2 => Color::Indexed(22),
            3 => Color::Indexed(58),
            5 => Color::Indexed(54),
            _ => Color::Indexed(24),
        };
        let style = Style::default().fg(fg).add_modifier(Modifier::BOLD);
        return if level == 1 {
            style.add_modifier(Modifier::UNDERLINED)
        } else {
            style
        };
    }
    let (fg, bg) = match level {
        1 => (tokyo_night::BLUE, tokyo_night::H1_BG),
        2 => (tokyo_night::YELLOW, tokyo_night::H2_BG),
        3 => (tokyo_night::GREEN, tokyo_night::H3_BG),
        4 => (tokyo_night::CYAN, tokyo_night::H4_BG),
        5 => (tokyo_night::MAGENTA, tokyo_night::H5_BG),
        _ => (tokyo_night::PURPLE, tokyo_night::H6_BG),
    };
    Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD)
}

/// Get the style for an inline span, merged with a base line style.
pub fn style_for_inline(base: Style, inline: InlineStyle) -> Style {
    let mut style = base;
    if inline.code_frame && !crate::highlight::is_light_background() {
        style = style.fg(tokyo_night::MUTED);
    }

    if let Some(fg) = inline.fg {
        style = style
            .fg(fg_color_for_terminal(fg))
            .remove_modifier(Modifier::DIM);
    }
    if let Some(bg) = inline.bg {
        style = style.bg(Color::Rgb(bg.r, bg.g, bg.b));
    }
    if inline.code
        && !inline.code_block
        && inline.bg.is_none()
        && !crate::highlight::is_light_background()
    {
        style = style.bg(tokyo_night::INLINE_CODE_BG);
    }

    if inline.emphasis {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if inline.strong {
        style = style.add_modifier(Modifier::BOLD);
    }
    if inline.strikethrough {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    if inline.link {
        style = style.add_modifier(Modifier::UNDERLINED);
        if inline.fg.is_none() {
            let light_bg = crate::highlight::is_light_background();
            style = style.fg(if light_bg {
                Color::Blue
            } else {
                tokyo_night::CYAN
            });
        }
    }
    if inline.math && inline.fg.is_none() {
        if crate::highlight::is_light_background() {
            style = style.fg(Color::Indexed(22)).add_modifier(Modifier::ITALIC);
        } else {
            style = style.fg(tokyo_night::BLUE1);
        }
    }
    if inline.code && inline.fg.is_none() {
        if crate::highlight::is_light_background() {
            style = style.fg(Color::Indexed(88)).add_modifier(Modifier::BOLD);
        } else if inline.code_block {
            style = style.fg(tokyo_night::GREEN);
        } else {
            style = style.fg(tokyo_night::BLUE);
        }
    }
    if inline.list_marker && !crate::highlight::is_light_background() {
        if inline.fg.is_none() {
            style = style.fg(tokyo_night::ORANGE);
        }
        style = style.add_modifier(Modifier::BOLD);
    }

    style
}

fn fg_color_for_terminal(fg: crate::document::InlineColor) -> Color {
    if supports_truecolor() {
        Color::Rgb(fg.r, fg.g, fg.b)
    } else {
        Color::Indexed(rgb_to_xterm_256(fg.r, fg.g, fg.b))
    }
}

fn supports_truecolor() -> bool {
    if let Ok(force) = std::env::var("MARKLESS_TRUECOLOR") {
        let value = force.to_ascii_lowercase();
        return matches!(value.as_str(), "1" | "true" | "yes" | "on");
    }
    supports_truecolor_from_env(
        std::env::var("COLORTERM").ok().as_deref(),
        std::env::var("TERM").ok().as_deref(),
    )
}

fn supports_truecolor_from_env(colorterm: Option<&str>, term: Option<&str>) -> bool {
    if let Some(ct) = colorterm {
        let lower = ct.to_ascii_lowercase();
        if lower.contains("truecolor") || lower.contains("24bit") {
            return true;
        }
    }
    if let Some(t) = term {
        let lower = t.to_ascii_lowercase();
        if lower.contains("direct") || lower.contains("truecolor") {
            return true;
        }
    }
    false
}

fn rgb_to_xterm_256(r: u8, g: u8, b: u8) -> u8 {
    // Result is always 0-5, fits in u8
    #[allow(clippy::cast_possible_truncation)]
    let to_cube = |v: u8| ((u16::from(v) * 5) / 255) as u8;
    let ri = to_cube(r);
    let gi = to_cube(g);
    let bi = to_cube(b);
    16 + (36 * ri) + (6 * gi) + bi
}

/// Theme configuration for the entire application.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Heading level 1 style
    pub h1: Style,
    /// Heading level 2 style
    pub h2: Style,
    /// Heading level 3 style
    pub h3: Style,
    /// Heading level 4 style
    pub h4: Style,
    /// Heading level 5 style
    pub h5: Style,
    /// Heading level 6 style
    pub h6: Style,
    /// Code block style
    pub code: Style,
    /// Inline code style
    pub inline_code: Style,
    /// Block quote style
    pub quote: Style,
    /// Link style
    pub link: Style,
    /// Emphasis (italic) style
    pub emphasis: Style,
    /// Strong (bold) style
    pub strong: Style,
    /// Strikethrough style
    pub strikethrough: Style,
    /// List bullet/number style
    pub list_marker: Style,
    /// Table border style
    pub table_border: Style,
    /// Image placeholder style
    pub image: Style,
    /// Horizontal rule style
    pub hr: Style,
    /// Status bar background
    pub status_bg: Color,
    /// Status bar foreground
    pub status_fg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            h1: heading_style(1, false),
            h2: heading_style(2, false),
            h3: heading_style(3, false),
            h4: heading_style(4, false),
            h5: heading_style(5, false),
            h6: heading_style(6, false),
            code: Style::default().fg(tokyo_night::GREEN),
            inline_code: Style::default()
                .fg(tokyo_night::BLUE)
                .bg(tokyo_night::INLINE_CODE_BG),
            quote: Style::default(),
            link: Style::default()
                .fg(tokyo_night::CYAN)
                .add_modifier(Modifier::UNDERLINED),
            emphasis: Style::default().add_modifier(Modifier::ITALIC),
            strong: Style::default().add_modifier(Modifier::BOLD),
            strikethrough: Style::default().add_modifier(Modifier::CROSSED_OUT),
            list_marker: Style::default()
                .fg(tokyo_night::ORANGE)
                .add_modifier(Modifier::BOLD),
            table_border: Style::default(),
            image: Style::default().fg(tokyo_night::BLUE1),
            hr: Style::default().fg(tokyo_night::MUTED),
            status_bg: tokyo_night::DARK_BG,
            status_fg: tokyo_night::DARK_FG,
        }
    }
}

impl Theme {
    /// Create a theme optimized for dark terminals.
    pub fn dark() -> Self {
        Self::default()
    }

    /// Create a theme optimized for light terminals.
    pub fn light() -> Self {
        Self {
            h1: heading_style(1, true),
            h2: heading_style(2, true),
            h3: heading_style(3, true),
            h4: heading_style(4, true),
            h5: heading_style(5, true),
            h6: heading_style(6, true),
            code: Style::default().fg(Color::Indexed(238)),
            inline_code: Style::default()
                .fg(Color::Indexed(88))
                .add_modifier(Modifier::BOLD),
            quote: Style::default()
                .fg(Color::Indexed(24))
                .add_modifier(Modifier::ITALIC),
            link: Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::UNDERLINED),
            emphasis: Style::default().add_modifier(Modifier::ITALIC),
            strong: Style::default().add_modifier(Modifier::BOLD),
            strikethrough: Style::default().add_modifier(Modifier::CROSSED_OUT),
            list_marker: Style::default(),
            table_border: Style::default(),
            image: Style::default()
                .fg(Color::Indexed(90))
                .add_modifier(Modifier::ITALIC),
            hr: Style::default()
                .fg(Color::Indexed(241))
                .add_modifier(Modifier::DIM),
            status_bg: Color::DarkGray,
            status_fg: Color::White,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::InlineColor;

    #[test]
    fn test_heading_styles_are_bold() {
        for level in 1..=6 {
            let style = style_for_line_type(&LineType::Heading(level));
            assert!(style.add_modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn test_code_block_not_dim() {
        let style = style_for_line_type(&LineType::CodeBlock);
        assert!(
            !style.add_modifier.contains(Modifier::DIM),
            "Code blocks should not use DIM modifier for better readability"
        );
    }

    #[test]
    fn test_light_theme_code_not_dim() {
        let theme = Theme::light();
        assert!(
            !theme.code.add_modifier.contains(Modifier::DIM),
            "Light theme code blocks should not use DIM modifier"
        );
    }

    #[test]
    fn test_inline_color_removes_dim_modifier() {
        let base = Style::default().add_modifier(Modifier::DIM);
        let mut inline = InlineStyle::default();
        inline.fg = Some(InlineColor { r: 255, g: 0, b: 0 });

        let styled = style_for_inline(base, inline);
        assert!(!styled.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn test_truecolor_detection_without_colorterm() {
        assert!(!supports_truecolor_from_env(None, Some("xterm-256color")));
    }

    #[test]
    fn test_truecolor_detection_with_colorterm() {
        assert!(supports_truecolor_from_env(
            Some("truecolor"),
            Some("xterm-256color")
        ));
    }

    #[test]
    fn test_fallback_indexed_color_when_not_truecolor() {
        let idx = rgb_to_xterm_256(255, 0, 0);
        assert_eq!(idx, 196);
    }
}
