//! Markdown parsing with comrak.

use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;
use std::ops::Range;

use anyhow::Result;
use comrak::nodes::{AstNode, NodeValue, TableAlignment};
use comrak::{Arena, Options, parse_document};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::types::{
    CodeBlockRef, Document, HeadingRef, ImageRef, InlineSpan, InlineStyle, LineType, LinkRef,
    ParsedDocument, RenderedLine,
};

/// Options controlling how diagrams and math expressions are rendered.
pub struct DiagramRenderOpts<'a> {
    /// Render mermaid code blocks as image placeholders.
    pub mermaid_as_images: bool,
    /// Mermaid sources that failed to render (fall back to code blocks).
    pub failed_mermaid_srcs: &'a HashSet<String>,
    /// Render display math as image placeholders.
    pub math_as_images: bool,
    /// Math sources that failed to render (fall back to text blocks).
    pub failed_math_srcs: &'a HashSet<String>,
    /// Disable inline (Unicode) math, rendering as images instead.
    pub no_inline_math: bool,
}

impl Default for DiagramRenderOpts<'_> {
    fn default() -> Self {
        Self {
            mermaid_as_images: false,
            failed_mermaid_srcs: &EMPTY_HASH_SET,
            math_as_images: false,
            failed_math_srcs: &EMPTY_HASH_SET,
            no_inline_math: false,
        }
    }
}

static EMPTY_HASH_SET: std::sync::LazyLock<HashSet<String>> =
    std::sync::LazyLock::new(HashSet::new);

/// Parse markdown source into a Document.
///
/// # Example
///
/// ```
/// use markless::document::Document;
///
/// let doc = Document::parse("# Hello\n\nWorld").unwrap();
/// assert!(doc.line_count() >= 3); // heading + empty + paragraph + trailing empty
/// ```
impl Document {
    /// # Errors
    /// Returns an error if markdown parsing fails.
    pub fn parse(source: &str) -> Result<Self> {
        parse(source)
    }

    /// # Errors
    /// Returns an error if markdown parsing fails.
    pub fn parse_with_layout(source: &str, width: u16) -> Result<Self> {
        parse_with_layout(source, width, &HashMap::new())
    }

    /// Parse with separate widths for prose wrapping and the document's
    /// maximum width.
    ///
    /// `wrap_width` controls where prose is word-wrapped. `max_width` is the
    /// usable terminal area and is used as the upper bound for elements that
    /// don't word-wrap (code blocks, tables). This split lets users set a
    /// narrow `--wrap-width` for prose without truncating code blocks below
    /// the terminal width.
    ///
    /// # Errors
    /// Returns an error if markdown parsing fails.
    pub fn parse_with_widths(source: &str, wrap_width: u16, max_width: u16) -> Result<Self> {
        Ok(parse_with_all_options_internal(
            source,
            wrap_width,
            max_width,
            &HashMap::new(),
            &DiagramRenderOpts::default(),
        ))
    }

    /// # Errors
    /// Returns an error if markdown parsing fails.
    pub fn parse_with_image_heights(
        source: &str,
        image_heights: &HashMap<String, usize>,
    ) -> Result<Self> {
        parse_with_image_heights(source, image_heights)
    }

    /// # Errors
    /// Returns an error if markdown parsing fails.
    pub fn parse_with_layout_and_image_heights(
        source: &str,
        width: u16,
        image_heights: &HashMap<String, usize>,
    ) -> Result<Self> {
        parse_with_layout(source, width, image_heights)
    }

    /// Parse markdown, rendering mermaid/math code blocks as image placeholders.
    ///
    /// # Errors
    /// Returns an error if markdown parsing fails.
    pub fn parse_with_mermaid_images(source: &str, width: u16) -> Result<Self> {
        Self::parse_with_all_options_and_failures(
            source,
            width,
            &HashMap::new(),
            &DiagramRenderOpts {
                mermaid_as_images: true,
                math_as_images: true,
                ..DiagramRenderOpts::default()
            },
        )
    }

    /// Parse with all options: layout width, image heights, and mermaid-as-images flag.
    ///
    /// # Errors
    /// Returns an error if markdown parsing fails.
    pub fn parse_with_all_options(
        source: &str,
        width: u16,
        image_heights: &HashMap<String, usize>,
        mermaid_as_images: bool,
    ) -> Result<Self> {
        Self::parse_with_all_options_and_failures(
            source,
            width,
            image_heights,
            &DiagramRenderOpts {
                mermaid_as_images,
                math_as_images: mermaid_as_images, // mirrors mermaid
                ..DiagramRenderOpts::default()
            },
        )
    }

    /// Parse with all options plus sets of sources that failed to render.
    ///
    /// Sources in `failed_mermaid_srcs` / `failed_math_srcs` will be rendered
    /// as code/text blocks even when the as-images flag is true.
    ///
    /// # Errors
    /// Returns an error if markdown parsing fails.
    pub fn parse_with_all_options_and_failures(
        source: &str,
        width: u16,
        image_heights: &HashMap<String, usize>,
        diagram_opts: &DiagramRenderOpts<'_>,
    ) -> Result<Self> {
        Ok(parse_with_all_options(
            source,
            width,
            image_heights,
            diagram_opts,
        ))
    }

    /// Like [`parse_with_all_options_and_failures`] but with separate widths
    /// for prose wrapping and the document maximum width.
    ///
    /// `wrap_width` controls where prose word-wraps. `max_width` is the upper
    /// bound for elements that don't word-wrap (code blocks, tables). When
    /// `--wrap-width` is set, callers should pass it as `wrap_width` and the
    /// full terminal area as `max_width` so code blocks aren't clipped below
    /// what the terminal could show.
    ///
    /// # Errors
    /// Returns an error if markdown parsing fails.
    pub fn parse_with_all_options_widths(
        source: &str,
        wrap_width: u16,
        max_width: u16,
        image_heights: &HashMap<String, usize>,
        diagram_opts: &DiagramRenderOpts<'_>,
    ) -> Result<Self> {
        Ok(parse_with_all_options_internal(
            source,
            wrap_width,
            max_width,
            image_heights,
            diagram_opts,
        ))
    }
}

/// Parse markdown source into a `Document`.
///
/// # Errors
/// Returns an error if markdown parsing fails.
pub fn parse(source: &str) -> Result<Document> {
    parse_with_layout(source, 80, &HashMap::new())
}

/// Parse markdown with known image heights (in terminal rows).
///
/// # Errors
/// Returns an error if markdown parsing fails.
pub fn parse_with_image_heights<S: BuildHasher>(
    source: &str,
    image_heights: &HashMap<String, usize, S>,
) -> Result<Document> {
    parse_with_layout(source, 80, image_heights)
}

/// Parse markdown with layout width and image heights.
///
/// # Errors
/// Returns an error if markdown parsing fails.
pub fn parse_with_layout<S: BuildHasher>(
    source: &str,
    width: u16,
    image_heights: &HashMap<String, usize, S>,
) -> Result<Document> {
    Ok(parse_with_all_options_internal(
        source,
        width,
        width,
        image_heights,
        &DiagramRenderOpts::default(),
    ))
}

/// Parse markdown with all options including mermaid/math-as-images flags.
fn parse_with_all_options(
    source: &str,
    width: u16,
    image_heights: &HashMap<String, usize>,
    opts: &DiagramRenderOpts<'_>,
) -> Document {
    parse_with_all_options_internal(source, width, width, image_heights, opts)
}

/// Internal parse implementation that accepts any `BuildHasher` for image heights.
///
/// `wrap_width` controls where prose is word-wrapped. `max_width` is the
/// upper bound for elements that don't word-wrap (code blocks, tables);
/// it usually equals the available terminal width.
fn parse_with_all_options_internal<S: BuildHasher>(
    source: &str,
    wrap_width: u16,
    max_width: u16,
    image_heights: &HashMap<String, usize, S>,
    opts: &DiagramRenderOpts<'_>,
) -> Document {
    let arena = Arena::new();
    let options = create_options();
    let root = parse_document(&arena, source, &options);

    let wrap_width = wrap_width.max(1) as usize;
    let block_max_width = (max_width.max(1) as usize).max(wrap_width);
    let mut ctx = ParseContext {
        lines: Vec::new(),
        headings: Vec::new(),
        images: Vec::new(),
        link_refs: Vec::new(),
        footnotes: HashMap::new(),
        code_blocks: Vec::new(),
        mermaid_sources: HashMap::new(),
        math_sources: HashMap::new(),
        image_heights,
        wrap_width,
        block_max_width,
        mermaid_as_images: opts.mermaid_as_images,
        failed_mermaid_srcs: opts.failed_mermaid_srcs,
        math_as_images: opts.math_as_images,
        failed_math_srcs: opts.failed_math_srcs,
        no_inline_math: opts.no_inline_math,
    };
    process_node(root, &mut ctx, 0, None);

    Document::from_parsed(
        source.to_string(),
        ParsedDocument {
            lines: ctx.lines,
            headings: ctx.headings,
            images: ctx.images,
            links: ctx.link_refs,
            footnotes: ctx.footnotes,
            code_blocks: ctx.code_blocks,
            mermaid_sources: ctx.mermaid_sources,
            math_sources: ctx.math_sources,
        },
    )
}

fn create_options() -> Options {
    let mut options = Options::default();

    // Enable GFM extensions
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options.extension.superscript = true;
    options.extension.subscript = true;

    // Enable other useful extensions
    options.extension.header_ids = Some(String::new());
    options.extension.description_lists = true;

    // Enable LaTeX math ($...$, $$...$$, and $`...`$)
    options.extension.math_dollars = true;
    options.extension.math_code = true;

    // Parse YAML front matter (---\n...\n---) as a FrontMatter node instead of
    // a thematic break followed by paragraph text. Issue #51.
    options.extension.front_matter_delimiter = Some("---".to_string());

    options
}

/// Mutable context threaded through recursive node processing.
struct ParseContext<'h, S: BuildHasher = std::collections::hash_map::RandomState> {
    lines: Vec<RenderedLine>,
    headings: Vec<HeadingRef>,
    images: Vec<ImageRef>,
    link_refs: Vec<LinkRef>,
    footnotes: HashMap<String, usize>,
    code_blocks: Vec<CodeBlockRef>,
    mermaid_sources: HashMap<String, String>,
    math_sources: HashMap<String, String>,
    image_heights: &'h HashMap<String, usize, S>,
    wrap_width: usize,
    /// Upper bound for non-prose blocks (code blocks, tables): the full document area, ignoring --wrap-width.
    block_max_width: usize,
    mermaid_as_images: bool,
    failed_mermaid_srcs: &'h HashSet<String>,
    math_as_images: bool,
    failed_math_srcs: &'h HashSet<String>,
    no_inline_math: bool,
}

