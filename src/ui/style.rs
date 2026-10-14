//! Theming and color definitions.
//!
//! This module defines the visual styling for rendered markdown elements.
//! Dark mode uses the Omarchy Tokyo Night palette; light mode retains ANSI styling.

use ratatui::style::{Color, Modifier, Style};

use crate::document::{InlineStyle, LineType};

/// Authoritative Omarchy Tokyo Night colors.
pub(crate) mod tokyo_night {
    use ratatui::style::Color;

    pub const BG: Color = Color::Rgb(0x1a, 0x1b, 0x26);
    pub const DARK_BG: Color = Color::Rgb(0x13, 0x14, 0x1c);
    pub const LIGHTER_BG: Color = Color::Rgb(0x24, 0x28, 0x3b);
    pub const FG: Color = Color::Rgb(0xa9, 0xb1, 0xd6);
    pub const BRIGHT_FG: Color = Color::Rgb(0xc0, 0xca, 0xf5);
    pub const MUTED: Color = Color::Rgb(0x41, 0x48, 0x68);
    pub const DARK_FG: Color = Color::Rgb(0x56, 0x5f, 0x89);
    pub const SELECTION: Color = Color::Rgb(0x29, 0x2e, 0x42);
    pub const RED: Color = Color::Rgb(0xf7, 0x76, 0x8e);
    pub const YELLOW: Color = Color::Rgb(0xe0, 0xaf, 0x68);
    pub const GREEN: Color = Color::Rgb(0x9e, 0xce, 0x6a);
    pub const CYAN: Color = Color::Rgb(0x44, 0x9d, 0xab);
    pub const BLUE: Color = Color::Rgb(0x7a, 0xa2, 0xf7);
    pub const MAGENTA: Color = Color::Rgb(0xad, 0x8e, 0xe6);
    pub const ORANGE: Color = Color::Rgb(0xeb, 0x92, 0x7b);
    pub const BRIGHT_CYAN: Color = Color::Rgb(0x0d, 0xb9, 0xd7);
    pub const BRIGHT_MAGENTA: Color = Color::Rgb(0xbb, 0x9a, 0xf7);
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
        // Headings - bold with distinct colors per level
        LineType::Heading(1) => base
            .fg(if light_bg {
                Color::Indexed(24)
            } else {
                tokyo_night::CYAN
            })
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        LineType::Heading(2) => base
            .fg(if light_bg {
                Color::Indexed(22)
            } else {
                tokyo_night::GREEN
            })
            .add_modifier(Modifier::BOLD),
        LineType::Heading(3) => base
            .fg(if light_bg {
                Color::Indexed(58)
            } else {
                tokyo_night::YELLOW
            })
            .add_modifier(Modifier::BOLD),
        LineType::Heading(4) => base
            .fg(if light_bg {
                Color::Indexed(24)
            } else {
                tokyo_night::BLUE
            })
            .add_modifier(Modifier::BOLD),
        LineType::Heading(5) => base
            .fg(if light_bg {
                Color::Indexed(54)
            } else {
                tokyo_night::MAGENTA
            })
            .add_modifier(Modifier::BOLD),
        LineType::Heading(_) => base
            .fg(if light_bg {
                Color::Indexed(24)
            } else {
                tokyo_night::CYAN
            })
            .add_modifier(Modifier::BOLD),

        // Code blocks - readable base color without DIM
        LineType::CodeBlock => base.fg(if light_bg {
            Color::Indexed(238)
        } else {
            tokyo_night::BRIGHT_FG
        }),

        // Block quotes - italic blue
        LineType::BlockQuote => base
            .fg(if light_bg {
                Color::Indexed(24)
            } else {
                tokyo_night::BLUE
            })
            .add_modifier(Modifier::ITALIC),

        // Horizontal rule - dim
        LineType::HorizontalRule => base
            .fg(if light_bg {
                Color::Indexed(241)
            } else {
                tokyo_night::MUTED
            })
            .add_modifier(Modifier::DIM),

        // Images - magenta italic to stand out as placeholder
        LineType::Image => base
            .fg(if light_bg {
                Color::Indexed(90)
            } else {
                tokyo_night::MAGENTA
            })
            .add_modifier(Modifier::ITALIC),

        // Math blocks - green italic
        LineType::Math => base
            .fg(if light_bg {
                Color::Indexed(22)
            } else {
                tokyo_night::GREEN
            })
            .add_modifier(Modifier::ITALIC),

        // List items, tables, paragraphs, empty lines - normal style
        LineType::ListItem(_) | LineType::Table | LineType::Paragraph | LineType::Empty => base,
    }
}

