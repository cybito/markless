use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::app::Model;

use super::style::{themed_style, tokyo_night};

pub fn render_hover_link_bar(model: &Model, frame: &mut Frame, area: Rect) {
    let Some(url) = model.hovered_link_url.as_deref() else {
        return;
    };
    let bar = Paragraph::new(format!("link: {url}")).style(themed_style(
        Style::default().bg(Color::Blue).fg(Color::White),
        Style::default()
            .bg(tokyo_night::BLUE)
            .fg(tokyo_night::DARK_BG),
    ));
    frame.render_widget(bar, area);
}

pub fn render_search_bar(model: &Model, frame: &mut Frame, area: Rect) {
    let query = model.search_query.as_deref().unwrap_or_default();
    let large_file = model.document.line_count() > 10_000;
    let deferred = large_file && model.search_match_count() == 0 && !query.trim().is_empty();
    let match_info = if query.trim().is_empty() {
        String::new()
    } else if deferred {
        "  [Enter to search]".to_string()
    } else if let Some((current, total)) = model.current_search_match() {
        format!("  [{current}/{total}]")
    } else {
        String::new()
    };
    let text = format!("/{query}{match_info}  Enter: next  Esc: clear");
    let bar = Paragraph::new(text).style(themed_style(
        Style::default()
            .bg(Color::Rgb(30, 58, 95))
            .fg(Color::Rgb(255, 255, 255)),
        Style::default()
            .bg(tokyo_night::LIGHTER_BG)
            .fg(tokyo_night::BRIGHT_FG),
    ));
    frame.render_widget(bar, area);
}

pub fn render_status_bar(model: &Model, frame: &mut Frame, area: Rect) {
    let filename = model.file_path.file_name().map_or_else(
        || "untitled".to_string(),
        |s| s.to_string_lossy().to_string(),
    );

    let percent = model.viewport.scroll_percent();
    let line_info = format!(
        "Line {}/{}",
        model.viewport.offset() + 1,
        model.viewport.total_lines()
    );

    let watch_indicator = if model.watch_enabled {
        " [watching]"
    } else {
        ""
    };
    let toc_indicator = if model.toc_visible { " [TOC]" } else { "" };

    let status =
        format!(" {filename}  [{percent}%]  {line_info}{watch_indicator}{toc_indicator}  ?:help");

    let status_bar = Paragraph::new(status).style(themed_style(
        Style::default().bg(Color::DarkGray).fg(Color::White),
        Style::default()
            .bg(tokyo_night::DARK_BG)
            .fg(tokyo_night::FG),
    ));

    frame.render_widget(status_bar, area);
}

pub fn render_toast_bar(model: &Model, frame: &mut Frame, area: Rect) {
    let Some((message, level)) = model.active_toast() else {
        return;
    };
    let (prefix, style) = match level {
        crate::app::ToastLevel::Info => (
            "[info]",
            themed_style(
                Style::default().bg(Color::DarkGray).fg(Color::White),
                Style::default()
                    .bg(tokyo_night::LIGHTER_BG)
                    .fg(tokyo_night::FG),
            ),
        ),
        crate::app::ToastLevel::Warning => (
            "[warn]",
            themed_style(
                Style::default().bg(Color::Yellow).fg(Color::Black),
                Style::default()
                    .bg(tokyo_night::YELLOW)
                    .fg(tokyo_night::DARK_BG),
            ),
        ),
        crate::app::ToastLevel::Error => (
            "[error]",
            themed_style(
                Style::default().bg(Color::Red).fg(Color::White),
                Style::default()
                    .bg(tokyo_night::RED)
                    .fg(tokyo_night::DARK_BG),
            ),
        ),
    };
    let toast = Paragraph::new(format!("{prefix} {message}")).style(style);
    frame.render_widget(toast, area);
}