fn process_node<'a, S: BuildHasher>(
    node: &'a AstNode<'a>,
    ctx: &mut ParseContext<'_, S>,
    depth: usize,
    list_marker: Option<String>,
) {
    match &node.data.borrow().value {
        NodeValue::Document => {
            for child in node.children() {
                process_node(child, ctx, depth, list_marker.clone());
            }
        }

        NodeValue::Heading(heading) => {
            let text = extract_text(node);

            // Keep headings visually separated with two rows above.
            ensure_trailing_empty_lines(&mut ctx.lines, 2);
            let line_num = ctx.lines.len();

            ctx.headings.push(HeadingRef {
                level: heading.level,
                text: text.clone(),
                line: line_num,
                id: None, // TODO: Extract from header_ids
            });

            collect_inline_elements(node, line_num, &mut ctx.images, &mut ctx.link_refs);

            let prefix = "#".repeat(heading.level as usize);
            ctx.lines.push(RenderedLine::new(
                format!("{prefix} {text}"),
                LineType::Heading(heading.level),
            ));
            ctx.lines
                .push(RenderedLine::new(String::new(), LineType::Empty));
        }

        NodeValue::Paragraph => {
            // Check if paragraph ends with display math ($$...$$).
            // This handles both standalone display math and paragraphs where
            // text precedes the $$ block (e.g. after a hard line break `\`).
            if let Some(literal) = extract_display_math_literal(node) {
                // If the paragraph has leading text before the math,
                // render that text first as a regular paragraph.
                if let Some(spans) = collect_spans_before_display_math(node) {
                    let wrapped = wrap_spans(&spans, ctx.wrap_width, "", "", false);
                    for line_spans in wrapped {
                        let content = spans_to_string(&line_spans);
                        ctx.lines.push(RenderedLine::with_spans(
                            content,
                            LineType::Paragraph,
                            line_spans,
                        ));
                    }
                }

                let source_failed = ctx.failed_math_srcs.contains(&literal);
                if ctx.math_as_images && !source_failed {
                    emit_math_image_placeholder(&literal, ctx);
                } else {
                    emit_math_text_block(&literal, ctx);
                }
                // Store in math_sources even for text fallback
                if !ctx.math_sources.values().any(|v| v == literal.trim_end()) {
                    let key = format!("math://{}", ctx.math_sources.len());
                    ctx.math_sources.insert(key, literal.trim_end().to_string());
                }
                ctx.lines
                    .push(RenderedLine::new(String::new(), LineType::Empty));
                return;
            }

            // Check if paragraph contains only an image (common case)
            let child_images = collect_paragraph_images(node);

            if child_images.is_empty() {
                // When no_inline_math is enabled, split paragraph at
                // inline math boundaries so each $...$ gets its own image.
                if ctx.no_inline_math && ctx.math_as_images && has_inline_math(node) {
                    emit_paragraph_with_no_inline_math(node, ctx, "", "", false);
                } else {
                    // Regular paragraph text with inline styling and wrapping
                    let spans = collect_inline_spans(node);
                    // Collect links with a placeholder line number (will be fixed up after wrapping)
                    let link_start = ctx.link_refs.len();
                    collect_inline_elements(node, 0, &mut ctx.images, &mut ctx.link_refs);

                    let base_line = ctx.lines.len();
                    let wrapped = wrap_spans(&spans, ctx.wrap_width, "", "", false);
                    for line_spans in wrapped {
                        let content = spans_to_string(&line_spans);
                        ctx.lines.push(RenderedLine::with_spans(
                            content,
                            LineType::Paragraph,
                            line_spans,
                        ));
                    }

                    // Fix up link line numbers: find which wrapped line contains each link's text
                    let extra = fixup_link_lines(
                        &mut ctx.link_refs[link_start..],
                        &ctx.lines[base_line..],
                        base_line,
                        0,
                    );
                    ctx.link_refs.extend(extra);
                }
            } else {
                for (alt, src) in child_images {
                    let height_lines = ctx.image_heights.get(&src).copied().unwrap_or(1).max(1);
                    let has_caption = ctx.image_heights.contains_key(&src) && !alt.is_empty();
                    let start_line = ctx.lines.len();
                    let label = format!("[Image: {}]", if alt.is_empty() { &src } else { &alt });

                    if has_caption {
                        ctx.lines
                            .push(RenderedLine::new(format!("    {alt}"), LineType::Image));
                    }

                    // First line shows the image placeholder/alt text
                    ctx.lines
                        .push(RenderedLine::new(label.clone(), LineType::Image));
                    ctx.link_refs.push(LinkRef {
                        text: label,
                        url: src.clone(),
                        line: start_line + usize::from(has_caption),
                    });

                    // Reserve additional lines for image content (empty Image lines)
                    for _ in 1..height_lines {
                        ctx.lines
                            .push(RenderedLine::new(String::new(), LineType::Image));
                    }

                    let end_line = ctx.lines.len();
                    ctx.images.push(ImageRef {
                        alt: alt.clone(),
                        src: src.clone(),
                        line_range: start_line + usize::from(has_caption)..end_line,
                    });
                }
            }
            ctx.lines
                .push(RenderedLine::new(String::new(), LineType::Empty));
        }

        NodeValue::CodeBlock(code_block) => {
            let info = code_block.info.clone();
            let literal = code_block.literal.clone();
            let language = info.split_whitespace().next().filter(|s| !s.is_empty());

            // Store mermaid diagram sources for optional image rendering.
            if language == Some("mermaid") {
                let trimmed_source = literal.trim_end().to_string();
                let key = format!("mermaid://{}", ctx.mermaid_sources.len());
                ctx.mermaid_sources
                    .insert(key.clone(), trimmed_source.clone());

                let source_failed = ctx.failed_mermaid_srcs.contains(&trimmed_source);
                if ctx.mermaid_as_images && !source_failed {
                    // Emit as an image placeholder instead of a code block.
                    let height_lines = ctx.image_heights.get(&key).copied().unwrap_or(1).max(1);
                    let has_caption = ctx.image_heights.contains_key(&key);
                    let start_line = ctx.lines.len();
                    let label = "[Image: mermaid diagram]".to_string();

                    if has_caption {
                        ctx.lines.push(RenderedLine::new(
                            "    mermaid diagram".to_string(),
                            LineType::Image,
                        ));
                    }

                    ctx.lines
                        .push(RenderedLine::new(label.clone(), LineType::Image));
                    ctx.link_refs.push(LinkRef {
                        text: label,
                        url: key.clone(),
                        line: start_line + usize::from(has_caption),
                    });

                    for _ in 1..height_lines {
                        ctx.lines
                            .push(RenderedLine::new(String::new(), LineType::Image));
                    }

                    let end_line = ctx.lines.len();
                    ctx.images.push(ImageRef {
                        alt: "mermaid diagram".to_string(),
                        src: key,
                        line_range: start_line + usize::from(has_caption)..end_line,
                    });
                    ctx.lines
                        .push(RenderedLine::new(String::new(), LineType::Empty));
                    // Skip the normal code block rendering below.
                    return;
                }
            }

            // Render ```math code blocks as math (image or text)
            if language == Some("math") {
                let trimmed_source = literal.trim_end().to_string();
                let source_failed = ctx.failed_math_srcs.contains(&trimmed_source);
                if ctx.math_as_images && !source_failed {
                    emit_math_image_placeholder(&trimmed_source, ctx);
                } else {
                    emit_math_text_block(&trimmed_source, ctx);
                }
                // Store in math_sources even for text fallback
                if !ctx.math_sources.values().any(|v| v == &trimmed_source) {
                    let key = format!("math://{}", ctx.math_sources.len());
                    ctx.math_sources.insert(key, trimmed_source);
                }
                ctx.lines
                    .push(RenderedLine::new(String::new(), LineType::Empty));
                return;
            }

            // Render CSV code blocks as tables instead of code blocks
            if language == Some("csv") {
                let csv_lines = render_csv_as_table(&literal, ctx.block_max_width);
                if !csv_lines.is_empty() {
                    ctx.lines.extend(csv_lines);
                    ctx.lines
                        .push(RenderedLine::new(String::new(), LineType::Empty));
                    return;
                }
                // Fall through to normal code block rendering if CSV parsing fails
            }
            emit_code_block(ctx, &literal, language);
        }

        NodeValue::FrontMatter(literal) => {
            // Strip the `---` delimiters and trailing blank line that comrak
            // includes in the FrontMatter literal so only the YAML body
            // appears inside the code frame.
            let body = strip_front_matter_delimiters(literal);
            emit_code_block(ctx, body, Some("yaml"));
        }

        NodeValue::List(list) => {
            let list_depth = depth + 1;
            let start = list.start;
            let delimiter = match list.delimiter {
                comrak::nodes::ListDelimType::Paren => ')',
                comrak::nodes::ListDelimType::Period => '.',
            };
            let list_len = node.children().count();
            let max_number = start + list_len.saturating_sub(1);
            let number_width = max_number.to_string().len();

            for (index, child) in node.children().enumerate() {
                let base_marker = match list.list_type {
                    comrak::nodes::ListType::Bullet => "•".to_string(),
                    comrak::nodes::ListType::Ordered => {
                        let number = start + index;
                        format!("{number:>number_width$}{delimiter}")
                    }
                };
                let marker = format!("{base_marker} ");
                process_node(child, ctx, list_depth, Some(marker));
            }
            ctx.lines
                .push(RenderedLine::new(String::new(), LineType::Empty));
        }

        NodeValue::TaskItem(symbol) => {
            let indent = "  ".repeat(depth.saturating_sub(1));
            let task_marker = if symbol.is_some() { "✓" } else { "□" };
            let marker = format!("{task_marker} ");
            let prefix_first = format!("{indent}{marker}");
            let prefix_next = format!("{}{}", indent, " ".repeat(marker.len()));

            let spans = collect_inline_spans(node);
            let link_start = ctx.link_refs.len();
            collect_inline_elements(node, 0, &mut ctx.images, &mut ctx.link_refs);
            let base_line = ctx.lines.len();
            let wrapped = wrap_spans(&spans, ctx.wrap_width, &prefix_first, &prefix_next, true);
            for line_spans in wrapped {
                let content = spans_to_string(&line_spans);
                ctx.lines.push(RenderedLine::with_spans(
                    content,
                    LineType::ListItem(depth),
                    line_spans,
                ));
            }
            let extra = fixup_link_lines(
                &mut ctx.link_refs[link_start..],
                &ctx.lines[base_line..],
                base_line,
                prefix_next.len(),
            );
            ctx.link_refs.extend(extra);

            for child in node.children() {
                if matches!(child.data.borrow().value, NodeValue::List(_)) {
                    process_node(child, ctx, depth, None);
                }
            }
        }

        NodeValue::Item(_) => {
            let indent = "  ".repeat(depth.saturating_sub(1));
            let base_marker = list_marker.unwrap_or_else(|| "- ".to_string());
            let task_marker = find_task_marker(node);
            let marker = task_marker.map_or(base_marker, |tm| format!("{tm} "));
            let prefix_first = format!("{indent}{marker}");
            let prefix_next = format!("{}{}", indent, " ".repeat(marker.len()));
            let mut rendered_any = false;
            let mut rendered_paragraphs = 0usize;

            for child in node.children() {
                match &child.data.borrow().value {
                    NodeValue::Paragraph | NodeValue::TaskItem(_) => {
                        // Check for display math ($$...$$) inside list paragraphs
                        if let Some(literal) = extract_display_math_literal(child) {
                            // Render any text before the math as a list item line
                            if let Some(spans) = collect_spans_before_display_math(child) {
                                let prefix = if rendered_any {
                                    &prefix_next
                                } else {
                                    &prefix_first
                                };
                                let wrapped = wrap_spans(
                                    &spans,
                                    ctx.wrap_width,
                                    prefix,
                                    &prefix_next,
                                    !rendered_any,
                                );
                                for line_spans in wrapped {
                                    let content = spans_to_string(&line_spans);
                                    ctx.lines.push(RenderedLine::with_spans(
                                        content,
                                        LineType::ListItem(depth),
                                        line_spans,
                                    ));
                                }
                            }
                            // Emit the display math block
                            let source_failed = ctx.failed_math_srcs.contains(&literal);
                            if ctx.math_as_images && !source_failed {
                                emit_math_image_placeholder(&literal, ctx);
                            } else {
                                emit_math_text_block(&literal, ctx);
                            }
                            if !ctx.math_sources.values().any(|v| v == literal.trim_end()) {
                                let key = format!("math://{}", ctx.math_sources.len());
                                ctx.math_sources.insert(key, literal.trim_end().to_string());
                            }
                            ctx.lines
                                .push(RenderedLine::new(String::new(), LineType::Empty));
                            rendered_any = true;
                            rendered_paragraphs += 1;
                            continue;
                        }

                        if rendered_paragraphs > 0 {
                            ctx.lines
                                .push(RenderedLine::new(String::new(), LineType::ListItem(depth)));
                        }
                        let prefix = if rendered_any {
                            &prefix_next
                        } else {
                            &prefix_first
                        };
                        if ctx.no_inline_math && ctx.math_as_images && has_inline_math(child) {
                            emit_paragraph_with_no_inline_math(
                                child,
                                ctx,
                                prefix,
                                &prefix_next,
                                !rendered_any,
                            );
                        } else {
                            let spans = collect_inline_spans(child);
                            let link_start = ctx.link_refs.len();
                            collect_inline_elements(child, 0, &mut ctx.images, &mut ctx.link_refs);
                            let base_line = ctx.lines.len();
                            let wrapped = wrap_spans(
                                &spans,
                                ctx.wrap_width,
                                prefix,
                                &prefix_next,
                                !rendered_any,
                            );

                            for line_spans in wrapped {
                                let content = spans_to_string(&line_spans);
                                ctx.lines.push(RenderedLine::with_spans(
                                    content,
                                    LineType::ListItem(depth),
                                    line_spans,
                                ));
                            }
                            let extra = fixup_link_lines(
                                &mut ctx.link_refs[link_start..],
                                &ctx.lines[base_line..],
                                base_line,
                                prefix_next.len(),
                            );
                            ctx.link_refs.extend(extra);
                        }
                        rendered_any = true;
                        rendered_paragraphs += 1;
                    }
                    _ => {
                        process_node(child, ctx, depth, None);
                    }
                }
            }

            if !rendered_any {
                let spans = collect_inline_spans(node);
                let link_start = ctx.link_refs.len();
                collect_inline_elements(node, 0, &mut ctx.images, &mut ctx.link_refs);
                let base_line = ctx.lines.len();
                let wrapped = wrap_spans(&spans, ctx.wrap_width, &prefix_first, &prefix_next, true);
                for line_spans in wrapped {
                    let content = spans_to_string(&line_spans);
                    ctx.lines.push(RenderedLine::with_spans(
                        content,
                        LineType::ListItem(depth),
                        line_spans,
                    ));
                }
                let extra = fixup_link_lines(
                    &mut ctx.link_refs[link_start..],
                    &ctx.lines[base_line..],
                    base_line,
                    prefix_next.len(),
                );
                ctx.link_refs.extend(extra);
            }
        }

        NodeValue::BlockQuote => {
            render_blockquote(
                node,
                &mut ctx.lines,
                &mut ctx.link_refs,
                &mut ctx.images,
                ctx.wrap_width,
                1,
            );
            ctx.lines
                .push(RenderedLine::new(String::new(), LineType::Empty));
        }

        NodeValue::ThematicBreak => {
            ctx.lines.push(RenderedLine::new(
                "─────".to_string(),
                LineType::HorizontalRule,
            ));
            ctx.lines
                .push(RenderedLine::new(String::new(), LineType::Empty));
        }

        NodeValue::Table(_) => {
            let base_line = ctx.lines.len();
            let link_start = ctx.link_refs.len();
            let table_lines = render_table(
                node,
                &mut ctx.link_refs,
                &mut ctx.images,
                ctx.block_max_width,
            );
            ctx.lines.extend(table_lines);
            // Fix up link lines relative to where the table was placed
            for link in &mut ctx.link_refs[link_start..] {
                link.line += base_line;
            }
            ctx.lines
                .push(RenderedLine::new(String::new(), LineType::Empty));
        }

        NodeValue::FootnoteDefinition(def) => {
            let line_num = ctx.lines.len();
            ctx.footnotes.insert(def.name.clone(), line_num);
            let label = format!("{} ", render_footnote_reference(&def.name));
            let continuation = " ".repeat(label.len());
            let spans = collect_inline_spans(node);
            let link_start = ctx.link_refs.len();
            collect_inline_elements(node, 0, &mut ctx.images, &mut ctx.link_refs);
            let base_line = ctx.lines.len();
            let wrapped = wrap_spans(&spans, ctx.wrap_width, &label, &continuation, false);
            if wrapped.is_empty() {
                ctx.lines
                    .push(RenderedLine::new(label, LineType::Paragraph));
            } else {
                for line_spans in wrapped {
                    let content = spans_to_string(&line_spans);
                    ctx.lines.push(RenderedLine::with_spans(
                        content,
                        LineType::Paragraph,
                        line_spans,
                    ));
                }
            }
            let extra = fixup_link_lines(
                &mut ctx.link_refs[link_start..],
                &ctx.lines[base_line..],
                base_line,
                continuation.len(),
            );
            ctx.link_refs.extend(extra);
            ctx.lines
                .push(RenderedLine::new(String::new(), LineType::Empty));
        }

        NodeValue::Image(image) => {
            let alt = extract_text(node);
            let src = image.url.clone();
            let line_num = ctx.lines.len();
            let label = format!("[Image: {}]", if alt.is_empty() { &src } else { &alt });
            let height_lines = ctx.image_heights.get(&src).copied().unwrap_or(1).max(1);
            let has_caption = ctx.image_heights.contains_key(&src) && !alt.is_empty();

            ctx.images.push(ImageRef {
                alt: alt.clone(),
                src: src.clone(),
                line_range: line_num + usize::from(has_caption)
                    ..line_num + usize::from(has_caption) + height_lines,
            });

            ctx.link_refs.push(LinkRef {
                text: label.clone(),
                url: src,
                line: line_num + usize::from(has_caption),
            });

            if has_caption {
                ctx.lines
                    .push(RenderedLine::new(format!("    {alt}"), LineType::Image));
            }
            ctx.lines.push(RenderedLine::new(label, LineType::Image));

            for _ in 1..height_lines {
                ctx.lines
                    .push(RenderedLine::new(String::new(), LineType::Image));
            }
        }

        NodeValue::HtmlBlock(html_block) => {
            for (src, alt) in extract_html_images(&html_block.literal) {
                emit_html_image(ctx, &src, &alt);
            }
        }

        NodeValue::HtmlInline(html) => {
            for (src, alt) in extract_html_images(html) {
                emit_html_image(ctx, &src, &alt);
            }
            // Also process children (nested inline HTML may wrap content)
            for child in node.children() {
                process_node(child, ctx, depth, list_marker.clone());
            }
        }

        _ => {
            // Process children for unhandled nodes
            for child in node.children() {
                process_node(child, ctx, depth, list_marker.clone());
            }
        }
    }
}

/// Extract a display math literal from a paragraph containing `$$...$$`.
///
/// Returns `Some(literal)` if the paragraph's **last** child (ignoring
/// trailing soft/line breaks) is a display-math node.  This handles both
/// the simple case (`$$math$$` alone) and paragraphs where text precedes
/// the display block (e.g. after a hard line break `\`).
/// Collect inline spans from children that precede a display math block.
///
/// Returns `None` if there is no non-whitespace text before the display math.
fn collect_spans_before_display_math<'a>(node: &'a AstNode<'a>) -> Option<Vec<InlineSpan>> {
    let has_leading_text = node.children().any(|c| {
        let d = c.data.borrow();
        !matches!(d.value, NodeValue::Math(ref m) if m.display_math)
            && !matches!(d.value, NodeValue::SoftBreak | NodeValue::LineBreak)
    });
    if !has_leading_text {
        return None;
    }
    let mut spans = Vec::new();
    for child in node.children() {
        let d = child.data.borrow();
        if matches!(&d.value, NodeValue::Math(m) if m.display_math) {
            break;
        }
        drop(d);
        collect_inline_spans_recursive(child, InlineStyle::default(), &mut spans);
    }
    // Drop trailing whitespace spans
    while spans.last().is_some_and(|s| s.text().trim().is_empty()) {
        spans.pop();
    }
    if spans.is_empty() { None } else { Some(spans) }
}

fn extract_display_math_literal<'a>(node: &'a AstNode<'a>) -> Option<String> {
    // Collect children so we can iterate in reverse.
    let children: Vec<_> = node.children().collect();
    for child in children.iter().rev() {
        let data = child.data.borrow();
        match &data.value {
            NodeValue::Math(math) if math.display_math => {
                return Some(math.literal.clone());
            }
            NodeValue::SoftBreak | NodeValue::LineBreak => {}
            _ => return None,
        }
    }
    None
}

/// Emit a framed code block with the given literal text and language label.
///
/// Shared between fenced code blocks and YAML front matter. The frame has a
/// `language` label in the top border, `│ … │` body lines, and lazy syntax
/// highlighting is registered against `ctx.code_blocks`.
fn emit_code_block<S: BuildHasher>(
    ctx: &mut ParseContext<'_, S>,
    literal: &str,
    language: Option<&str>,
) {
    const CODE_RIGHT_PADDING: usize = 3;

    // When the longest line doesn't fit, clip to `max_width - 2` so visible
    // content extends to the edge of the screen. The 5 trailing frame chars
    // (3 padding + space + `│`) get clipped by the terminal — the missing
    // right `│` is the visual cue that the line was truncated. Reserving
    // more room (e.g. `- 4`) just leaves dead whitespace before the clipped
    // border.
    let content_width = literal
        .lines()
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0)
        .min(ctx.block_max_width.saturating_sub(2).max(1));
    let title = language.unwrap_or("code");
    let label = format!(" {title} ");
    let frame_inner_width = content_width + 2 + CODE_RIGHT_PADDING;
    let top_label_width = frame_inner_width.min(UnicodeWidthStr::width(label.as_str()));
    let visible_label: String = label.chars().take(top_label_width).collect();
    let top = format!(
        "┌{}{}┐",
        visible_label,
        "─".repeat(
            frame_inner_width.saturating_sub(UnicodeWidthStr::width(visible_label.as_str()))
        )
    );
    let frame_style = InlineStyle {
        code_frame: true,
        ..InlineStyle::default()
    };
    let top_spans = vec![InlineSpan::new(top.clone(), frame_style)];
    ctx.lines.push(RenderedLine::with_spans(
        top,
        LineType::CodeBlock,
        top_spans,
    ));

    let body_start = ctx.lines.len();
    let raw_lines: Vec<String> = literal.lines().map(ToString::to_string).collect();
    for raw_line in &raw_lines {
        let plain_style = InlineStyle {
            code: true,
            code_block: true,
            ..InlineStyle::default()
        };
        let spans = vec![InlineSpan::new(raw_line.clone(), plain_style)];
        let trimmed_spans = truncate_spans(&spans, content_width);
        let trimmed_len = UnicodeWidthStr::width(spans_to_string(&trimmed_spans).as_str());
        let padding = " ".repeat(content_width.saturating_sub(trimmed_len) + CODE_RIGHT_PADDING);

        let mut line_spans = Vec::new();
        line_spans.push(InlineSpan::new("│ ".to_string(), frame_style));
        line_spans.extend(trimmed_spans);
        line_spans.push(InlineSpan::new(format!("{padding} │"), frame_style));
        let content = spans_to_string(&line_spans);
        ctx.lines.push(RenderedLine::with_spans(
            content,
            LineType::CodeBlock,
            line_spans,
        ));
    }
    let body_end = ctx.lines.len();

    ctx.code_blocks.push(CodeBlockRef {
        line_range: body_start..body_end,
        language: language.map(ToString::to_string),
        raw_lines,
        highlighted: false,
        content_width,
        right_padding: CODE_RIGHT_PADDING,
    });

    let bottom = format!("└{}┘", "─".repeat(frame_inner_width));
    let bottom_spans = vec![InlineSpan::new(bottom.clone(), frame_style)];
    ctx.lines.push(RenderedLine::with_spans(
        bottom,
        LineType::CodeBlock,
        bottom_spans,
    ));
    ctx.lines
        .push(RenderedLine::new(String::new(), LineType::Empty));
}

/// Strip the leading and trailing `---` delimiter lines (and trailing blank
/// line) that comrak includes in the front-matter literal, leaving only the
/// YAML body.
fn strip_front_matter_delimiters(literal: &str) -> &str {
    let after_first = literal
        .strip_prefix("---\r\n")
        .or_else(|| literal.strip_prefix("---\n"))
        .unwrap_or(literal);
    // Find the closing `\n---\r?\n` and take everything before it.
    let closing = after_first
        .rfind("\n---\r\n")
        .map(|i| (i, "\n---\r\n".len()))
        .or_else(|| after_first.rfind("\n---\n").map(|i| (i, "\n---\n".len())))
        .or_else(|| after_first.rfind("\n---").map(|i| (i, "\n---".len())));
    let body = match closing {
        Some((idx, _)) => &after_first[..idx],
        None => after_first,
    };
    body.trim_end_matches(['\n', '\r'])
}

/// Emit a display math block as a framed text block with Unicode approximation.
fn emit_math_text_block<S: BuildHasher>(literal: &str, ctx: &mut ParseContext<'_, S>) {
    let unicode = crate::math::latex_to_unicode(literal);
    let lines: Vec<&str> = unicode.lines().collect();
    let max_width = lines
        .iter()
        .map(|l| unicode_width::UnicodeWidthStr::width(*l))
        .max()
        .unwrap_or(0);
    let frame_width = max_width + 2; // 1 space padding on each side
    let label = " math ";
    let label_width = unicode_width::UnicodeWidthStr::width(label).min(frame_width);
    let visible_label: String = label.chars().take(label_width).collect();
    let top = format!(
        "┌{}{}┐",
        visible_label,
        "─".repeat(
            frame_width.saturating_sub(unicode_width::UnicodeWidthStr::width(
                visible_label.as_str()
            ))
        )
    );
    ctx.lines.push(RenderedLine::new(top, LineType::Math));

    for line in &lines {
        let line_width = unicode_width::UnicodeWidthStr::width(*line);
        let padding = " ".repeat(max_width.saturating_sub(line_width));
        ctx.lines.push(RenderedLine::new(
            format!("│ {line}{padding} │"),
            LineType::Math,
        ));
    }

    ctx.lines.push(RenderedLine::new(
        format!("└{}┘", "─".repeat(frame_width)),
        LineType::Math,
    ));
}

/// Emit a display math block as an image placeholder (when images are supported).
fn emit_math_image_placeholder<S: BuildHasher>(literal: &str, ctx: &mut ParseContext<'_, S>) {
    let key = format!("math://{}", ctx.math_sources.len());
    ctx.math_sources
        .insert(key.clone(), literal.trim_end().to_string());

    let height_lines = ctx.image_heights.get(&key).copied().unwrap_or(1).max(1);
    let start_line = ctx.lines.len();
    let label = "[Image: math equation]".to_string();

    // No caption for math — "math equation" adds no useful info.
    ctx.lines
        .push(RenderedLine::new(label.clone(), LineType::Image));
    ctx.link_refs.push(LinkRef {
        text: label,
        url: key.clone(),
        line: start_line,
    });

    for _ in 1..height_lines {
        ctx.lines
            .push(RenderedLine::new(String::new(), LineType::Image));
    }

    let end_line = ctx.lines.len();
    ctx.images.push(ImageRef {
        alt: String::new(),
        src: key,
        line_range: start_line..end_line,
    });
}

/// Check if a paragraph node contains any non-display inline math ($...$).
fn has_inline_math<'a>(node: &'a AstNode<'a>) -> bool {
    for child in node.children() {
        let data = child.data.borrow();
        if matches!(&data.value, NodeValue::Math(m) if !m.display_math) {
            return true;
        }
    }
    false
}