/// Get the style for an inline span, merged with a base line style.
pub fn style_for_inline(base: Style, inline: InlineStyle) -> Style {
    let mut style = base;

    if let Some(fg) = inline.fg {
        style = style
            .fg(fg_color_for_terminal(fg))
            .remove_modifier(Modifier::DIM);
    }
    if let Some(bg) = inline.bg {
        style = style.bg(Color::Rgb(bg.r, bg.g, bg.b));
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
                tokyo_night::BLUE
            });
        }
    }
    if inline.math && inline.fg.is_none() {
        let light_bg = crate::highlight::is_light_background();
        style = style
            .fg(if light_bg {
                Color::Indexed(22)
            } else {
                tokyo_night::GREEN
            })
            .add_modifier(Modifier::ITALIC);
    }
    if inline.code && inline.fg.is_none() {
        let light_bg = crate::highlight::is_light_background();
        style = style
            .fg(if light_bg {
                Color::Indexed(88)
            } else {
                tokyo_night::RED
            })
            .add_modifier(Modifier::BOLD);
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
    /// Heading level 4+ style
    pub h4: Style,
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
            h1: Style::default()
                .fg(tokyo_night::CYAN)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            h2: Style::default()
                .fg(tokyo_night::GREEN)
                .add_modifier(Modifier::BOLD),
            h3: Style::default()
                .fg(tokyo_night::YELLOW)
                .add_modifier(Modifier::BOLD),
            h4: Style::default()
                .fg(tokyo_night::BLUE)
                .add_modifier(Modifier::BOLD),
            code: Style::default().fg(tokyo_night::BRIGHT_FG),
            inline_code: Style::default()
                .fg(tokyo_night::RED)
                .add_modifier(Modifier::BOLD),
            quote: Style::default()
                .fg(tokyo_night::BLUE)
                .add_modifier(Modifier::ITALIC),
            link: Style::default()
                .fg(tokyo_night::BLUE)
                .add_modifier(Modifier::UNDERLINED),
            emphasis: Style::default().add_modifier(Modifier::ITALIC),
            strong: Style::default().add_modifier(Modifier::BOLD),
            strikethrough: Style::default().add_modifier(Modifier::CROSSED_OUT),
            list_marker: Style::default().fg(tokyo_night::YELLOW),
            table_border: Style::default().fg(tokyo_night::MUTED),
            image: Style::default()
                .fg(tokyo_night::MAGENTA)
                .add_modifier(Modifier::ITALIC),
            hr: Style::default().fg(tokyo_night::MUTED),
            status_bg: tokyo_night::DARK_BG,
            status_fg: tokyo_night::FG,
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
            h1: Style::default()
                .fg(Color::Indexed(31)) // Darker cyan
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            h2: Style::default()
                .fg(Color::Indexed(28)) // Darker green
                .add_modifier(Modifier::BOLD),
            h3: Style::default()
                .fg(Color::Indexed(136)) // Darker yellow/olive
                .add_modifier(Modifier::BOLD),
            h4: Style::default()
                .fg(Color::Indexed(25)) // Darker blue
                .add_modifier(Modifier::BOLD),
            code: Style::default().fg(Color::Indexed(240)),
            inline_code: Style::default()
                .fg(Color::Indexed(124)) // Darker red
                .add_modifier(Modifier::BOLD),
            quote: Style::default()
                .fg(Color::Indexed(25))
                .add_modifier(Modifier::ITALIC),
            link: Style::default()
                .fg(Color::Indexed(25))
                .add_modifier(Modifier::UNDERLINED),
            emphasis: Style::default().add_modifier(Modifier::ITALIC),
            strong: Style::default().add_modifier(Modifier::BOLD),
            strikethrough: Style::default().add_modifier(Modifier::CROSSED_OUT),
            list_marker: Style::default().fg(Color::Indexed(136)),
            table_border: Style::default().fg(Color::Indexed(245)),
            image: Style::default()
                .fg(Color::Indexed(133))
                .add_modifier(Modifier::ITALIC),
            hr: Style::default().fg(Color::Indexed(245)),
            status_bg: Color::Indexed(252),
            status_fg: Color::Indexed(235),
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
    fn test_h1_is_underlined() {
        let style = style_for_line_type(&LineType::Heading(1));
        assert!(style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn test_code_block_style() {
        let style = style_for_line_type(&LineType::CodeBlock);
        assert!(style.fg.is_some());
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
    fn test_default_theme() {
        let theme = Theme::default();
        assert!(theme.h1.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_dark_theme() {
        let theme = Theme::dark();
        assert!(theme.h1.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_light_theme() {
        let theme = Theme::light();
        assert!(theme.h1.add_modifier.contains(Modifier::BOLD));
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