/// Emit a paragraph, splitting at inline math boundaries so each `$...$`
/// becomes its own image placeholder line.
fn emit_paragraph_with_no_inline_math<'a, S: BuildHasher>(
    node: &'a AstNode<'a>,
    ctx: &mut ParseContext<'_, S>,
    prefix_first: &str,
    prefix_next: &str,
    first_prefix_is_list_marker: bool,
) {
    let line_type = if prefix_first.is_empty() {
        LineType::Paragraph
    } else {
        // Infer depth from prefix indentation
        let indent_chars = prefix_first.len() - prefix_first.trim_start().len();
        let depth = (indent_chars / 2) + 1;
        LineType::ListItem(depth)
    };

    // Collect links up-front so hyperlinks remain clickable even when
    // the paragraph is split at inline-math boundaries.
    let link_start = ctx.link_refs.len();
    collect_inline_elements(node, 0, &mut ctx.images, &mut ctx.link_refs);
    let base_line = ctx.lines.len();

    let mut pending_spans: Vec<InlineSpan> = Vec::new();
    let mut is_first_chunk = true;

    for child in node.children() {
        let data = child.data.borrow();
        let math_literal = match &data.value {
            NodeValue::Math(m) if !m.display_math => Some(m.literal.clone()),
            _ => None,
        };
        drop(data);

        if let Some(literal) = math_literal {
            // Flush accumulated text spans as paragraph/list-item lines
            flush_inline_spans(
                &pending_spans,
                ctx,
                line_type,
                is_first_chunk,
                prefix_first,
                prefix_next,
                first_prefix_is_list_marker,
            );
            pending_spans.clear();
            is_first_chunk = false;

            // Emit this inline math as an image placeholder
            if ctx.failed_math_srcs.contains(&literal) {
                // Fallback: render as inline Unicode text
                let text = crate::math::latex_to_unicode(&literal);
                let math_style = InlineStyle {
                    math: true,
                    ..InlineStyle::default()
                };
                pending_spans.push(InlineSpan::new(text, math_style));
            } else {
                emit_math_image_placeholder(&literal, ctx);
            }
        } else {
            // Accumulate non-math spans
            collect_inline_spans_recursive(child, InlineStyle::default(), &mut pending_spans);
        }
    }

    // Flush remaining text spans
    flush_inline_spans(
        &pending_spans,
        ctx,
        line_type,
        is_first_chunk,
        prefix_first,
        prefix_next,
        first_prefix_is_list_marker,
    );

    // Fix up link line numbers to match rendered output lines.
    let prefix_len = prefix_next.len();
    let extra = fixup_link_lines(
        &mut ctx.link_refs[link_start..],
        &ctx.lines[base_line..],
        base_line,
        prefix_len,
    );
    ctx.link_refs.extend(extra);
}

/// Flush accumulated inline spans as wrapped paragraph or list-item lines.
fn flush_inline_spans<S: BuildHasher>(
    spans: &[InlineSpan],
    ctx: &mut ParseContext<'_, S>,
    line_type: LineType,
    is_first: bool,
    prefix_first: &str,
    prefix_next: &str,
    first_prefix_is_list_marker: bool,
) {
    if spans.is_empty() {
        return;
    }
    // Drop leading/trailing whitespace-only spans
    let start = spans
        .iter()
        .position(|s| !s.text().trim().is_empty())
        .unwrap_or(spans.len());
    let end = spans
        .iter()
        .rposition(|s| !s.text().trim().is_empty())
        .map_or(0, |i| i + 1);
    let trimmed = &spans[start..end];
    if trimmed.is_empty() {
        return;
    }
    let prefix = if is_first { prefix_first } else { prefix_next };
    let wrapped = wrap_spans(
        trimmed,
        ctx.wrap_width,
        prefix,
        prefix_next,
        is_first && first_prefix_is_list_marker,
    );
    for line_spans in wrapped {
        let content = spans_to_string(&line_spans);
        ctx.lines
            .push(RenderedLine::with_spans(content, line_type, line_spans));
    }
}

fn ensure_trailing_empty_lines(lines: &mut Vec<RenderedLine>, count: usize) {
    let existing = lines
        .iter()
        .rev()
        .take_while(|line| matches!(line.line_type(), LineType::Empty))
        .count();
    for _ in existing..count {
        lines.push(RenderedLine::new(String::new(), LineType::Empty));
    }
}

fn render_blockquote<'a>(
    node: &'a AstNode<'a>,
    lines: &mut Vec<RenderedLine>,
    link_refs: &mut Vec<LinkRef>,
    images: &mut Vec<ImageRef>,
    wrap_width: usize,
    quote_depth: usize,
) {
    let prefix = quote_prefix(quote_depth);

    for child in node.children() {
        match &child.data.borrow().value {
            NodeValue::Paragraph => {
                let spans = collect_inline_spans(child);
                let link_start = link_refs.len();
                collect_inline_elements(child, 0, images, link_refs);
                let base_line = lines.len();
                let wrapped = wrap_spans(&spans, wrap_width, &prefix, &prefix, false);
                for line_spans in wrapped {
                    let content = spans_to_string(&line_spans);
                    lines.push(RenderedLine::with_spans(
                        content,
                        LineType::BlockQuote,
                        line_spans,
                    ));
                }
                let extra = fixup_link_lines(
                    &mut link_refs[link_start..],
                    &lines[base_line..],
                    base_line,
                    prefix.len(),
                );
                link_refs.extend(extra);
            }
            NodeValue::BlockQuote => {
                render_blockquote(child, lines, link_refs, images, wrap_width, quote_depth + 1);
            }
            _ => {
                let link_start = link_refs.len();
                collect_inline_elements(child, 0, images, link_refs);
                let text = extract_text(child);
                let base_line = lines.len();
                for raw_line in text.lines() {
                    let spans = vec![InlineSpan::new(
                        raw_line.to_string(),
                        InlineStyle::default(),
                    )];
                    let wrapped = wrap_spans(&spans, wrap_width, &prefix, &prefix, false);
                    for line_spans in wrapped {
                        let content = spans_to_string(&line_spans);
                        lines.push(RenderedLine::with_spans(
                            content,
                            LineType::BlockQuote,
                            line_spans,
                        ));
                    }
                }
                let extra = fixup_link_lines(
                    &mut link_refs[link_start..],
                    &lines[base_line..],
                    base_line,
                    prefix.len(),
                );
                link_refs.extend(extra);
            }
        }
    }
}

fn quote_prefix(depth: usize) -> String {
    let mut prefix = String::from("  ");
    for _ in 0..depth {
        prefix.push('│');
        prefix.push(' ');
    }
    prefix
}

#[derive(Debug, Clone)]
struct TableCellRender {
    text: String,
    spans: Vec<InlineSpan>,
}

fn render_table<'a>(
    table_node: &'a AstNode<'a>,
    link_refs: &mut Vec<LinkRef>,
    images: &mut Vec<ImageRef>,
    max_width: usize,
) -> Vec<RenderedLine> {
    let CollectedTableRows {
        alignments,
        rows,
        has_header,
        cell_link_ranges,
    } = collect_table_rows(table_node, link_refs, images);
    let layout = render_table_rows(&rows, &alignments, has_header, max_width);

    for ((row_cells, line_range), row_links) in layout
        .cell_lines
        .iter()
        .zip(&layout.row_line_ranges)
        .zip(cell_link_ranges)
    {
        for (cell_lines, link_range) in row_cells.iter().zip(row_links) {
            let cell_links = &mut link_refs[link_range];
            // Fallback when the link text cannot be located in the wrapped cell.
            for link in cell_links.iter_mut() {
                link.line = line_range.start;
            }
            // Match against undecorated cell text; padding and separators
            // would otherwise interrupt labels wrapped across physical lines.
            let extra = fixup_link_lines(cell_links, cell_lines, line_range.start, 0);
            link_refs.extend(extra);
        }
    }
    layout.lines
}

struct CollectedTableRows {
    alignments: Vec<TableAlignment>,
    rows: Vec<Vec<TableCellRender>>,
    has_header: bool,
    /// For each row and cell, the contiguous range of `link_refs` it owns.
    cell_link_ranges: Vec<Vec<Range<usize>>>,
}

fn collect_table_rows<'a>(
    table_node: &'a AstNode<'a>,
    link_refs: &mut Vec<LinkRef>,
    images: &mut Vec<ImageRef>,
) -> CollectedTableRows {
    let alignments = match &table_node.data.borrow().value {
        NodeValue::Table(table) => table.alignments.clone(),
        _ => Vec::new(),
    };

    let mut rows = Vec::new();
    let mut has_header = false;
    let mut cell_link_ranges = Vec::new();
    for row_node in table_node.children() {
        let is_header_row = matches!(row_node.data.borrow().value, NodeValue::TableRow(true));
        if is_header_row {
            has_header = true;
        }
        if !matches!(row_node.data.borrow().value, NodeValue::TableRow(_)) {
            continue;
        }

        let mut row_cells = Vec::new();
        let mut row_link_ranges = Vec::new();
        for cell_node in row_node.children() {
            if !matches!(cell_node.data.borrow().value, NodeValue::TableCell) {
                continue;
            }
            let spans = normalize_inline_whitespace(&collect_inline_spans(cell_node));
            let text = spans_to_string(&spans);
            // Collect links from this cell; line will be fixed up later
            let link_start = link_refs.len();
            collect_inline_elements(cell_node, 0, images, link_refs);
            row_link_ranges.push(link_start..link_refs.len());
            row_cells.push(TableCellRender { text, spans });
        }
        rows.push(row_cells);
        cell_link_ranges.push(row_link_ranges);
    }

    CollectedTableRows {
        alignments,
        rows,
        has_header,
        cell_link_ranges,
    }
}

struct TableLayout {
    lines: Vec<RenderedLine>,
    /// For each source row, the range of `lines` it occupies (header divider excluded).
    row_line_ranges: Vec<Range<usize>>,
    /// For each source row and cell, its undecorated wrapped lines.
    cell_lines: Vec<Vec<Vec<RenderedLine>>>,
}

/// Lay out table rows within `max_width`, wrapping cells onto multiple
/// physical lines when the natural column widths do not fit. Missing
/// trailing cells and alignments render as empty, left-aligned cells.
fn render_table_rows(
    rows: &[Vec<TableCellRender>],
    alignments: &[TableAlignment],
    has_header: bool,
    max_width: usize,
) -> TableLayout {
    let num_cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if num_cols == 0 {
        return TableLayout {
            lines: Vec::new(),
            row_line_ranges: Vec::new(),
            cell_lines: Vec::new(),
        };
    }

    let mut natural = vec![1_usize; num_cols];
    let mut minimum = vec![1_usize; num_cols];
    for row in rows {
        for (idx, cell) in row.iter().enumerate() {
            natural[idx] = natural[idx].max(display_width(&cell.text));
            minimum[idx] = cell
                .text
                .chars()
                .filter_map(UnicodeWidthChar::width)
                .fold(minimum[idx], usize::max);
        }
    }
    let widths = allocate_column_widths(&natural, &minimum, max_width);

    let mut lines = Vec::new();
    let mut row_line_ranges = Vec::with_capacity(rows.len());
    let mut cell_lines = Vec::with_capacity(rows.len());
    for (row_idx, row) in rows.iter().enumerate() {
        let wrapped: Vec<Vec<Vec<InlineSpan>>> = row
            .iter()
            .zip(&widths)
            .map(|(cell, &width)| wrap_cell_spans(&cell.spans, width))
            .collect();
        let height = wrapped.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let start = lines.len();
        for line_idx in 0..height {
            let cell_lines: Vec<&[InlineSpan]> = wrapped
                .iter()
                .map(|cell| cell.get(line_idx).map_or(&[][..], Vec::as_slice))
                .collect();
            lines.push(render_table_line(&cell_lines, &widths, alignments));
        }
        row_line_ranges.push(start..lines.len());
        cell_lines.push(
            wrapped
                .iter()
                .map(|cell| {
                    cell.iter()
                        .map(|line| RenderedLine::new(spans_to_string(line), LineType::Table))
                        .collect()
                })
                .collect(),
        );
        if has_header && row_idx == 0 {
            lines.push(RenderedLine::new(
                render_table_inner_divider(&widths),
                LineType::Table,
            ));
        }
    }

    TableLayout {
        lines,
        row_line_ranges,
        cell_lines,
    }
}

/// Fit column widths into `max_width` (including the 2-column padding per
/// cell and the 1-column separator between cells). Each column first gets
/// its widest character; spare width then goes to columns below natural
/// width, with small deficits filled first and large ones shared proportionally.
/// If even the minimums do not fit, return them and let the viewport clip.
fn allocate_column_widths(natural: &[usize], minimum: &[usize], max_width: usize) -> Vec<usize> {
    let n = natural.len();
    if n == 0 {
        return Vec::new();
    }
    let overhead = 3 * n - 1;
    if natural.iter().sum::<usize>() + overhead <= max_width {
        return natural.to_vec();
    }
    let mut widths: Vec<usize> = minimum
        .iter()
        .zip(natural)
        .map(|(&min, &nat)| min.max(1).min(nat.max(1)))
        .collect();
    let reserved: usize = widths.iter().sum();
    let Some(mut remaining) = max_width
        .checked_sub(overhead)
        .and_then(|available| available.checked_sub(reserved))
    else {
        return widths;
    };

    let deficits: Vec<usize> = natural
        .iter()
        .zip(&widths)
        .map(|(&nat, &width)| nat.saturating_sub(width))
        .collect();
    let mut fixed: Vec<bool> = deficits.iter().map(|&deficit| deficit == 0).collect();
    let mut open = fixed.iter().filter(|&&is_fixed| !is_fixed).count();
    while open > 0 {
        let fair = remaining / open;
        let mut changed = false;
        for idx in 0..n {
            if !fixed[idx] && deficits[idx] <= fair {
                widths[idx] += deficits[idx];
                fixed[idx] = true;
                remaining -= deficits[idx];
                open -= 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    if open == 0 {
        return widths;
    }

    // Every open column has a deficit above the fair share, so the sum is non-zero.
    let open_deficit: usize = (0..n)
        .filter(|&idx| !fixed[idx])
        .map(|idx| deficits[idx])
        .sum();
    let mut assigned = 0_usize;
    for idx in 0..n {
        if !fixed[idx] {
            let share = remaining.saturating_mul(deficits[idx]) / open_deficit;
            widths[idx] += share;
            assigned += share;
        }
    }
    let mut leftover = remaining - assigned;
    for idx in 0..n {
        if leftover == 0 {
            break;
        }
        if !fixed[idx] && widths[idx] < natural[idx] {
            widths[idx] += 1;
            leftover -= 1;
        }
    }
    widths
}

/// Wrap a table cell's spans to `width`, hard-splitting words that are
/// still wider than the column. Always returns at least one (possibly empty) line.
fn wrap_cell_spans(spans: &[InlineSpan], width: usize) -> Vec<Vec<InlineSpan>> {
    let mut out = Vec::new();
    for mut line in wrap_spans(spans, width, "", "", false) {
        while line
            .last()
            .is_some_and(|span| span.text().chars().all(char::is_whitespace))
        {
            line.pop();
        }
        if display_width(&spans_to_string(&line)) > width {
            out.extend(split_spans_to_width(&line, width));
        } else {
            out.push(line);
        }
    }
    if out.is_empty() {
        out.push(Vec::new());
    }
    out
}

/// Split spans at character boundaries so each line is at most `width`
/// columns wide (a single character wider than `width` gets its own line).
fn split_spans_to_width(spans: &[InlineSpan], width: usize) -> Vec<Vec<InlineSpan>> {
    let mut lines = Vec::new();
    let mut current: Vec<InlineSpan> = Vec::new();
    let mut buf = String::new();
    let mut used = 0_usize;
    for span in spans {
        for ch in span.text().chars() {
            let w = ch.width().unwrap_or(0);
            if used + w > width && used > 0 {
                if !buf.is_empty() {
                    current.push(InlineSpan::new(std::mem::take(&mut buf), span.style()));
                }
                lines.push(std::mem::take(&mut current));
                used = 0;
            }
            buf.push(ch);
            used += w;
        }
        if !buf.is_empty() {
            current.push(InlineSpan::new(std::mem::take(&mut buf), span.style()));
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

fn render_table_inner_divider(widths: &[usize]) -> String {
    let mut out = String::new();
    for (idx, width) in widths.iter().enumerate() {
        out.push_str(&"─".repeat(width + 2));
        if idx + 1 < widths.len() {
            out.push('┼');
        }
    }
    out
}

fn render_table_line(
    cell_lines: &[&[InlineSpan]],
    widths: &[usize],
    alignments: &[TableAlignment],
) -> RenderedLine {
    let mut spans = Vec::new();
    for (idx, &width) in widths.iter().enumerate() {
        let cell = cell_lines.get(idx).copied().unwrap_or_default();
        let mut content_spans = truncate_spans_by_display_width(cell, width);
        let content_width = display_width(&spans_to_string(&content_spans));
        let padding = width.saturating_sub(content_width);

        spans.push(InlineSpan::new(" ".to_string(), InlineStyle::default()));
        match alignments.get(idx).copied().unwrap_or(TableAlignment::None) {
            TableAlignment::Right => {
                spans.push(InlineSpan::new(" ".repeat(padding), InlineStyle::default()));
                spans.append(&mut content_spans);
            }
            TableAlignment::Center => {
                let left = padding / 2;
                let right = padding - left;
                spans.push(InlineSpan::new(" ".repeat(left), InlineStyle::default()));
                spans.append(&mut content_spans);
                spans.push(InlineSpan::new(" ".repeat(right), InlineStyle::default()));
            }
            TableAlignment::Left | TableAlignment::None => {
                spans.append(&mut content_spans);
                spans.push(InlineSpan::new(" ".repeat(padding), InlineStyle::default()));
            }
        }
        if idx + 1 < widths.len() {
            spans.push(InlineSpan::new(" │".to_string(), InlineStyle::default()));
        }
    }
    let content = spans_to_string(&spans);
    RenderedLine::with_spans(content, LineType::Table, spans)
}

/// Render CSV content as table lines, reusing the existing table rendering infrastructure.
/// All rows are rendered; the TUI viewport handles paging/scrolling.
fn render_csv_as_table(csv_content: &str, max_width: usize) -> Vec<RenderedLine> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(csv_content.as_bytes());

    let mut rows: Vec<Vec<TableCellRender>> = Vec::new();
    for result in reader.records() {
        let Ok(record) = result else { continue };
        let cells: Vec<TableCellRender> = record
            .iter()
            .map(|field| {
                let text = field.trim().to_string();
                let spans = vec![InlineSpan::new(text.clone(), InlineStyle::default())];
                TableCellRender { text, spans }
            })
            .collect();
        if !cells.is_empty() {
            rows.push(cells);
        }
    }

    // CSV has no alignment info (all columns left-aligned); the first row is the header.
    render_table_rows(&rows, &[], true, max_width).lines
}

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

fn normalize_inline_whitespace(spans: &[InlineSpan]) -> Vec<InlineSpan> {
    let text = spans_to_string(spans);
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut first_word = true;
    for token in split_tokens_preserve_whitespace(spans) {
        let is_ws = token.text().chars().all(char::is_whitespace);
        if is_ws {
            continue;
        }
        if !first_word {
            result.push(InlineSpan::new(" ".to_string(), InlineStyle::default()));
        }
        result.push(token);
        first_word = false;
    }
    result
}

fn split_tokens_preserve_whitespace(spans: &[InlineSpan]) -> Vec<InlineSpan> {
    let mut out = Vec::new();
    for span in spans {
        out.extend(split_inline_tokens(span));
    }
    out
}

fn truncate_spans_by_display_width(spans: &[InlineSpan], max_width: usize) -> Vec<InlineSpan> {
    let mut out = Vec::new();
    let mut used = 0usize;

    for span in spans {
        if used >= max_width {
            break;
        }
        let mut taken = String::new();
        for ch in span.text().chars() {
            let w = ch.width().unwrap_or(0);
            if used + w > max_width {
                break;
            }
            taken.push(ch);
            used += w;
        }
        if !taken.is_empty() {
            out.push(InlineSpan::new(taken, span.style()));
        }
    }
    out
}

fn render_superscript_text(text: &str) -> String {
    render_script_text(text, true)
}

fn render_subscript_text(text: &str) -> String {
    render_script_text(text, false)
}

fn render_script_text(text: &str, superscript: bool) -> String {
    let Some(mapped) = map_script_chars(text, superscript) else {
        return if superscript {
            format!("^({text})")
        } else {
            format!("_({text})")
        };
    };
    mapped
}

fn render_footnote_reference(name: &str) -> String {
    // Reuse the same superscript renderer as inline superscript, but only for
    // numeric footnote labels to avoid uneven glyph spacing in some fonts.
    if name.chars().all(|c| c.is_ascii_digit()) {
        return render_superscript_text(name);
    }
    format!("[^{name}]")
}

fn map_script_chars(text: &str, superscript: bool) -> Option<String> {
    let mut mapped = String::new();
    for ch in text.chars() {
        let mapped_char = if superscript {
            superscript_char(ch)
        } else {
            subscript_char(ch)
        }?;
        mapped.push(mapped_char);
    }
    Some(mapped)
}

const fn superscript_char(ch: char) -> Option<char> {
    match ch {
        'a' => Some('ᵃ'),
        'b' => Some('ᵇ'),
        'c' => Some('ᶜ'),
        'd' => Some('ᵈ'),
        'e' => Some('ᵉ'),
        'f' => Some('ᶠ'),
        'g' => Some('ᵍ'),
        'h' => Some('ʰ'),
        '0' => Some('⁰'),
        '1' => Some('¹'),
        '2' => Some('²'),
        '3' => Some('³'),
        '4' => Some('⁴'),
        '5' => Some('⁵'),
        '6' => Some('⁶'),
        '7' => Some('⁷'),
        '8' => Some('⁸'),
        '9' => Some('⁹'),
        'j' => Some('ʲ'),
        'k' => Some('ᵏ'),
        'l' => Some('ˡ'),
        'm' => Some('ᵐ'),
        'o' => Some('ᵒ'),
        'p' => Some('ᵖ'),
        'r' => Some('ʳ'),
        's' => Some('ˢ'),
        't' => Some('ᵗ'),
        'u' => Some('ᵘ'),
        'v' => Some('ᵛ'),
        'w' => Some('ʷ'),
        'x' => Some('ˣ'),
        'y' => Some('ʸ'),
        'z' => Some('ᶻ'),
        '+' => Some('⁺'),
        '-' => Some('⁻'),
        '=' => Some('⁼'),
        '(' => Some('⁽'),
        ')' => Some('⁾'),
        'n' => Some('ⁿ'),
        'i' => Some('ⁱ'),
        _ => None,
    }
}

const fn subscript_char(ch: char) -> Option<char> {
    match ch {
        '0' => Some('₀'),
        '1' => Some('₁'),
        '2' => Some('₂'),
        '3' => Some('₃'),
        '4' => Some('₄'),
        '5' => Some('₅'),
        '6' => Some('₆'),
        '7' => Some('₇'),
        '8' => Some('₈'),
        '9' => Some('₉'),
        '+' => Some('₊'),
        '-' => Some('₋'),
        '=' => Some('₌'),
        '(' => Some('₍'),
        ')' => Some('₎'),
        'a' => Some('ₐ'),
        'e' => Some('ₑ'),
        'h' => Some('ₕ'),
        'i' => Some('ᵢ'),
        'j' => Some('ⱼ'),
        'k' => Some('ₖ'),
        'l' => Some('ₗ'),
        'm' => Some('ₘ'),
        'n' => Some('ₙ'),
        'o' => Some('ₒ'),
        'p' => Some('ₚ'),
        'r' => Some('ᵣ'),
        's' => Some('ₛ'),
        't' => Some('ₜ'),
        'u' => Some('ᵤ'),
        'v' => Some('ᵥ'),
        'x' => Some('ₓ'),
        _ => None,
    }
}

fn extract_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut text = String::new();
    extract_text_recursive(node, &mut text);
    text
}

fn collect_inline_spans<'a>(node: &'a AstNode<'a>) -> Vec<InlineSpan> {
    let mut spans = Vec::new();
    collect_inline_spans_recursive(node, InlineStyle::default(), &mut spans);
    spans
}

fn collect_inline_spans_recursive<'a>(
    node: &'a AstNode<'a>,
    style: InlineStyle,
    spans: &mut Vec<InlineSpan>,
) {
    match &node.data.borrow().value {
        NodeValue::List(_) | NodeValue::Item(_) => {}

        NodeValue::Text(t) => {
            push_text_with_footnote_fallback(spans, t, style);
        }
        NodeValue::Code(code) => {
            let mut code_style = style;
            code_style.code = true;
            code_style.emphasis = false;
            code_style.strong = false;
            code_style.strikethrough = false;
            spans.push(InlineSpan::new(code.literal.clone(), code_style));
        }
        NodeValue::Emph => {
            let mut next = style;
            next.emphasis = true;
            for child in node.children() {
                collect_inline_spans_recursive(child, next, spans);
            }
        }
        NodeValue::Strong => {
            let mut next = style;
            next.strong = true;
            for child in node.children() {
                collect_inline_spans_recursive(child, next, spans);
            }
        }
        NodeValue::Strikethrough => {
            let mut next = style;
            next.strikethrough = true;
            for child in node.children() {
                collect_inline_spans_recursive(child, next, spans);
            }
        }
        NodeValue::Superscript => {
            let mut inner = String::new();
            for child in node.children() {
                inner.push_str(&extract_text(child));
            }
            spans.push(InlineSpan::new(render_superscript_text(&inner), style));
        }
        NodeValue::Subscript => {
            let mut inner = String::new();
            for child in node.children() {
                inner.push_str(&extract_text(child));
            }
            spans.push(InlineSpan::new(render_subscript_text(&inner), style));
        }
        NodeValue::Link(_) => {
            let mut next = style;
            next.link = true;
            for child in node.children() {
                collect_inline_spans_recursive(child, next, spans);
            }
        }
        NodeValue::FootnoteReference(reference) => {
            spans.push(InlineSpan::new(
                render_footnote_reference(&reference.name),
                style,
            ));
        }
        NodeValue::Math(math) => {
            let mut math_style = style;
            math_style.math = true;
            let text = crate::math::latex_to_unicode(&math.literal);
            spans.push(InlineSpan::new(text, math_style));
        }
        NodeValue::LineBreak => {
            spans.push(InlineSpan::new(HARD_BREAK.to_string(), style));
        }
        NodeValue::SoftBreak => {
            // CommonMark joins soft breaks with a space; Chinese and Japanese
            // have no inter-word spaces, so join breaks between them directly.
            let prev = node
                .previous_sibling()
                .and_then(|sibling| extract_text(sibling).chars().next_back());
            let next = node
                .next_sibling()
                .and_then(|sibling| extract_text(sibling).chars().next());
            let joins_cjk = matches!(
                (prev, next),
                (Some(a), Some(b)) if joins_without_space(a) && joins_without_space(b)
            );
            if !joins_cjk {
                spans.push(InlineSpan::new(" ".to_string(), style));
            }
        }
        _ => {
            for child in node.children() {
                collect_inline_spans_recursive(child, style, spans);
            }
        }
    }
}

fn find_task_marker<'a>(node: &'a AstNode<'a>) -> Option<&'static str> {
    for child in node.children() {
        match &child.data.borrow().value {
            NodeValue::TaskItem(symbol) => {
                return Some(if symbol.is_some() { "✓" } else { "□" });
            }
            _ => {
                if let Some(found) = find_task_marker(child) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn extract_text_recursive<'a>(node: &'a AstNode<'a>, text: &mut String) {
    match &node.data.borrow().value {
        NodeValue::Text(t) => {
            text.push_str(&render_text_with_footnote_fallback(t));
        }
        NodeValue::Code(c) => {
            text.push('`');
            text.push_str(&c.literal);
            text.push('`');
        }
        NodeValue::Superscript => {
            let mut inner = String::new();
            for child in node.children() {
                extract_text_recursive(child, &mut inner);
            }
            text.push_str(&render_superscript_text(&inner));
        }
        NodeValue::Subscript => {
            let mut inner = String::new();
            for child in node.children() {
                extract_text_recursive(child, &mut inner);
            }
            text.push_str(&render_subscript_text(&inner));
        }
        NodeValue::FootnoteReference(reference) => {
            text.push_str(&render_footnote_reference(&reference.name));
        }
        NodeValue::Math(math) => {
            text.push_str(&crate::math::latex_to_unicode(&math.literal));
        }
        NodeValue::SoftBreak | NodeValue::LineBreak => {
            text.push('\n');
        }
        _ => {
            for child in node.children() {
                extract_text_recursive(child, text);
            }
        }
    }
}

/// Span emitted for a hard line break. Text, Code and Math nodes never
/// produce `\n`, so a span with exactly this text only comes from `LineBreak`.
const HARD_BREAK: &str = "\n";

fn wrap_spans(
    spans: &[InlineSpan],
    width: usize,
    prefix_first: &str,
    prefix_next: &str,
    first_prefix_is_list_marker: bool,
) -> Vec<Vec<InlineSpan>> {
    let mut tokens: Vec<InlineSpan> = Vec::new();
    for span in spans {
        tokens.extend(split_wrap_tokens(span));
    }

    let mut lines: Vec<Vec<InlineSpan>> = Vec::new();
    let mut current: Vec<InlineSpan> = Vec::new();
    let mut current_len = 0usize;
    let mut has_word = false;
    // A whitespace token can wrap before the next content token arrives.
    let mut pending_soft_wrap = false;

    let start_new_line = |prefix: &str,
                          list_marker: bool,
                          current: &mut Vec<InlineSpan>,
                          current_len: &mut usize,
                          has_word: &mut bool| {
        current.clear();
        if prefix.is_empty() {
            *current_len = 0;
        } else {
            current.push(InlineSpan::new(
                prefix.to_string(),
                InlineStyle {
                    list_marker,
                    ..InlineStyle::default()
                },
            ));
            *current_len = UnicodeWidthStr::width(prefix);
        }
        *has_word = false;
    };

    start_new_line(
        prefix_first,
        first_prefix_is_list_marker,
        &mut current,
        &mut current_len,
        &mut has_word,
    );

    for token in tokens {
        if token.text() == HARD_BREAK {
            if !pending_soft_wrap {
                lines.push(std::mem::take(&mut current));
            }
            start_new_line(
                prefix_next,
                false,
                &mut current,
                &mut current_len,
                &mut has_word,
            );
            pending_soft_wrap = false;
            continue;
        }
        let token_len = UnicodeWidthStr::width(token.text());
        let token_is_ws = token.text().chars().all(char::is_whitespace);

        if current_len + token_len > width && has_word {
            lines.push(current.clone());
            start_new_line(
                prefix_next,
                false,
                &mut current,
                &mut current_len,
                &mut has_word,
            );
            pending_soft_wrap = true;
        }

        if token_is_ws && !has_word {
            // Drop leading whitespace at wrapped line starts.
            continue;
        }

        current_len += token_len;
        pending_soft_wrap = false;
        current.push(token);
        has_word = token_is_ws || has_word;
        if !token_is_ws {
            has_word = true;
        }
    }

    if current.is_empty() && !prefix_first.is_empty() {
        current.push(InlineSpan::new(
            prefix_first.to_string(),
            InlineStyle {
                list_marker: first_prefix_is_list_marker,
                ..InlineStyle::default()
            },
        ));
    }

    lines.push(current);
    lines
}

fn push_text_with_footnote_fallback(spans: &mut Vec<InlineSpan>, text: &str, style: InlineStyle) {
    let rendered = render_text_with_footnote_fallback(text);
    if !rendered.is_empty() {
        spans.push(InlineSpan::new(rendered, style));
    }
}

fn render_text_with_footnote_fallback(text: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '[' && i + 3 < chars.len() && chars[i + 1] == '^' {
            let mut j = i + 2;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 2 && j < chars.len() && chars[j] == ']' {
                let digits: String = chars[i + 2..j].iter().collect();
                out.push_str(&render_superscript_text(&digits));
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn split_inline_tokens(span: &InlineSpan) -> Vec<InlineSpan> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut ws_state: Option<bool> = None;

    for ch in span.text().chars() {
        let is_ws = ch.is_whitespace();
        match ws_state {
            Some(state) if state == is_ws => {
                buf.push(ch);
            }
            Some(_) => {
                out.push(InlineSpan::new(std::mem::take(&mut buf), span.style()));
                buf.push(ch);
                ws_state = Some(is_ws);
            }
            None => {
                buf.push(ch);
                ws_state = Some(is_ws);
            }
        }
    }

    if !buf.is_empty() {
        out.push(InlineSpan::new(buf, span.style()));
    }

    out
}

/// Characters that must not begin a line (closing punctuation).
const NO_LINE_START: &[char] = &[
    '，', '。', '、', '；', '：', '！', '？', '）', '」', '』', '】', '》', '〉', '〕', '］', '｝',
    '”', '’', '…', ',', '.', ';', ':', '!', '?', ')', ']', '}', '%',
];

/// Characters that must not end a line (opening punctuation).
const NO_LINE_END: &[char] = &[
    '（', '「', '『', '【', '《', '〈', '〔', '［', '｛', '“', '‘', '(', '[', '{',
];

/// Whether `ch` belongs to a CJK script (Hangul, CJK punctuation/ideographs,
/// kana, Bopomofo, fullwidth forms), where a line may break between characters.
const fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x11FF
            | 0x2E80..=0x2FDF
            | 0x3000..=0x303F
            | 0x3040..=0x30FF
            | 0x3100..=0x312F
            | 0x3130..=0x318F
            | 0x31F0..=0x31FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0xFF00..=0xFFEF
            | 0x20000..=0x2FFFF
    )
}

/// Whether a soft break next to `ch` needs no space: CJK except Hangul,
/// because Korean separates words with spaces.
const fn joins_without_space(ch: char) -> bool {
    is_cjk_char(ch) && !matches!(ch as u32, 0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF)
}

/// Split a span into wrap candidates: like `split_inline_tokens`, plus
/// break opportunities between adjacent characters when either is CJK
/// (respecting line-start/line-end punctuation rules). Only for
/// `wrap_spans`; `normalize_inline_whitespace` must keep whole words.
fn split_wrap_tokens(span: &InlineSpan) -> Vec<InlineSpan> {
    let mut out = Vec::new();
    for token in split_inline_tokens(span) {
        if !token.text().chars().any(is_cjk_char) {
            out.push(token);
            continue;
        }
        let mut buf = String::new();
        let mut prev: Option<char> = None;
        for ch in token.text().chars() {
            if let Some(a) = prev
                && (is_cjk_char(a) || is_cjk_char(ch))
                && !NO_LINE_START.contains(&ch)
                && !NO_LINE_END.contains(&a)
            {
                out.push(InlineSpan::new(std::mem::take(&mut buf), span.style()));
            }
            buf.push(ch);
            prev = Some(ch);
        }
        if !buf.is_empty() {
            out.push(InlineSpan::new(buf, span.style()));
        }
    }
    out
}

fn spans_to_string(spans: &[InlineSpan]) -> String {
    let mut content = String::new();
    for span in spans {
        content.push_str(span.text());
    }
    content
}

fn truncate_spans(spans: &[InlineSpan], max_len: usize) -> Vec<InlineSpan> {
    let mut out = Vec::new();
    let mut remaining = max_len;
    for span in spans {
        if remaining == 0 {
            break;
        }
        let mut taken = String::new();
        let mut used = 0usize;
        for ch in span.text().chars() {
            let w = ch.width().unwrap_or(0);
            if used + w > remaining {
                break;
            }
            taken.push(ch);
            used += w;
        }
        if used > 0 {
            out.push(InlineSpan::new(taken, span.style()));
            remaining = remaining.saturating_sub(used);
        }
    }
    out
}

/// Collect images from a paragraph node, returning (alt, src) pairs.
fn collect_paragraph_images<'a>(node: &'a AstNode<'a>) -> Vec<(String, String)> {
    let mut images = Vec::new();
    collect_paragraph_images_recursive(node, &mut images);
    images
}

fn collect_paragraph_images_recursive<'a>(
    node: &'a AstNode<'a>,
    images: &mut Vec<(String, String)>,
) {
    match &node.data.borrow().value {
        NodeValue::Image(image) => {
            let alt = extract_text(node);
            images.push((alt, image.url.clone()));
        }
        _ => {
            for child in node.children() {
                collect_paragraph_images_recursive(child, images);
            }
        }
    }
}

/// After wrapping, fix up `LinkRef.line` values so each link points to
/// the actual rendered line that contains its text, not the first line
/// of the paragraph.
///
/// When link text wraps across multiple rendered lines, the original
/// `LinkRef` is updated to cover the first line and additional `LinkRef`
/// entries (one per extra line) are returned so that every line containing
/// part of the link text is clickable.
///
/// `prefix_len` is the byte length of the line prefix (e.g. blockquote
/// marker, list indent) that should be stripped before cross-line
/// matching.  All wrapped lines share the same prefix length.
fn fixup_link_lines(
    links: &mut [LinkRef],
    wrapped_lines: &[RenderedLine],
    base_line: usize,
    prefix_len: usize,
) -> Vec<LinkRef> {
    // Track which (line_index, byte_offset) occurrences have been claimed
    // so duplicate link text (e.g., two links both labelled "here") each
    // match a distinct occurrence in the rendered output.
    let mut claimed: Vec<(usize, usize)> = Vec::new();
    let mut extra: Vec<LinkRef> = Vec::new();
    // Built on first use: most links fit on a single line.
    let mut compact: Option<CompactLines> = None;

    for link in links.iter_mut() {
        if link.text.is_empty() {
            continue;
        }
        // First pass: try to find the full link text on a single line.
        let mut found = false;
        for (i, line) in wrapped_lines.iter().enumerate() {
            let content = line.content();
            let mut search = 0usize;
            while let Some(pos) = content[search..].find(&link.text) {
                let byte_offset = search + pos;
                if !claimed.contains(&(i, byte_offset)) {
                    link.line = base_line + i;
                    claimed.push((i, byte_offset));
                    found = true;
                    break;
                }
                search = byte_offset + link.text.len();
            }
            if found {
                break;
            }
        }

        if found || wrapped_lines.len() < 2 {
            continue;
        }

        // Match each boundary against the original label: a wrap can
        // remove whitespace or split adjacent CJK characters in the same link.
        let compact = compact.get_or_insert_with(|| CompactLines::new(wrapped_lines, prefix_len));
        if let Some(portions) =
            find_wrapped_link(wrapped_lines, &link.text, prefix_len, &claimed, compact)
        {
            for (part, (line, range)) in portions.into_iter().enumerate() {
                let text = wrapped_lines[line].content()[range.clone()].to_string();
                claimed.push((line, range.start));
                if part == 0 {
                    link.line = base_line + line;
                    link.text = text;
                } else {
                    extra.push(LinkRef {
                        text,
                        url: link.url.clone(),
                        line: base_line + line,
                    });
                }
            }
        }
    }

    extra
}

/// Rendered lines with all whitespace removed, so candidate starts for a
/// wrapped label come from one linear substring search wherever wrapping
/// dropped spaces or line breaks.
struct CompactLines {
    text: String,
    /// For each char of `text`: its byte offset in `text`, its line, and its
    /// byte offset in that line's prefix-stripped content.
    origins: Vec<(usize, usize, usize)>,
}

impl CompactLines {
    fn new(lines: &[RenderedLine], prefix_len: usize) -> Self {
        let mut text = String::new();
        let mut origins = Vec::new();
        for (line_idx, line) in lines.iter().enumerate() {
            let content = line.content().get(prefix_len..).unwrap_or("");
            for (byte, ch) in content.char_indices() {
                if !ch.is_whitespace() {
                    origins.push((text.len(), line_idx, byte));
                    text.push(ch);
                }
            }
        }
        Self { text, origins }
    }

    /// Line and content offset of the char at byte `pos` of `text`.
    fn origin(&self, pos: usize) -> (usize, usize) {
        let idx = self.origins.partition_point(|&(byte, _, _)| byte < pos);
        let (_, line, offset) = self.origins[idx];
        (line, offset)
    }
}

/// Locate a label across lines without inventing a uniform separator.
/// Every match keeps the label's non-whitespace characters contiguous, so
/// only occurrences in `compact` are verified, in rendered order.
fn find_wrapped_link(
    lines: &[RenderedLine],
    text: &str,
    prefix_len: usize,
    claimed: &[(usize, usize)],
    compact: &CompactLines,
) -> Option<Vec<(usize, Range<usize>)>> {
    let needle: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
    let first_len = needle.chars().next()?.len_utf8();
    // Leading whitespace can only match literally on the starting line.
    let lead = text.len() - text.trim_start().len();
    let mut from = 0;
    while let Some(pos) = compact.text[from..].find(&needle) {
        let hit = from + pos;
        from = hit + first_len;
        let (line, offset) = compact.origin(hit);
        let Some(start) = offset.checked_sub(lead) else {
            continue;
        };
        if claimed.contains(&(line, prefix_len + start)) {
            continue;
        }
        if let Some(portions) = match_wrapped_link(lines, text, prefix_len, line, start) {
            return Some(portions);
        }
    }
    None
}

/// Match `text` from byte `start` of `start_line`'s prefix-stripped content.
/// Whitespace in the original label may disappear only at a line boundary.
fn match_wrapped_link(
    lines: &[RenderedLine],
    text: &str,
    prefix_len: usize,
    start_line: usize,
    start: usize,
) -> Option<Vec<(usize, Range<usize>)>> {
    let mut remaining = text;
    let mut portions = Vec::new();
    for (idx, line) in lines.iter().enumerate().skip(start_line) {
        let content = line.content().get(prefix_len..).unwrap_or("").trim_end();
        let offset = if idx == start_line { start } else { 0 };
        let part = content.get(offset..)?;
        if part.is_empty() {
            remaining = remaining.strip_prefix(HARD_BREAK)?;
            continue;
        }
        let matched = part
            .chars()
            .zip(remaining.chars())
            .take_while(|(a, b)| a == b)
            .map(|(ch, _)| ch.len_utf8())
            .sum::<usize>();
        if matched == 0 {
            return None;
        }
        portions.push((idx, prefix_len + offset..prefix_len + offset + matched));
        remaining = &remaining[matched..];
        if remaining.is_empty() {
            return Some(portions);
        }
        if matched != part.len() {
            return None;
        }
        // An ordinary wrap may discard spaces, but each explicit
        // hard break consumes exactly one physical line boundary.
        remaining = remaining.trim_start_matches(|ch: char| ch.is_whitespace() && ch != '\n');
        remaining = remaining.strip_prefix(HARD_BREAK).unwrap_or(remaining);
        if remaining.is_empty() {
            return Some(portions);
        }
    }
    None
}

fn collect_inline_elements<'a>(
    node: &'a AstNode<'a>,
    base_line: usize,
    images: &mut Vec<ImageRef>,
    links: &mut Vec<LinkRef>,
) {
    match &node.data.borrow().value {
        NodeValue::Image(image) => {
            let alt = extract_text(node);
            let src = image.url.clone();
            images.push(ImageRef {
                alt,
                src,
                line_range: base_line..base_line + 1,
            });
        }
        NodeValue::Link(link) => {
            // Use rendered inline text so code-style link labels (e.g. [`foo`])
            // match what appears on screen for clickable line fixups.
            // Keep explicit breaks so matching can distinguish intentional
            // empty lines from whitespace discarded by automatic wrapping.
            let text = spans_to_string(&collect_inline_spans(node));
            let url = link.url.clone();
            links.push(LinkRef {
                text,
                url,
                line: base_line,
            });
        }
        NodeValue::FootnoteReference(reference) => {
            links.push(LinkRef {
                text: render_footnote_reference(&reference.name),
                url: format!("footnote:{}", reference.name),
                line: base_line,
            });
        }
        NodeValue::Text(t) => {
            // When comrak emits [^N] as plain text (no matching definition),
            // render_text_with_footnote_fallback converts it to superscript.
            // We need to create LinkRefs for these too.
            collect_text_footnote_links(t, base_line, links);
        }
        _ => {
            for child in node.children() {
                collect_inline_elements(child, base_line, images, links);
            }
        }
    }
}

/// Scan a plain text string for `[^N]` footnote patterns (the same ones that
/// `render_text_with_footnote_fallback` converts to superscript) and create
/// `LinkRef` entries so they are clickable.
fn collect_text_footnote_links(text: &str, base_line: usize, links: &mut Vec<LinkRef>) {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '[' && i + 3 < chars.len() && chars[i + 1] == '^' {
            let mut j = i + 2;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 2 && j < chars.len() && chars[j] == ']' {
                let digits: String = chars[i + 2..j].iter().collect();
                links.push(LinkRef {
                    text: render_superscript_text(&digits),
                    url: format!("footnote:{digits}"),
                    line: base_line,
                });
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
}

/// Extract `(src, alt)` pairs from `<img>` tags in raw HTML.
fn extract_html_images(html: &str) -> Vec<(String, String)> {
    let mut results = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut search = 0;
    while let Some(pos) = lower[search..].find("<img") {
        let abs = search + pos;
        // Find the end of this tag
        let tag_end = match html[abs..].find('>') {
            Some(e) => abs + e + 1,
            None => break,
        };
        let tag = &html[abs..tag_end];
        if let Some(src) = extract_html_attr(tag, "src") {
            let alt = extract_html_attr(tag, "alt").unwrap_or_default();
            results.push((src, alt));
        }
        search = tag_end;
    }
    results
}

/// Extract the value of an HTML attribute from a tag string.
fn extract_html_attr(tag: &str, attr_name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let pattern = format!("{attr_name}=");
    let pos = lower.find(&pattern)?;
    let after = &tag[pos + pattern.len()..];
    let trimmed = after.trim_start();
    if let Some(content) = trimmed.strip_prefix('"') {
        let end = content.find('"')?;
        Some(content[..end].to_string())
    } else if let Some(content) = trimmed.strip_prefix('\'') {
        let end = content.find('\'')?;
        Some(content[..end].to_string())
    } else {
        // Unquoted attribute value - take until whitespace or >
        let end = trimmed
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(trimmed.len());
        if end > 0 {
            Some(trimmed[..end].to_string())
        } else {
            None
        }
    }
}

/// Emit an image placeholder for an HTML `<img>` tag.
fn emit_html_image<S: BuildHasher>(ctx: &mut ParseContext<'_, S>, src: &str, alt: &str) {
    let line_num = ctx.lines.len();
    let label = format!("[Image: {}]", if alt.is_empty() { src } else { alt });
    let height_lines = ctx.image_heights.get(src).copied().unwrap_or(1).max(1);
    let has_caption = ctx.image_heights.contains_key(src) && !alt.is_empty();

    ctx.images.push(ImageRef {
        alt: alt.to_string(),
        src: src.to_string(),
        line_range: line_num + usize::from(has_caption)
            ..line_num + usize::from(has_caption) + height_lines,
    });

    ctx.link_refs.push(LinkRef {
        text: label.clone(),
        url: src.to_string(),
        line: line_num + usize::from(has_caption),
    });

    if has_caption {
        ctx.lines
            .push(RenderedLine::new(format!("    {alt}"), LineType::Image));
    }
    ctx.lines.push(RenderedLine::new(label, LineType::Image));

    for _ in 1..height_lines {
        ctx.lines
            .push(RenderedLine::new(String::new(), LineType::Image));
    }
    ctx.lines
        .push(RenderedLine::new(String::new(), LineType::Empty));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_and_block_code_keep_distinct_semantics_after_highlighting() {
        let mut doc = parse("`same text`\n\n```text\nsame text\n```").unwrap();
        for highlighted in [false, true] {
            if highlighted {
                doc.ensure_highlight_for_range(0..doc.line_count());
            }
            let lines = doc.visible_lines(0, doc.line_count());
            let inline = lines
                .iter()
                .filter(|line| matches!(line.line_type(), LineType::Paragraph))
                .flat_map(|line| line.spans().unwrap_or_default())
                .find(|span| span.style().code)
                .expect("inline raw code");
            assert!(!inline.style().code_block);
            let block = lines
                .iter()
                .filter(|line| matches!(line.line_type(), LineType::CodeBlock))
                .flat_map(|line| line.spans().unwrap_or_default())
                .find(|span| span.style().code)
                .expect("block raw code");
            assert!(block.style().code_block);
            assert!(!block.style().strong);
            assert!(!block.style().code_frame);
        }
    }

    #[test]
    fn list_marker_metadata_does_not_emphasize_body_or_continuations() {
        for marker in ["-", "1.", "- [x]", "- [ ]"] {
            let doc = Document::parse_with_layout(
                &format!("{marker} normal body wraps onto another line"),
                20,
            )
            .unwrap();
            let lines = doc.visible_lines(0, doc.line_count());
            let list_lines: Vec<_> = lines
                .iter()
                .filter(|line| matches!(line.line_type(), LineType::ListItem(_)))
                .collect();
            assert!(list_lines.len() > 1);
            let first_spans = list_lines[0].spans().unwrap();
            assert!(first_spans[0].style().list_marker);
            for span in first_spans.iter().skip(1).chain(
                list_lines
                    .iter()
                    .skip(1)
                    .flat_map(|line| line.spans().unwrap_or_default()),
            ) {
                assert!(!span.style().list_marker);
                assert!(!span.style().strong);
            }
        }
    }

    #[test]
    fn test_parse_empty_document() {
        let doc = parse("").unwrap();
        assert_eq!(doc.line_count(), 0);
    }

    #[test]
    fn test_parse_simple_paragraph() {
        let doc = parse("Hello world").unwrap();
        assert!(doc.line_count() >= 1);
        let lines = doc.visible_lines(0, 10);
        assert!(lines.iter().any(|l| l.content().contains("Hello")));
    }

    #[test]
    fn test_parse_heading() {
        let doc = parse("# Title").unwrap();
        assert_eq!(doc.headings().len(), 1);
        assert_eq!(doc.headings()[0].text, "Title");
        assert_eq!(doc.headings()[0].level, 1);
    }

    #[test]
    fn test_parse_multiple_headings() {
        let doc = parse("# One\n\n## Two\n\n### Three").unwrap();
        assert_eq!(doc.headings().len(), 3);
        assert_eq!(doc.headings()[0].level, 1);
        assert_eq!(doc.headings()[1].level, 2);
        assert_eq!(doc.headings()[2].level, 3);
    }

    #[test]
    fn test_parse_code_block() {
        let doc = parse("```rust\nfn main() {}\n```").unwrap();
        let lines = doc.visible_lines(0, 10);
        assert!(lines.iter().any(|l| *l.line_type() == LineType::CodeBlock));
    }

    #[test]
    fn test_parse_list() {
        let doc = parse("- Item 1\n- Item 2").unwrap();
        let lines = doc.visible_lines(0, 10);
        assert!(lines.iter().any(|l| l.content().contains("Item 1")));
    }

    #[test]
    fn test_parse_link() {
        let doc = parse("[Click here](https://example.com)").unwrap();
        assert_eq!(doc.links().len(), 1);
        assert_eq!(doc.links()[0].url, "https://example.com");
    }

    #[test]
    fn test_parse_image() {
        let doc = parse("![Alt text](image.png)").unwrap();
        assert_eq!(doc.images().len(), 1);
        assert_eq!(doc.images()[0].alt, "Alt text");
        assert_eq!(doc.images()[0].src, "image.png");
    }

    #[test]
    fn test_parse_blockquote() {
        let doc = parse("> This is a quote").unwrap();
        let lines = doc.visible_lines(0, 10);
        assert!(lines.iter().any(|l| *l.line_type() == LineType::BlockQuote));
        assert!(lines.iter().any(|l| l.content().starts_with("  │ ")));
        assert!(!lines.iter().any(|l| l.content().starts_with("> ")));
    }

    #[test]
    fn test_blockquote_wraps_with_quote_prefix() {
        let md = "> This is a long block quote line that should wrap and keep the quote prefix.";
        let doc = Document::parse_with_layout(md, 30).unwrap();
        let lines = doc.visible_lines(0, 20);
        let quote_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::BlockQuote)
            .collect();
        assert!(quote_lines.len() > 1);
        for line in quote_lines {
            assert!(line.content().starts_with("  │ "));
            assert!(unicode_width::UnicodeWidthStr::width(line.content()) <= 30);
        }
    }

    #[test]
    fn test_heading_line_numbers() {
        let doc = parse("# First\n\nParagraph\n\n# Second").unwrap();
        assert_eq!(doc.headings().len(), 2);
        // Headings have two rows above them.
        assert_eq!(doc.headings()[0].line, 2);
        // Second heading should be after the first heading + empty + paragraph + empty
        assert!(doc.headings()[1].line > doc.headings()[0].line);
    }

    #[test]
    fn test_heading_has_two_rows_above() {
        let doc = Document::parse_with_layout("Paragraph\n\n## Heading", 80).unwrap();
        let heading_line = doc.headings().first().expect("heading missing").line;
        let lines = doc.visible_lines(0, heading_line + 1);
        assert!(heading_line >= 2);
        assert_eq!(*lines[heading_line - 1].line_type(), LineType::Empty);
        assert_eq!(*lines[heading_line - 2].line_type(), LineType::Empty);
    }

    #[test]
    fn test_gfm_strikethrough() {
        let doc = parse("~~deleted~~").unwrap();
        // Should parse without error (content check would need styled spans)
        assert!(doc.line_count() > 0);
    }

    #[test]
    fn test_gfm_tasklist() {
        let doc = parse("- [x] Done\n- [ ] Todo").unwrap();
        assert!(doc.line_count() > 0);
    }

    #[test]
    fn test_gfm_table() {
        let doc = parse("| A | B |\n|---|---|\n| 1 | 2 |").unwrap();
        let lines = doc.visible_lines(0, 10);
        let table_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::Table)
            .collect();
        assert!(!table_lines.is_empty());
        assert!(
            table_lines
                .iter()
                .any(|l| l.content().contains("A") && l.content().contains("B"))
        );
        assert!(
            table_lines
                .iter()
                .any(|l| l.content().contains("1") && l.content().contains("2"))
        );
        assert!(table_lines.iter().any(|l| l.content().contains('│')));
        assert!(table_lines.iter().any(|l| l.content().contains("───┼───")));
    }

    #[test]
    fn test_gfm_table_preserves_cell_content() {
        let md = "| Very long heading | Value |\n|---|---:|\n| some really long content | 12345 |";
        let doc = Document::parse_with_layout(md, 24).unwrap();
        let table = table_contents(&doc);
        for line in &table {
            assert!(UnicodeWidthStr::width(line.as_str()) <= 24, "{line:?}");
        }
        for word in ["Very", "long", "heading", "some", "really", "content"] {
            assert!(
                table.iter().any(|l| l.contains(word)),
                "missing {word:?} in table"
            );
        }
        assert_eq!(table.iter().filter(|l| l.contains('┼')).count(), 1);
    }

    fn paragraph_contents(doc: &Document) -> Vec<String> {
        doc.visible_lines(0, doc.line_count())
            .iter()
            .filter(|l| *l.line_type() == LineType::Paragraph)
            .map(|l| l.content().to_string())
            .collect()
    }

    fn table_contents(doc: &Document) -> Vec<String> {
        doc.visible_lines(0, doc.line_count())
            .iter()
            .filter(|l| *l.line_type() == LineType::Table)
            .map(|l| l.content().to_string())
            .collect()
    }

    #[test]
    fn test_hard_line_break_backslash_and_spaces() {
        let doc = Document::parse_with_layout("A\\\nB\n\nC  \nD", 80).unwrap();
        assert_eq!(paragraph_contents(&doc), ["A", "B", "C", "D"]);
    }

    #[test]
    fn test_hard_line_break_after_wrapped_space() {
        for (md, expected) in [
            ("abcd \\\nX", vec!["abcd", "X"]),
            ("abcd \\\n\\\nX", vec!["abcd", "", "X"]),
            ("abcd\\\n\\\nX", vec!["abcd", "", "X"]),
        ] {
            let doc = Document::parse_with_layout(md, 4).unwrap();
            assert_eq!(paragraph_contents(&doc), expected, "{md:?}");
        }
    }

    #[test]
    fn test_hard_line_break_link_preserves_empty_lines() {
        for prefix in ["", "> ", "- "] {
            let continuation = if prefix == "- " { "  " } else { prefix };
            let md = format!("{prefix}[a\\\n{continuation}\\\n{continuation}b](https://x.test)");
            let doc = Document::parse_with_layout(&md, 80).unwrap();
            let links = doc.links();
            assert_eq!(links.len(), 2, "{md:?}: {links:?}");
            assert_eq!(links[0].text, "a");
            assert_eq!(links[1].text, "b");
            assert_eq!(links[1].line, links[0].line + 2);
            for link in links {
                assert_eq!(link.url, "https://x.test");
                assert!(
                    doc.visible_lines(link.line, 1)[0]
                        .content()
                        .contains(&link.text)
                );
            }
        }
    }

    #[test]
    fn test_hard_line_break_in_list_item_uses_continuation_indent() {
        let second_line = |md: &str, width: u16| {
            let doc = Document::parse_with_layout(md, width).unwrap();
            doc.visible_lines(0, doc.line_count())[1]
                .content()
                .to_string()
        };
        let soft_wrapped = second_line("- aaaa bbbb", 9);
        let indent = soft_wrapped
            .strip_suffix("bbbb")
            .expect("soft-wrapped item");
        assert!(!indent.is_empty() && indent.trim().is_empty());
        assert_eq!(second_line("- one\\\n  two", 80), format!("{indent}two"));
    }

    #[test]
    fn test_cjk_soft_break_joins_without_space() {
        let render = |md: &str| paragraph_contents(&Document::parse_with_layout(md, 80).unwrap());
        assert_eq!(render("中文\n继续"), ["中文继续"]);
        assert_eq!(render("中文\nEnglish"), ["中文 English"]);
        assert_eq!(render("foo\nbar"), ["foo bar"]);
        assert_eq!(render("나는\n학생입니다"), ["나는 학생입니다"]);
    }

    #[test]
    fn test_cjk_paragraph_wraps_by_character() {
        let text = "一二三四五六七八九十，".repeat(6);
        let doc = Document::parse_with_layout(&text, 20).unwrap();
        let lines = paragraph_contents(&doc);
        assert!(lines.len() > 1);
        for line in &lines {
            assert!(UnicodeWidthStr::width(line.as_str()) <= 20, "{line:?}");
            assert!(!line.starts_with('，'), "{line:?}");
        }
        assert_eq!(lines.concat(), text);
    }

    #[test]
    fn test_cjk_link_split_across_lines_is_clickable() {
        let md = "前文前文前文前文[中文链接文本很长很长](https://x.test)";
        let doc = Document::parse_with_layout(md, 12).unwrap();
        let refs: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url == "https://x.test")
            .collect();
        assert!(refs.len() > 1, "link should span lines: {refs:?}");
        let joined: String = refs.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(joined, "中文链接文本很长很长");
        for link in refs {
            let line = doc.visible_lines(link.line, 1)[0];
            assert!(line.content().contains(&link.text), "{link:?}");
        }
    }

    #[test]
    fn test_cjk_mixed_link_keeps_all_wrapped_portions_clickable() {
        let doc =
            Document::parse_with_layout("[中文中文 English 中文中文](https://x.test)", 6).unwrap();
        let links = doc.links();
        assert_eq!(
            links.iter().map(|l| l.text.as_str()).collect::<String>(),
            "中文中文English中文中文"
        );
        assert_eq!(
            links.iter().map(|l| l.line).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4]
        );
        for link in links {
            assert!(
                doc.visible_lines(link.line, 1)[0]
                    .content()
                    .contains(&link.text)
            );
        }
    }

    #[test]
    fn test_allocate_column_widths() {
        assert_eq!(allocate_column_widths(&[3, 4], &[1, 1], 40), [3, 4]);
        let widths = allocate_column_widths(&[2, 50, 50], &[1, 1, 1], 40);
        assert_eq!(widths[0], 2);
        assert!(widths.iter().sum::<usize>() + 3 * 3 - 1 <= 40, "{widths:?}");
        assert_eq!(allocate_column_widths(&[10, 10], &[1, 1], 3), [1, 1]);
    }

    #[test]
    fn test_table_wrapped_link_portions_are_clickable() {
        let md = "| A | B |\n|---|---|\n| [abcdefghijklmnop](https://x.test) | z |";
        let doc = Document::parse_with_layout(md, 16).unwrap();
        let links: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url == "https://x.test")
            .collect();
        assert_eq!(
            links.iter().map(|l| l.text.as_str()).collect::<String>(),
            "abcdefghijklmnop"
        );
        assert!(links.len() > 1, "{links:?}");
        for link in links {
            assert!(
                doc.visible_lines(link.line, 1)[0]
                    .content()
                    .contains(&link.text),
                "{link:?}"
            );
        }
    }

    #[test]
    fn test_table_skewed_columns_fit_width() {
        let md = format!(
            "| aaa | bbb | {} |\n|---|---|---|\n| x | y | z |",
            "c".repeat(100)
        );
        let doc = Document::parse_with_layout(&md, 14).unwrap();
        for line in table_contents(&doc) {
            assert!(UnicodeWidthStr::width(line.as_str()) <= 14, "{line:?}");
        }
    }

    #[test]
    fn test_table_keeps_wide_characters_when_width_allows() {
        let md = format!("| A | B |\n|---|---|\n| {} | 中文字 |", "x".repeat(100));
        let doc = Document::parse_with_layout(&md, 15).unwrap();
        let table = table_contents(&doc);
        for ch in ['中', '文', '字'] {
            assert!(table.iter().any(|l| l.contains(ch)), "{ch}: {table:#?}");
        }
        for line in &table {
            assert!(UnicodeWidthStr::width(line.as_str()) <= 15, "{line:?}");
        }
    }

    #[test]
    fn test_table_wraps_long_cell_and_keeps_alignment_and_links() {
        let md = "| IC | 新建项 | 级 |\n|---|---|---:|\n| IC-016 | [ast-cu9.15](https://a.test) Analysis Object 名称约束，Authority Instance 永久 UUID | P1 |";
        let doc = Document::parse_with_layout(md, 40).unwrap();
        let table = table_contents(&doc);
        for line in &table {
            assert!(UnicodeWidthStr::width(line.as_str()) <= 40, "{line:?}");
        }
        let divider = table
            .iter()
            .position(|l| l.contains('┼'))
            .expect("header divider");
        assert!(
            table.len() - divider - 1 >= 2,
            "data row should wrap: {table:#?}"
        );
        assert!(table.iter().any(|l| l.contains("UUID")));
        assert!(
            table.iter().any(|l| l.ends_with("P1")),
            "right alignment lost: {table:#?}"
        );
        let link = doc
            .links()
            .iter()
            .find(|l| l.url == "https://a.test")
            .expect("table link");
        assert!(
            doc.visible_lines(link.line, 1)[0]
                .content()
                .contains("ast-cu9.15")
        );
    }

    #[test]
    fn test_csv_wraps_to_width() {
        let md = "```csv\nname,desc\nx,aaaa bbbb cccc dddd eeee ffff\n```";
        let doc = Document::parse_with_layout(md, 20).unwrap();
        let table = table_contents(&doc);
        for line in &table {
            assert!(UnicodeWidthStr::width(line.as_str()) <= 20, "{line:?}");
        }
        assert!(table.iter().any(|l| l.contains("ffff")));
    }

    #[test]
    fn test_gfm_table_with_emoji_preserves_cell_content() {
        let md =
            "| Feature | Status |\n|---|---|\n| Bold | ✅ Supported |\n| Italic | ✅ Supported |";
        let doc = Document::parse_with_layout(md, 28).unwrap();
        let lines = doc.visible_lines(0, 20);
        let table_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::Table)
            .collect();
        assert!(
            table_lines
                .iter()
                .any(|l| l.content().contains("✅ Supported"))
        );
    }

    #[test]
    fn test_gfm_table_mixed_content_renders_each_row_separately() {
        let md = "| Feature | Status | Notes |\n|---------|--------|-------|\n| **Bold** | ✅ Supported | Works well |\n| *Italic* | ✅ Supported | Works well |\n| `Code` | ✅ Supported | Inline only |\n| ~~Strike~~ | ✅ Supported | GFM extension |\n| [Links](/) | ✅ Supported | Full support |";
        let doc = Document::parse_with_layout(md, 120).unwrap();
        let table_lines: Vec<_> = doc
            .visible_lines(0, 100)
            .into_iter()
            .filter(|l| *l.line_type() == LineType::Table)
            .map(|l| l.content().to_string())
            .collect();

        assert_eq!(table_lines.len(), 7);
        assert!(table_lines.iter().all(|line| !line.contains('\n')));
        assert!(table_lines.iter().any(|line| line.contains("Bold")));
        assert!(table_lines.iter().any(|line| line.contains("Italic")));
        assert!(table_lines.iter().any(|line| line.contains("Code")));
        assert!(table_lines.iter().any(|line| line.contains("Strike")));
        assert!(table_lines.iter().any(|line| line.contains("Links")));
    }

    #[test]
    fn test_gfm_table_mixed_content_preserves_inline_styles_in_cells() {
        let md = "| Feature | Type |\n|---|---|\n| **Bold** | a |\n| *Italic* | b |\n| `Code` | c |\n| ~~Strike~~ | d |\n| [Links](/) | e |";
        let doc = Document::parse_with_layout(md, 120).unwrap();
        let table_rows: Vec<_> = doc
            .visible_lines(0, 50)
            .into_iter()
            .filter(|l| *l.line_type() == LineType::Table)
            .filter(|l| !l.content().contains('┼'))
            .collect();

        assert!(table_rows.iter().any(|row| {
            row.spans()
                .is_some_and(|spans| spans.iter().any(|s| s.style().strong))
        }));
        assert!(table_rows.iter().any(|row| {
            row.spans()
                .is_some_and(|spans| spans.iter().any(|s| s.style().emphasis))
        }));
        assert!(table_rows.iter().any(|row| {
            row.spans()
                .is_some_and(|spans| spans.iter().any(|s| s.style().code))
        }));
        assert!(table_rows.iter().any(|row| {
            row.spans()
                .is_some_and(|spans| spans.iter().any(|s| s.style().strikethrough))
        }));
        assert!(table_rows.iter().any(|row| {
            row.spans()
                .is_some_and(|spans| spans.iter().any(|s| s.style().link))
        }));
    }

    #[test]
    fn test_paragraph_wraps_to_width() {
        let md = "This is a long paragraph that should wrap at the specified width.";
        let doc = Document::parse_with_layout(md, 20).unwrap();
        let lines = doc.visible_lines(0, 100);

        let paragraph_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::Paragraph)
            .collect();

        assert!(paragraph_lines.len() > 1);
        for line in paragraph_lines {
            assert!(unicode_width::UnicodeWidthStr::width(line.content()) <= 20);
        }
    }

    #[test]
    fn test_inline_styles_create_spans() {
        let md = "*em* **strong** `code` [link](https://example.com) ~~strike~~";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        let paragraph = lines
            .iter()
            .find(|l| *l.line_type() == LineType::Paragraph)
            .expect("Paragraph line missing");
        let spans = paragraph.spans().expect("Inline spans missing");

        assert!(spans.iter().any(|s| s.style().emphasis));
        assert!(spans.iter().any(|s| s.style().strong));
        assert!(spans.iter().any(|s| s.style().code));
        assert!(spans.iter().any(|s| s.style().link));
        assert!(spans.iter().any(|s| s.style().strikethrough));
    }

    #[test]
    fn test_superscript_renders_with_unicode_digits() {
        let md = "E = mc^2^";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        let paragraph = lines
            .iter()
            .find(|l| *l.line_type() == LineType::Paragraph)
            .expect("Paragraph line missing");
        assert!(paragraph.content().contains("²"));
    }

    #[test]
    fn test_subscript_renders_with_unicode_digits() {
        let md = "H~2~O";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        let paragraph = lines
            .iter()
            .find(|l| *l.line_type() == LineType::Paragraph)
            .expect("Paragraph line missing");
        assert!(paragraph.content().contains("₂"));
    }

    #[test]
    fn test_subscript_falls_back_when_glyph_missing() {
        let md = "x~q~";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        let paragraph = lines
            .iter()
            .find(|l| *l.line_type() == LineType::Paragraph)
            .expect("Paragraph line missing");
        assert!(paragraph.content().contains("_(q)"));
    }

    #[test]
    fn test_superscript_letters_and_symbols_render_unicode() {
        let md = "x^abc+()^";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        let paragraph = lines
            .iter()
            .find(|l| *l.line_type() == LineType::Paragraph)
            .expect("Paragraph line missing");
        assert!(paragraph.content().contains("ᵃᵇᶜ⁺⁽⁾"));
    }

    #[test]
    fn test_footnote_reference_and_definition_render() {
        let md = "Alpha[^12]\n\n[^12]: Footnote text";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 20);
        assert!(lines.iter().any(|l| l.content().contains("Alpha¹²")));
        assert!(lines.iter().any(|l| l.content().contains("¹² ")));
        assert!(lines.iter().any(|l| l.content().contains("Footnote text")));
    }

    #[test]
    fn test_footnote_reference_falls_back_when_superscript_missing() {
        let md = "Alpha[^q]\n\n[^q]: Footnote text";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 20);
        assert!(lines.iter().any(|l| l.content().contains("[^q]")));
        assert!(
            lines
                .iter()
                .any(|l| l.content().contains("[^q] Footnote text"))
        );
    }

    #[test]
    fn test_footnote_reference_alpha_label_uses_plain_fallback() {
        let md = "Alpha[^n]\n\n[^n]: Footnote text";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 20);
        assert!(lines.iter().any(|l| l.content().contains("[^n]")));
        assert!(
            lines
                .iter()
                .any(|l| l.content().contains("[^n] Footnote text"))
        );
    }

    #[test]
    fn test_numeric_footnote_reference_renders_without_definition() {
        let md = "Here is a sentence with a footnote[^123].";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        assert!(lines.iter().any(|l| l.content().contains("footnote¹²³.")));
    }

    #[test]
    fn test_footnote_without_definition_still_creates_link_ref() {
        // When comrak emits [^1] as plain text (no definition),
        // the text fallback renders it as superscript but should still create a LinkRef
        let md = "Here is a sentence with a footnote[^1].";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let footnote_links: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url.starts_with("footnote:"))
            .collect();
        assert_eq!(
            footnote_links.len(),
            1,
            "text fallback should create a LinkRef for [^1]"
        );
        assert_eq!(footnote_links[0].text, "¹");
    }

    #[test]
    fn test_horizontal_rule_renders_subtle_line() {
        let md = "Alpha\n\n---\n\nBeta";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        assert!(lines.iter().any(|l| l.content() == "─────"));
    }

    #[test]
    fn test_image_caption_renders_only_when_image_height_known() {
        let md = "![Alt text](image.png)";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        assert!(!lines.iter().any(|l| l.content() == "Alt text"));

        let mut heights = HashMap::new();
        heights.insert("image.png".to_string(), 2);
        let doc = Document::parse_with_layout_and_image_heights(md, 80, &heights).unwrap();
        let lines = doc.visible_lines(0, 10);
        let image_idx = lines
            .iter()
            .position(|l| l.content().starts_with("[Image:"))
            .expect("image placeholder missing");
        assert!(image_idx > 0, "caption should be above image");
        assert_eq!(lines[image_idx - 1].content(), "    Alt text");
    }

    #[test]
    fn test_code_block_highlights_with_language() {
        let md = "```rust\nfn main() {}\n```";
        let mut doc = Document::parse_with_layout(md, 80).unwrap();
        doc.ensure_highlight_for_range(0..doc.line_count());
        let lines = doc.visible_lines(0, 10);
        let code_line = lines
            .iter()
            .find(|l| l.content().contains("fn main"))
            .expect("Code line missing");
        let spans = code_line.spans().expect("Expected code line spans");
        assert!(
            spans.iter().any(|s| s.style().fg.is_some()),
            "Expected highlighted code spans"
        );
    }

    #[test]
    fn test_code_block_is_plain_until_range_is_highlighted() {
        let md = "```rust\nfn main() {}\n```";
        let mut doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        let code_line = lines
            .iter()
            .find(|l| l.content().contains("fn main"))
            .expect("Code line missing");
        let spans = code_line.spans().expect("Expected code line spans");
        assert!(
            spans.iter().all(|s| s.style().fg.is_none()),
            "Expected plain code before lazy highlighting"
        );

        doc.ensure_highlight_for_range(0..doc.line_count());
        let lines = doc.visible_lines(0, 10);
        let code_line = lines
            .iter()
            .find(|l| l.content().contains("fn main"))
            .expect("Code line missing");
        let spans = code_line.spans().expect("Expected code line spans");
        assert!(spans.iter().any(|s| s.style().fg.is_some()));
    }

    #[test]
    fn test_strip_front_matter_delimiters_handles_unix_and_windows_endings() {
        assert_eq!(
            super::strip_front_matter_delimiters("---\nfoo: 1\nbar: 2\n---\n"),
            "foo: 1\nbar: 2"
        );
        assert_eq!(
            super::strip_front_matter_delimiters("---\r\nfoo: 1\r\nbar: 2\r\n---\r\n"),
            "foo: 1\r\nbar: 2"
        );
        // Missing trailing blank line (comrak doesn't always include one).
        assert_eq!(
            super::strip_front_matter_delimiters("---\nfoo: 1\n---"),
            "foo: 1"
        );
    }

    #[test]
    fn test_front_matter_renders_as_yaml_code_block() {
        // Issue #51: YAML front matter delimited by `---` was rendered as two
        // horizontal rules with the keys/values bleeding through as paragraph
        // text. It should render as a single framed YAML code block.
        let md = "---\ntitle: My Document\nauthor: Jane\n---\n\n# Body\n";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 20);

        // No horizontal rules from the `---` delimiters.
        assert!(
            !lines
                .iter()
                .any(|l| *l.line_type() == LineType::HorizontalRule),
            "front matter delimiters should not render as horizontal rules"
        );

        // Front matter content appears inside a framed code block.
        let code_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::CodeBlock)
            .collect();
        assert!(
            code_lines.iter().any(|l| l.content().contains("title:")),
            "front matter title key should appear inside the code block"
        );
        assert!(
            code_lines.iter().any(|l| l.content().contains("author:")),
            "front matter author key should appear inside the code block"
        );

        // Frame should be labeled `yaml` so the language is visible to the user.
        let top = code_lines
            .first()
            .expect("code block should have a top border");
        assert!(
            top.content().contains(" yaml "),
            "front matter frame should be labeled `yaml`; got {:?}",
            top.content()
        );

        // The body content should not leak as paragraph text (regression check
        // for the original bug where keys appeared verbatim in the document).
        assert!(
            !lines.iter().any(|l| {
                *l.line_type() == LineType::Paragraph && l.content().contains("title: My Document")
            }),
            "front matter keys should not appear as paragraph text"
        );

        // The body heading after the front matter should still render.
        assert!(
            lines
                .iter()
                .any(|l| *l.line_type() == LineType::Heading(1) && l.content().contains("Body")),
            "heading after front matter should still render"
        );
    }

    #[test]
    fn test_horizontal_rule_in_body_still_renders_as_thematic_break() {
        // Detection is positional: `---` only counts as a front-matter opener
        // when it's the very first line of the file. A horizontal rule that
        // happens to sit in the middle of a document must still render as one.
        let md = "# Heading\n\nSome prose.\n\n---\n\nMore prose.\n";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 30);

        assert!(
            lines
                .iter()
                .any(|l| *l.line_type() == LineType::HorizontalRule),
            "mid-document `---` should still render as a horizontal rule"
        );
        // Nothing should leak into a code block from a mid-document rule.
        assert!(
            !lines.iter().any(|l| *l.line_type() == LineType::CodeBlock),
            "a horizontal rule should not be misread as front matter"
        );
    }

    #[test]
    fn test_dashes_after_blank_line_are_not_front_matter() {
        // If the document doesn't start with `---` on line 1 (e.g. there's a
        // blank line first), comrak will not treat it as front matter.
        let md = "\n---\nfoo: bar\n---\n\n# Body\n";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 30);
        assert!(
            !lines
                .iter()
                .any(|l| l.content().contains(" yaml ") && l.content().starts_with('┌')),
            "delimiters not at line 1 should NOT be parsed as front matter"
        );
    }

    #[test]
    fn test_unclosed_front_matter_falls_back_to_normal_parsing() {
        // Without a matching closing `---`, comrak's split function returns
        // None and we fall back to the default (thematic break + paragraphs).
        let md = "---\nfoo: bar\n\n# Heading\n";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 30);
        assert!(
            !lines
                .iter()
                .any(|l| l.content().contains(" yaml ") && l.content().starts_with('┌')),
            "front matter without a closing delimiter should not be detected"
        );
        assert!(
            lines.iter().any(|l| *l.line_type() == LineType::Heading(1)),
            "the heading should still render in the fallback path"
        );
    }

    #[test]
    fn test_code_block_renders_without_fence_markers() {
        let md = "```rust\nfn main() {}\n```";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);

        assert!(!lines.iter().any(|l| l.content().starts_with("```")));
        assert!(lines.iter().any(|l| l.content().contains(" rust ")));
    }

    #[test]
    fn test_code_block_renders_ascii_box() {
        let md = "```rust\nfn main() {}\n```";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        let code_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::CodeBlock)
            .collect();

        assert!(code_lines.first().unwrap().content().starts_with('┌'));
        assert!(code_lines.first().unwrap().content().ends_with('┐'));
        assert!(code_lines.last().unwrap().content().starts_with('└'));
        assert!(code_lines.last().unwrap().content().ends_with('┘'));
        assert!(code_lines.iter().any(|l| l.content().starts_with("│ ")));
        let top_width =
            unicode_width::UnicodeWidthStr::width(code_lines.first().unwrap().content());
        for line in &code_lines {
            assert_eq!(
                unicode_width::UnicodeWidthStr::width(line.content()),
                top_width
            );
        }
    }

    #[test]
    fn test_code_block_uses_max_width_not_wrap_width() {
        // Regression for #55: a code block containing a line wider than the
        // user's --wrap-width should NOT be truncated. wrap_width controls
        // prose wrapping; code blocks should preserve their natural width up
        // to the terminal's available area.
        let long_line = "x".repeat(100);
        let md = format!("```\n{long_line}\n```");
        let doc = Document::parse_with_widths(&md, 80, 120).unwrap();
        let lines = doc.visible_lines(0, 10);
        let body_line = lines
            .iter()
            .find(|l| l.content().contains("xx"))
            .expect("code body line missing");
        let content = body_line.content();
        let stripped = content
            .strip_prefix("│ ")
            .and_then(|s| s.strip_suffix(" │"))
            .map(str::trim_end)
            .expect("code line should have │ borders");
        let x_count = stripped.chars().filter(|c| *c == 'x').count();
        assert_eq!(
            x_count, 100,
            "expected all 100 x's preserved; got {x_count} in {stripped:?}"
        );
    }

    #[test]
    fn test_code_block_clips_to_max_width_when_line_exceeds_terminal() {
        // When even the terminal width can't fit the line, content is clipped
        // to max_width - 2 (just leaving room for the left `│ `). The right
        // border and trailing padding overflow past max_width and get
        // clipped by the terminal; the missing right `│` is the visual cue.
        let long_line = "x".repeat(200);
        let md = format!("```\n{long_line}\n```");
        let doc = Document::parse_with_widths(&md, 80, 100).unwrap();
        let lines = doc.visible_lines(0, 10);
        let body_line = lines
            .iter()
            .find(|l| l.content().contains("xx"))
            .expect("code body line missing");
        let stripped = body_line
            .content()
            .strip_prefix("│ ")
            .and_then(|s| s.strip_suffix(" │"))
            .map(str::trim_end)
            .expect("code line should have │ borders");
        let x_count = stripped.chars().filter(|c| *c == 'x').count();
        assert_eq!(
            x_count, 98,
            "expected line truncated to max_width-2=98; got {x_count}"
        );
    }

    #[test]
    fn test_code_block_has_right_padding_inside_frame() {
        let md = "```rust\nlet x = 1;\n```";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        let code_line = lines
            .iter()
            .find(|l| l.content().contains("let x = 1;"))
            .expect("code line missing");
        assert!(
            code_line.content().contains("   │"),
            "expected at least a few spaces of right padding before border"
        );
    }

    #[test]
    fn test_ordered_list_marker() {
        let md = "1. First item\n2. Second item";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        let list_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::ListItem(1))
            .collect();

        assert!(list_lines[0].content().starts_with("1. "));
        assert!(list_lines[1].content().starts_with("2. "));
    }

    #[test]
    fn test_list_wraps_with_hanging_indent() {
        let md = "1. This is a long list item that should wrap to the next line.";
        let doc = Document::parse_with_layout(md, 20).unwrap();
        let lines = doc.visible_lines(0, 10);
        let list_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::ListItem(1))
            .collect();

        assert!(list_lines.len() > 1);
        assert!(list_lines[0].content().starts_with("1. "));
        assert!(list_lines[1].content().starts_with("   "));
    }

    #[test]
    fn test_unordered_list_uses_bullet_character() {
        let md = "* Item";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        let list_line = lines
            .iter()
            .find(|l| *l.line_type() == LineType::ListItem(1))
            .expect("List line missing");

        assert!(list_line.content().starts_with("• "));
    }

    #[test]
    fn test_nested_list_indents_children() {
        let md = "- Parent\n  - Child";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        let list_lines: Vec<_> = lines
            .iter()
            .filter(|l| matches!(l.line_type(), LineType::ListItem(_)))
            .collect();

        assert!(list_lines[0].content().starts_with("• "));
        assert!(list_lines[1].content().starts_with("  • "));
    }

    #[test]
    fn test_list_item_with_multiple_paragraphs_has_blank_line() {
        let md = "- First paragraph\n\n  Second paragraph";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        let list_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::ListItem(1))
            .collect();

        assert!(list_lines.len() >= 3);
        assert_eq!(list_lines[1].content(), "");
    }

    #[test]
    fn test_list_has_trailing_blank_line() {
        let md = "- One\n- Two\n\nAfter";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 20);
        let after_idx = lines
            .iter()
            .position(|l| l.content().contains("After"))
            .expect("After line missing");
        assert!(after_idx > 0);
        assert_eq!(lines[after_idx - 1].content(), "");
    }

    #[test]
    fn test_task_list_marker() {
        let md = "- [x] Done\n- [ ] Todo";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        let list_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::ListItem(1))
            .collect();

        assert!(list_lines[0].content().starts_with("✓ "));
        assert!(list_lines[1].content().starts_with("□ "));
    }

    #[test]
    fn test_ordered_list_alignment_for_two_digits() {
        let md = "9. Ninth\n10. Tenth";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        let list_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::ListItem(1))
            .collect();

        assert!(list_lines[0].content().starts_with(" 9. "));
        assert!(list_lines[1].content().starts_with("10. "));
    }

    #[test]
    fn test_nested_task_list_markers_indent() {
        let md = "- [x] Parent\n  - [ ] Child";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        let list_lines: Vec<_> = lines
            .iter()
            .filter(|l| matches!(l.line_type(), LineType::ListItem(_)))
            .collect();

        assert!(list_lines[0].content().starts_with("✓ "));
        assert!(list_lines[1].content().starts_with("  □ "));
    }

    #[test]
    fn test_task_list_parent_does_not_inline_children() {
        let md = "- [x] Main task completed\n  - [x] Subtask 1 done\n  - [ ] Subtask 2 pending";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 10);
        let list_lines: Vec<_> = lines
            .iter()
            .filter(|l| matches!(l.line_type(), LineType::ListItem(_)))
            .collect();

        assert!(list_lines[0].content().contains("Main task completed"));
        assert!(!list_lines[0].content().contains("Subtask"));
    }

    #[test]
    fn test_csv_code_block_renders_as_table() {
        let md = "```csv\nName,Age,City\nAlice,30,NYC\nBob,25,LA\n```";
        let doc = parse(md).unwrap();
        let lines = doc.visible_lines(0, 20);
        let table_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::Table)
            .collect();
        assert!(
            !table_lines.is_empty(),
            "CSV block should render as Table lines"
        );
        assert!(
            table_lines
                .iter()
                .any(|l| l.content().contains("Name") && l.content().contains("Age")),
            "Header row should contain column names"
        );
        assert!(
            table_lines
                .iter()
                .any(|l| l.content().contains("Alice") && l.content().contains("30")),
            "Data rows should be present"
        );
    }

    #[test]
    fn test_csv_code_block_has_header_divider() {
        let md = "```csv\nA,B\n1,2\n```";
        let doc = parse(md).unwrap();
        let lines = doc.visible_lines(0, 10);
        let table_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::Table)
            .collect();
        assert!(
            table_lines.iter().any(|l| l.content().contains("┼")),
            "CSV table should have a header divider with ┼"
        );
    }

    #[test]
    fn test_csv_code_block_not_rendered_as_code_block() {
        let md = "```csv\nX,Y\n1,2\n```";
        let doc = parse(md).unwrap();
        let lines = doc.visible_lines(0, 10);
        let code_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::CodeBlock)
            .collect();
        assert!(
            code_lines.is_empty(),
            "CSV block should not render as CodeBlock lines"
        );
    }

    #[test]
    fn test_csv_code_block_preserves_cell_content() {
        let md =
            "```csv\nVery Long Column Name,Another Long Name\nsome really long content,12345\n```";
        let doc = Document::parse_with_layout(md, 30).unwrap();
        let table = table_contents(&doc);
        for line in &table {
            assert!(UnicodeWidthStr::width(line.as_str()) <= 30, "{line:?}");
        }
        for word in [
            "Very", "Long", "Column", "Name", "Another", "some", "really", "long", "content",
            "12345",
        ] {
            assert!(
                table.iter().any(|l| l.contains(word)),
                "missing {word:?} in table"
            );
        }
    }

    #[test]
    fn test_csv_with_quoted_fields() {
        let md = "```csv\nName,Description\nAlice,\"Has a, comma\"\nBob,\"Simple\"\n```";
        let doc = parse(md).unwrap();
        let lines = doc.visible_lines(0, 20);
        let table_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::Table)
            .collect();
        assert!(
            table_lines
                .iter()
                .any(|l| l.content().contains("Has a, comma")),
            "Quoted CSV fields with commas should be handled correctly"
        );
    }

    #[test]
    fn test_csv_empty_block_renders_nothing() {
        let md = "```csv\n```";
        let doc = parse(md).unwrap();
        let lines = doc.visible_lines(0, 10);
        let table_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::Table)
            .collect();
        assert!(
            table_lines.is_empty(),
            "Empty CSV block should not render table lines"
        );
    }

    #[test]
    fn test_csv_single_column() {
        let md = "```csv\nName\nAlice\nBob\n```";
        let doc = parse(md).unwrap();
        let lines = doc.visible_lines(0, 10);
        let table_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::Table)
            .collect();
        assert!(
            !table_lines.is_empty(),
            "Single-column CSV should render as table"
        );
        assert!(
            table_lines.iter().any(|l| l.content().contains("Name")),
            "Header should be present"
        );
        assert!(
            table_lines.iter().any(|l| l.content().contains("Alice")),
            "Data rows should be present"
        );
    }

    #[test]
    fn test_csv_large_file_renders_all_rows() {
        let mut md = String::from("```csv\nID,Value\n");
        for i in 0..200 {
            md.push_str(&format!("{},{}\n", i, i * 10));
        }
        md.push_str("```");
        let doc = parse(&md).unwrap();
        let lines = doc.visible_lines(0, 500);
        let data_rows: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::Table && !l.content().contains('┼'))
            .collect();
        // 1 header + 200 data rows = 201 total non-divider table lines
        assert_eq!(data_rows.len(), 201, "All CSV rows should be rendered");
    }

    #[test]
    fn test_csv_large_file_pages_via_viewport() {
        let mut md = String::from("```csv\nID,Value\n");
        for i in 0..200 {
            md.push_str(&format!("{},{}\n", i, i * 10));
        }
        md.push_str("```");
        let doc = parse(&md).unwrap();
        // Simulate viewport paging: first page
        let page1 = doc.visible_lines(0, 10);
        assert_eq!(page1.len(), 10);
        // Later page
        let page2 = doc.visible_lines(100, 10);
        assert_eq!(page2.len(), 10);
        assert_ne!(page1[0].content(), page2[0].content());
    }

    #[test]
    fn test_non_csv_code_block_unchanged() {
        let md = "```rust\nfn main() {}\n```";
        let doc = parse(md).unwrap();
        let lines = doc.visible_lines(0, 10);
        let code_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::CodeBlock)
            .collect();
        assert!(
            !code_lines.is_empty(),
            "Non-CSV code blocks should still render as CodeBlock"
        );
    }

    #[test]
    fn test_mermaid_code_block_stored_as_mermaid_source() {
        let md = "```mermaid\ngraph TD\n    A --> B\n```";
        let doc = parse(md).unwrap();
        assert_eq!(doc.mermaid_sources().len(), 1);
        let source = doc.mermaid_sources().values().next().unwrap();
        assert!(source.contains("graph TD"));
        assert!(source.contains("A --> B"));
    }

    #[test]
    fn test_non_mermaid_code_block_not_in_mermaid_sources() {
        let md = "```rust\nfn main() {}\n```";
        let doc = parse(md).unwrap();
        assert!(doc.mermaid_sources().is_empty());
    }

    #[test]
    fn test_mermaid_block_renders_as_image_when_flag_set() {
        let md = "```mermaid\ngraph TD\n    A --> B\n```";
        let doc = Document::parse_with_mermaid_images(md, 80).unwrap();
        assert_eq!(doc.images().len(), 1);
        assert!(doc.images()[0].src.starts_with("mermaid://"));
        // Should still have the source stored
        assert_eq!(doc.mermaid_sources().len(), 1);
    }

    #[test]
    fn test_mermaid_block_stays_as_code_without_flag() {
        let md = "```mermaid\ngraph TD\n    A --> B\n```";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        // No image entries for mermaid without the flag
        assert!(doc.images().is_empty());
        // Still stored as mermaid source
        assert_eq!(doc.mermaid_sources().len(), 1);
        // Rendered as code block lines
        let lines = doc.visible_lines(0, 20);
        assert!(lines.iter().any(|l| *l.line_type() == LineType::CodeBlock));
    }

    #[test]
    fn test_mermaid_image_placeholder_text() {
        let md = "```mermaid\ngraph TD\n    A --> B\n```";
        let doc = Document::parse_with_mermaid_images(md, 80).unwrap();
        let lines = doc.visible_lines(0, 20);
        assert!(
            lines
                .iter()
                .any(|l| l.content().contains("[Image: mermaid diagram]"))
        );
    }

    #[test]
    fn test_mermaid_block_falls_back_to_code_when_in_failed_set() {
        let md = "```mermaid\ngraph TD\n    A --> B\n```";
        let mermaid_source = "graph TD\n    A --> B";
        let mut failed = HashSet::new();
        failed.insert(mermaid_source.to_string());
        let doc = Document::parse_with_all_options_and_failures(
            md,
            80,
            &HashMap::new(),
            &DiagramRenderOpts {
                mermaid_as_images: true,
                failed_mermaid_srcs: &failed,
                ..DiagramRenderOpts::default()
            },
        )
        .unwrap();
        // Should NOT have an image placeholder
        assert!(doc.images().is_empty());
        // Should render as code block lines
        let lines = doc.visible_lines(0, 20);
        assert!(lines.iter().any(|l| *l.line_type() == LineType::CodeBlock));
        // Should still store the mermaid source
        assert_eq!(doc.mermaid_sources().len(), 1);
    }

    #[test]
    fn test_mermaid_block_renders_as_image_when_not_in_failed_set() {
        let md = "```mermaid\ngraph TD\n    A --> B\n```";
        let failed = HashSet::new(); // empty set
        let doc = Document::parse_with_all_options_and_failures(
            md,
            80,
            &HashMap::new(),
            &DiagramRenderOpts {
                mermaid_as_images: true,
                failed_mermaid_srcs: &failed,
                ..DiagramRenderOpts::default()
            },
        )
        .unwrap();
        // Should have an image placeholder
        assert_eq!(doc.images().len(), 1);
        assert!(doc.images()[0].src.starts_with("mermaid://"));
    }

    #[test]
    fn test_numeric_footnote_reference_link_uses_rendered_text() {
        let md = "See this[^1].\n\n[^1]: A footnote.";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        // The rendered text uses superscript "¹", so the LinkRef text must match
        let footnote_links: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url.starts_with("footnote:"))
            .collect();
        assert!(!footnote_links.is_empty(), "should have a footnote link");
        let link = &footnote_links[0];
        assert_eq!(
            link.text, "¹",
            "LinkRef text should be the rendered superscript, not [^1]"
        );
    }

    #[test]
    fn test_footnote_definition_contains_link_refs() {
        let md = "Alpha[^1]\n\n[^1]: See [example](https://example.com) for details.";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let url_links: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url == "https://example.com")
            .collect();
        assert_eq!(
            url_links.len(),
            1,
            "link inside footnote definition should be registered"
        );
        assert_eq!(url_links[0].text, "example");
    }

    #[test]
    fn test_list_item_contains_link_refs() {
        let md = "- Visit [Rust](https://rust-lang.org) for more\n- No link here";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let links: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url == "https://rust-lang.org")
            .collect();
        assert_eq!(links.len(), 1, "link inside list item should be registered");
        assert_eq!(links[0].text, "Rust");
        // The link line should match a line that actually contains "Rust"
        let line_content = doc.line_at(links[0].line).unwrap().content();
        assert!(
            line_content.contains("Rust"),
            "link line {} should contain 'Rust', got: {line_content}",
            links[0].line
        );
    }

    #[test]
    fn test_blockquote_contains_link_refs() {
        let md = "> Check [docs](https://docs.rs) for details";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let links: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url == "https://docs.rs")
            .collect();
        assert_eq!(
            links.len(),
            1,
            "link inside blockquote should be registered"
        );
        assert_eq!(links[0].text, "docs");
        let line_content = doc.line_at(links[0].line).unwrap().content();
        assert!(
            line_content.contains("docs"),
            "link line {} should contain 'docs', got: {line_content}",
            links[0].line
        );
    }

    #[test]
    fn test_table_contains_link_refs() {
        let md = "| Name | Link |\n|------|------|\n| Foo | [Bar](https://bar.com) |";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let links: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url == "https://bar.com")
            .collect();
        assert_eq!(
            links.len(),
            1,
            "link inside table cell should be registered"
        );
        assert_eq!(links[0].text, "Bar");
        let line_content = doc.line_at(links[0].line).unwrap().content();
        assert!(
            line_content.contains("Bar"),
            "link line {} should contain 'Bar', got: {line_content}",
            links[0].line
        );
    }

    #[test]
    fn test_task_item_contains_link_refs() {
        let md = "- [ ] Read [guide](https://guide.com)\n- [x] Done";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let links: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url == "https://guide.com")
            .collect();
        assert_eq!(links.len(), 1, "link inside task item should be registered");
        assert_eq!(links[0].text, "guide");
        let line_content = doc.line_at(links[0].line).unwrap().content();
        assert!(
            line_content.contains("guide"),
            "link line {} should contain 'guide', got: {line_content}",
            links[0].line
        );
    }

    #[test]
    fn test_heading_contains_link_refs() {
        let md = "# Title with [link](https://heading.com)";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let links: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url == "https://heading.com")
            .collect();
        assert_eq!(links.len(), 1, "link inside heading should be registered");
        assert_eq!(links[0].text, "link");
        let line_content = doc.line_at(links[0].line).unwrap().content();
        assert!(
            line_content.contains("link"),
            "link line {} should contain 'link', got: {line_content}",
            links[0].line
        );
    }

    #[test]
    fn test_code_styled_link_text_uses_rendered_label_for_line_fixup() {
        let md = "See [`fixes/README.md`](fixes/README.md) for details.";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let link = doc
            .links()
            .iter()
            .find(|l| l.url == "fixes/README.md")
            .expect("link should be collected");

        assert_eq!(
            link.text, "fixes/README.md",
            "link text should match rendered label without markdown backticks"
        );
        let line_content = doc.line_at(link.line).unwrap().content();
        assert!(
            line_content.contains("fixes/README.md"),
            "link line {} should contain rendered text, got: {line_content}",
            link.line
        );
    }

    #[test]
    fn test_nested_blockquote_contains_link_refs() {
        let md = "> > See [inner](https://inner.com)";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let links: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url == "https://inner.com")
            .collect();
        assert_eq!(
            links.len(),
            1,
            "link inside nested blockquote should be registered"
        );
        assert_eq!(links[0].text, "inner");
    }

    // Issue 2: fixup_link_lines with duplicate link text
    #[test]
    fn test_duplicate_link_text_assigned_to_correct_wrapped_lines() {
        // Two links both labelled "here" in a paragraph, narrow width forces
        // the second "here" onto a different wrapped line.
        let md = "Click [here](https://first.com) for the first thing, then click [here](https://second.com) for second.";
        let doc = Document::parse_with_layout(md, 35).unwrap();
        let first: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url == "https://first.com")
            .collect();
        let second: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url == "https://second.com")
            .collect();
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);

        // Verify they actually ended up on different rendered lines
        // (the wrap at 35 chars should separate them)
        let line_first = doc.line_at(first[0].line).unwrap().content();
        let line_second = doc.line_at(second[0].line).unwrap().content();

        // Both lines must contain the link text
        assert!(
            line_first.contains("here"),
            "first link at line {} should contain 'here': {line_first:?}",
            first[0].line
        );
        assert!(
            line_second.contains("here"),
            "second link at line {} should contain 'here': {line_second:?}",
            second[0].line
        );

        // The second link must NOT be assigned to the same line as the first
        // if the text is actually on a different rendered line
        assert_ne!(
            first[0].line, second[0].line,
            "duplicate 'here' links should be on different lines after wrapping \
             (first line: {line_first:?}, second line: {line_second:?})"
        );
    }

    // Issue: link text wrapping across lines should create clickable regions on each line
    #[test]
    fn test_wrapped_link_text_creates_link_refs_on_each_line() {
        // A link whose text is long enough to wrap across two rendered lines.
        // At width 25, "click here for more details" should wrap.
        let md = "Go [click here for more details](https://example.com) now.";
        let doc = Document::parse_with_layout(md, 25).unwrap();
        let links: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url == "https://example.com")
            .collect();

        // There should be link refs on BOTH wrapped lines so clicking
        // either line follows the link.
        assert!(
            links.len() >= 2,
            "wrapped link should produce link refs on each spanned line, got {} link(s): {:?}",
            links.len(),
            links
        );

        // Each link ref's text should be found on its respective line
        for link in &links {
            let line_content = doc.line_at(link.line).unwrap().content();
            assert!(
                line_content.contains(&link.text),
                "link text {:?} should appear on line {} ({:?})",
                link.text,
                link.line,
                line_content
            );
        }

        // The links should be on different lines
        let lines: Vec<_> = links.iter().map(|l| l.line).collect();
        let unique_lines: std::collections::HashSet<_> = lines.iter().collect();
        assert!(
            unique_lines.len() >= 2,
            "wrapped link refs should span multiple lines, got lines: {:?}",
            lines
        );
    }

    // Issue 3: links in lists inside blockquotes
    #[test]
    fn test_blockquote_list_contains_link_refs() {
        let md = "> - Visit [Rust](https://rust-lang.org) today";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let links: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url == "https://rust-lang.org")
            .collect();
        assert_eq!(
            links.len(),
            1,
            "link inside list in blockquote should be registered"
        );
        assert_eq!(links[0].text, "Rust");
    }

    #[test]
    fn test_wrapped_link_in_blockquote_is_clickable_on_each_line() {
        // A link inside a blockquote whose text wraps across two lines.
        // The blockquote prefix ("  │ ") must not break link detection.
        let md = "> Visit [click here for more details](https://example.com) for info.";
        let doc = Document::parse_with_layout(md, 30).unwrap();
        let links: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url == "https://example.com")
            .collect();

        assert!(
            links.len() >= 2,
            "wrapped link in blockquote should produce link refs on each spanned line, \
             got {} link(s): {:?}",
            links.len(),
            links
        );

        for link in &links {
            let line_content = doc.line_at(link.line).unwrap().content();
            assert!(
                line_content.contains(&link.text),
                "link text {:?} should appear on line {} ({:?})",
                link.text,
                link.line,
                line_content
            );
        }
    }

    #[test]
    fn test_wrapped_link_spanning_three_lines() {
        // A very long link text that wraps across 3 lines at narrow width.
        let md = "A [one two three four five six seven eight](https://example.com) end.";
        let doc = Document::parse_with_layout(md, 15).unwrap();
        let links: Vec<_> = doc
            .links()
            .iter()
            .filter(|l| l.url == "https://example.com")
            .collect();

        assert!(
            links.len() >= 3,
            "link spanning 3+ lines should produce 3+ link refs, got {}: {:?}",
            links.len(),
            links
        );

        for link in &links {
            let line_content = doc.line_at(link.line).unwrap().content();
            assert!(
                line_content.contains(&link.text),
                "link text {:?} should appear on line {} ({:?})",
                link.text,
                link.line,
                line_content
            );
        }
    }

    #[test]
    fn test_html_img_tag_renders_as_image_placeholder() {
        let md = r#"<img src="photo.png" alt="A photo">"#;
        let doc = Document::parse(md).unwrap();
        let image_lines: Vec<_> = (0..doc.line_count())
            .filter_map(|i| doc.line_at(i))
            .filter(|l| *l.line_type() == LineType::Image)
            .collect();
        assert!(
            !image_lines.is_empty(),
            "HTML <img> tag should produce Image lines"
        );
        assert!(
            image_lines[0].content().contains("[Image:"),
            "Image placeholder should contain [Image: prefix, got: {}",
            image_lines[0].content()
        );
    }

    #[test]
    fn test_html_img_tag_populates_image_refs() {
        let md = r#"<img src="photo.png" alt="A photo">"#;
        let doc = Document::parse(md).unwrap();
        assert!(
            !doc.images().is_empty(),
            "HTML <img> tag should register an image ref"
        );
        assert_eq!(doc.images()[0].src, "photo.png");
        assert_eq!(doc.images()[0].alt, "A photo");
    }

    #[test]
    fn test_html_img_without_alt_uses_src() {
        let md = r#"<img src="diagram.svg">"#;
        let doc = Document::parse(md).unwrap();
        let image_lines: Vec<_> = (0..doc.line_count())
            .filter_map(|i| doc.line_at(i))
            .filter(|l| *l.line_type() == LineType::Image)
            .collect();
        assert!(!image_lines.is_empty());
        assert!(
            image_lines[0].content().contains("diagram.svg"),
            "Should use src as fallback label"
        );
    }

    #[test]
    fn test_html_img_centered_with_div() {
        let md = r#"<div align="center"><img src="logo.png" alt="Logo"></div>"#;
        let doc = Document::parse(md).unwrap();
        assert!(
            !doc.images().is_empty(),
            "Centered HTML <img> inside <div> should register an image ref"
        );
        assert_eq!(doc.images()[0].src, "logo.png");
    }

    #[test]
    fn test_extract_html_images_basic() {
        let results = extract_html_images(r#"<img src="a.png" alt="desc">"#);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "a.png");
        assert_eq!(results[0].1, "desc");
    }

    #[test]
    fn test_extract_html_images_no_alt() {
        let results = extract_html_images(r#"<img src="b.png">"#);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "b.png");
        assert_eq!(results[0].1, "");
    }

    #[test]
    fn test_extract_html_images_single_quotes() {
        let results = extract_html_images("<img src='c.png' alt='hello'>");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "c.png");
        assert_eq!(results[0].1, "hello");
    }

    #[test]
    fn test_extract_html_images_multiple() {
        let html = r#"<img src="a.png"><br><img src="b.png" alt="B">"#;
        let results = extract_html_images(html);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "a.png");
        assert_eq!(results[1].0, "b.png");
        assert_eq!(results[1].1, "B");
    }

    #[test]
    fn test_extract_html_images_self_closing() {
        let results = extract_html_images(r#"<img src="d.png" />"#);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "d.png");
    }

    #[test]
    fn test_extract_html_attr_missing() {
        assert!(extract_html_attr(r#"<img src="a.png">"#, "alt").is_none());
    }

    #[test]
    fn test_extract_html_no_img_tag() {
        let results = extract_html_images(r#"<div class="foo">hello</div>"#);
        assert!(results.is_empty());
    }

    /// Test the exact example from issue #11: an `<img>` nested inside a
    /// styled `<div>` block, which is a common pattern in GitHub READMEs.
    #[test]
    fn test_html_img_in_styled_div_issue_11() {
        let md = r#"<div style="text-align: center;">
  <img src="Emacs-rust-eglot-markdown.png" height=800>
</div>"#;
        let doc = Document::parse(md).unwrap();
        assert!(
            !doc.images().is_empty(),
            "Should extract <img> nested inside a styled <div>"
        );
        assert_eq!(doc.images()[0].src, "Emacs-rust-eglot-markdown.png");

        let image_lines: Vec<_> = (0..doc.line_count())
            .filter_map(|i| doc.line_at(i))
            .filter(|l| *l.line_type() == LineType::Image)
            .collect();
        assert!(
            !image_lines.is_empty(),
            "Should render an image placeholder line"
        );
        assert!(
            image_lines[0]
                .content()
                .contains("Emacs-rust-eglot-markdown.png")
        );
    }

    // --- Math tests ---

    #[test]
    fn test_inline_math_produces_styled_span() {
        let md = "The value $x^2$ is positive.";
        let doc = Document::parse(md).unwrap();
        let lines = doc.visible_lines(0, 20);
        // Should find a span with math: true
        let has_math_span = lines.iter().any(|l| {
            l.spans()
                .is_some_and(|spans| spans.iter().any(|s| s.style().math))
        });
        assert!(
            has_math_span,
            "inline $x^2$ should produce a math-styled span"
        );
    }

    #[test]
    fn test_inline_math_unicode_conversion() {
        let md = r"The value $\alpha$ is greek.";
        let doc = Document::parse(md).unwrap();
        let lines = doc.visible_lines(0, 20);
        let text: String = lines.iter().map(|l| l.content().to_string()).collect();
        assert!(
            text.contains('α'),
            "inline \\alpha should be converted to Unicode α, got: {text}"
        );
    }

    #[test]
    fn test_display_math_renders_as_image_when_flag_set() {
        let md = "$$E = mc^2$$";
        let doc = Document::parse_with_all_options_and_failures(
            md,
            80,
            &HashMap::new(),
            &DiagramRenderOpts {
                math_as_images: true,
                ..DiagramRenderOpts::default()
            },
        )
        .unwrap();
        assert!(
            !doc.images().is_empty(),
            "display math with math_as_images=true should produce ImageRef"
        );
        assert!(doc.images()[0].src.starts_with("math://"));
    }

    #[test]
    fn test_display_math_renders_as_text_block_without_flag() {
        let md = "$$E = mc^2$$";
        let doc = Document::parse(md).unwrap();
        let lines = doc.visible_lines(0, 20);
        let has_math_line = lines.iter().any(|l| *l.line_type() == LineType::Math);
        assert!(
            has_math_line,
            "display math without flag should produce LineType::Math lines"
        );
    }

    #[test]
    fn test_math_code_fence_as_image() {
        let md = "```math\nE = mc^2\n```";
        let doc = Document::parse_with_all_options_and_failures(
            md,
            80,
            &HashMap::new(),
            &DiagramRenderOpts {
                math_as_images: true,
                ..DiagramRenderOpts::default()
            },
        )
        .unwrap();
        assert!(
            !doc.images().is_empty(),
            "```math with flag should produce ImageRef"
        );
        assert!(doc.images()[0].src.starts_with("math://"));
    }

    #[test]
    fn test_math_code_fence_as_text() {
        let md = "```math\nE = mc^2\n```";
        let doc = Document::parse(md).unwrap();
        let lines = doc.visible_lines(0, 20);
        let has_math_line = lines.iter().any(|l| *l.line_type() == LineType::Math);
        assert!(
            has_math_line,
            "```math without flag should produce LineType::Math lines"
        );
    }

    #[test]
    fn test_math_code_inline() {
        // $`x^2`$ is "code math" syntax
        let md = "The value $`x^2`$ is positive.";
        let doc = Document::parse(md).unwrap();
        let lines = doc.visible_lines(0, 20);
        let has_math_span = lines.iter().any(|l| {
            l.spans()
                .is_some_and(|spans| spans.iter().any(|s| s.style().math))
        });
        assert!(
            has_math_span,
            "code math $`x^2`$ should produce a math-styled span"
        );
    }

    #[test]
    fn test_math_in_heading_text_extraction() {
        let md = "# Title $x$";
        let doc = Document::parse(md).unwrap();
        let headings = doc.headings();
        assert!(!headings.is_empty());
        assert!(
            headings[0].text.contains('x'),
            "heading text should include math content"
        );
    }

    #[test]
    fn test_math_source_stored() {
        let md = "$$E = mc^2$$";
        let doc = Document::parse_with_all_options_and_failures(
            md,
            80,
            &HashMap::new(),
            &DiagramRenderOpts {
                math_as_images: true,
                ..DiagramRenderOpts::default()
            },
        )
        .unwrap();
        assert!(
            !doc.math_sources().is_empty(),
            "math_sources should have an entry for display math"
        );
    }

    #[test]
    fn test_math_falls_back_when_in_failed_set() {
        let md = "$$E = mc^2$$";
        let mut failed = HashSet::new();
        failed.insert("E = mc^2".to_string());
        let doc = Document::parse_with_all_options_and_failures(
            md,
            80,
            &HashMap::new(),
            &DiagramRenderOpts {
                math_as_images: true,
                failed_math_srcs: &failed,
                ..DiagramRenderOpts::default()
            },
        )
        .unwrap();
        // Should NOT have an image placeholder since source is in failed set
        assert!(
            doc.images().is_empty(),
            "failed math source should not produce ImageRef"
        );
        // Should still have LineType::Math text block
        let lines = doc.visible_lines(0, 20);
        let has_math_line = lines.iter().any(|l| *l.line_type() == LineType::Math);
        assert!(
            has_math_line,
            "failed math should fall through to text block"
        );
    }

    #[test]
    fn test_inline_math_as_image_splits_paragraph() {
        // Paragraph with inline math should split into text + image + text
        let md = "Before $x^2$ after.";
        let doc = Document::parse_with_all_options_and_failures(
            md,
            80,
            &HashMap::new(),
            &DiagramRenderOpts {
                math_as_images: true,
                no_inline_math: true,
                ..DiagramRenderOpts::default()
            },
        )
        .unwrap();
        // Should have an image placeholder for the inline math
        assert!(
            !doc.images().is_empty(),
            "inline math with no_inline_math=true should produce ImageRef"
        );
        assert!(doc.images()[0].src.starts_with("math://"));
        // Should have paragraph lines for text before and after the math
        let lines = doc.visible_lines(0, 20);
        let para_lines: Vec<_> = lines
            .iter()
            .filter(|l| *l.line_type() == LineType::Paragraph)
            .collect();
        assert!(
            para_lines.len() >= 2,
            "should have paragraph lines for text before and after math, got {}",
            para_lines.len()
        );
    }

    #[test]
    fn test_inline_math_as_image_disabled_uses_unicode() {
        // With no_inline_math=false, inline math should remain as Unicode text
        let md = r"The value $\alpha$ is greek.";
        let doc = Document::parse_with_all_options_and_failures(
            md,
            80,
            &HashMap::new(),
            &DiagramRenderOpts {
                math_as_images: true,
                ..DiagramRenderOpts::default()
            },
        )
        .unwrap();
        // Should NOT have any image placeholders
        assert!(
            doc.images().is_empty(),
            "inline math with no_inline_math=false should not produce ImageRef"
        );
        // Should have inline Unicode text
        let lines = doc.visible_lines(0, 20);
        let text: String = lines.iter().map(|l| l.content().to_string()).collect();
        assert!(
            text.contains('α'),
            "inline math should be Unicode text when no_inline_math=false, got: {text}"
        );
    }

    #[test]
    fn test_inline_math_as_image_in_list() {
        let md = "- Item with $x^2$ math";
        let doc = Document::parse_with_all_options_and_failures(
            md,
            80,
            &HashMap::new(),
            &DiagramRenderOpts {
                math_as_images: true,
                no_inline_math: true,
                ..DiagramRenderOpts::default()
            },
        )
        .unwrap();
        assert!(
            !doc.images().is_empty(),
            "inline math in list item with no_inline_math=true should produce ImageRef"
        );
        assert!(doc.images()[0].src.starts_with("math://"));
    }

    #[test]
    fn test_inline_math_multiple_in_paragraph() {
        let md = "First $a$ then $b$ last.";
        let doc = Document::parse_with_all_options_and_failures(
            md,
            80,
            &HashMap::new(),
            &DiagramRenderOpts {
                math_as_images: true,
                no_inline_math: true,
                ..DiagramRenderOpts::default()
            },
        )
        .unwrap();
        assert_eq!(
            doc.images().len(),
            2,
            "two inline math expressions should produce two ImageRefs, got {}",
            doc.images().len()
        );
        assert!(doc.images()[0].src.starts_with("math://"));
        assert!(doc.images()[1].src.starts_with("math://"));
    }

    #[test]
    fn test_inline_math_as_image_preserves_links() {
        // Hyperlinks in paragraphs with inline math must remain clickable.
        let md = "See [the paper](https://example.com) for proof that $x^2 > 0$.";
        let doc = Document::parse_with_all_options_and_failures(
            md,
            80,
            &HashMap::new(),
            &DiagramRenderOpts {
                math_as_images: true,
                no_inline_math: true,
                ..DiagramRenderOpts::default()
            },
        )
        .unwrap();
        assert!(
            !doc.links().is_empty(),
            "links in paragraph with inline math should be collected"
        );
        assert_eq!(doc.links()[0].url, "https://example.com");
    }

    #[test]
    fn test_inline_math_before_display_math_counts_as_leading_text() {
        // Inline math $x$ before display math $$y$$ should be treated as
        // leading text, not silently dropped.
        let md = "$x$ then\n\n$$y$$";
        let doc = Document::parse_with_layout(md, 80).unwrap();
        let lines = doc.visible_lines(0, 20);
        let has_x = lines.iter().any(|l| l.content().contains('x'));
        assert!(
            has_x,
            "inline math $x$ before display math should be visible"
        );
    }
}
