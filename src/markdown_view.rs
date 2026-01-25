// SPDX-License-Identifier: GPL-3.0-only

//! Markdown rendering widget using libcosmic/iced widgets.
//!
//! This module provides a rendered view of markdown content using native
//! libcosmic widgets, without modifying cosmic-text.

use cosmic::{
    iced::{Alignment, Length, Padding},
    widget::{self, scrollable},
    Element,
};
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::Message;

/// Render markdown content as a scrollable widget tree.
pub fn markdown_view<'a>(content: &str) -> Element<'a, Message> {
    let parser = Parser::new(content);
    let elements = render_events(parser);

    scrollable(
        widget::column::with_children(elements)
            .spacing(8)
            .padding(Padding::new(16.0)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// State for tracking nested list context
struct RenderState {
    /// Current list nesting level
    list_depth: usize,
    /// Ordered list counters at each depth
    list_counters: Vec<Option<u64>>,
    /// Whether we're in a code block
    in_code_block: bool,
    /// Accumulated text for current code block
    code_block_text: String,
    /// Accumulated text for current paragraph
    paragraph_text: String,
    /// Whether we're in a paragraph
    in_paragraph: bool,
    /// Current heading level (if any)
    heading_level: Option<HeadingLevel>,
    /// Accumulated text for current heading
    heading_text: String,
    /// Whether we're inside strong/bold text
    in_strong: bool,
    /// Whether we're inside emphasized/italic text
    in_emphasis: bool,
}

impl Default for RenderState {
    fn default() -> Self {
        Self {
            list_depth: 0,
            list_counters: Vec::new(),
            in_code_block: false,
            code_block_text: String::new(),
            paragraph_text: String::new(),
            in_paragraph: false,
            heading_level: None,
            heading_text: String::new(),
            in_strong: false,
            in_emphasis: false,
        }
    }
}

fn render_events<'a>(parser: Parser<'_>) -> Vec<Element<'a, Message>> {
    let mut elements: Vec<Element<'a, Message>> = Vec::new();
    let mut state = RenderState::default();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    state.heading_level = Some(level);
                    state.heading_text.clear();
                }
                Tag::Paragraph => {
                    state.in_paragraph = true;
                    state.paragraph_text.clear();
                }
                Tag::CodeBlock(_) => {
                    state.in_code_block = true;
                    state.code_block_text.clear();
                }
                Tag::List(start) => {
                    state.list_depth += 1;
                    state.list_counters.push(start);
                }
                Tag::Item => {
                    // Item start - will accumulate text
                }
                Tag::BlockQuote => {
                    // Start blockquote
                }
                Tag::Strong => {
                    state.in_strong = true;
                }
                Tag::Emphasis => {
                    state.in_emphasis = true;
                }
                Tag::Strikethrough | Tag::Link { .. } => {
                    // Other inline formatting - handled in text
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    if let Some(level) = state.heading_level.take() {
                        let text = std::mem::take(&mut state.heading_text);
                        elements.push(render_heading(level, &text));
                    }
                }
                TagEnd::Paragraph => {
                    state.in_paragraph = false;
                    let text = std::mem::take(&mut state.paragraph_text);
                    if !text.is_empty() {
                        elements.push(render_paragraph(&text));
                    }
                }
                TagEnd::CodeBlock => {
                    state.in_code_block = false;
                    let code = std::mem::take(&mut state.code_block_text);
                    elements.push(render_code_block(&code));
                }
                TagEnd::List(_) => {
                    state.list_depth = state.list_depth.saturating_sub(1);
                    state.list_counters.pop();
                }
                TagEnd::Item => {
                    // Item end - handled when text is added
                }
                TagEnd::Strong => {
                    state.in_strong = false;
                }
                TagEnd::Emphasis => {
                    state.in_emphasis = false;
                }
                _ => {}
            },
            Event::Text(text) => {
                if state.in_code_block {
                    state.code_block_text.push_str(&text);
                } else if state.heading_level.is_some() {
                    state.heading_text.push_str(&text);
                } else if state.in_paragraph {
                    // For paragraph text, mark bold/emphasis with markers
                    if state.in_strong {
                        state.paragraph_text.push_str("\x02"); // Start bold marker
                        state.paragraph_text.push_str(&text);
                        state.paragraph_text.push_str("\x03"); // End bold marker
                    } else if state.in_emphasis {
                        state.paragraph_text.push_str("\x04"); // Start italic marker
                        state.paragraph_text.push_str(&text);
                        state.paragraph_text.push_str("\x05"); // End italic marker
                    } else {
                        state.paragraph_text.push_str(&text);
                    }
                } else if state.list_depth > 0 {
                    // List item text
                    let indent = "    ".repeat(state.list_depth.saturating_sub(1));
                    let bullet = if let Some(Some(n)) = state.list_counters.last_mut() {
                        let b = format!("{}. ", n);
                        *n += 1;
                        b
                    } else {
                        "\u{2022} ".to_string() // bullet point
                    };
                    elements.push(render_list_item(&indent, &bullet, &text, state.in_strong, state.in_emphasis));
                } else {
                    // Standalone text - render with formatting
                    elements.push(render_text_with_formatting(&text, state.in_strong, state.in_emphasis));
                }
            }
            Event::Code(code) => {
                // Inline code
                if state.in_paragraph {
                    state.paragraph_text.push('`');
                    state.paragraph_text.push_str(&code);
                    state.paragraph_text.push('`');
                } else if state.heading_level.is_some() {
                    state.heading_text.push_str(&code);
                } else {
                    elements.push(render_inline_code(&code));
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if state.in_paragraph {
                    state.paragraph_text.push(' ');
                } else if state.heading_level.is_some() {
                    state.heading_text.push(' ');
                }
            }
            Event::Rule => {
                elements.push(render_horizontal_rule());
            }
            _ => {}
        }
    }

    elements
}

fn render_heading<'a>(level: HeadingLevel, text: &str) -> Element<'a, Message> {
    let size = match level {
        HeadingLevel::H1 => 28,
        HeadingLevel::H2 => 24,
        HeadingLevel::H3 => 20,
        HeadingLevel::H4 => 18,
        HeadingLevel::H5 => 16,
        HeadingLevel::H6 => 14,
    };

    widget::container(
        widget::text(text.to_string())
            .size(size)
            .font(cosmic::font::Font {
                weight: cosmic::iced::font::Weight::Bold,
                ..cosmic::font::default()
            }),
    )
    .padding(Padding::from([8, 0]))
    .into()
}

fn render_paragraph<'a>(text: &str) -> Element<'a, Message> {
    // Check for formatting markers and render appropriately
    // \x02...\x03 = bold, \x04...\x05 = italic
    if text.contains('\x02') || text.contains('\x04') {
        // Has formatting - render as rich text
        render_rich_paragraph(text)
    } else {
        widget::text(text.to_string())
            .width(Length::Fill)
            .wrapping(cosmic::iced::widget::text::Wrapping::Word)
            .into()
    }
}

fn render_rich_paragraph<'a>(text: &str) -> Element<'a, Message> {
    // Parse formatted text and render as a row of text widgets
    let mut parts: Vec<Element<'a, Message>> = Vec::new();
    let mut current = String::new();
    let mut in_bold = false;
    let mut in_italic = false;

    for c in text.chars() {
        match c {
            '\x02' => {
                // Start bold
                if !current.is_empty() {
                    parts.push(widget::text(std::mem::take(&mut current)).into());
                }
                in_bold = true;
            }
            '\x03' => {
                // End bold
                if !current.is_empty() {
                    parts.push(
                        widget::text(std::mem::take(&mut current))
                            .font(cosmic::font::Font {
                                weight: cosmic::iced::font::Weight::Bold,
                                ..cosmic::font::default()
                            })
                            .into(),
                    );
                }
                in_bold = false;
            }
            '\x04' => {
                // Start italic
                if !current.is_empty() {
                    parts.push(widget::text(std::mem::take(&mut current)).into());
                }
                in_italic = true;
            }
            '\x05' => {
                // End italic
                if !current.is_empty() {
                    parts.push(
                        widget::text(std::mem::take(&mut current))
                            .font(cosmic::font::Font {
                                style: cosmic::iced::font::Style::Italic,
                                ..cosmic::font::default()
                            })
                            .into(),
                    );
                }
                in_italic = false;
            }
            _ => {
                current.push(c);
            }
        }
    }

    // Remaining text
    if !current.is_empty() {
        if in_bold {
            parts.push(
                widget::text(current)
                    .font(cosmic::font::Font {
                        weight: cosmic::iced::font::Weight::Bold,
                        ..cosmic::font::default()
                    })
                    .into(),
            );
        } else if in_italic {
            parts.push(
                widget::text(current)
                    .font(cosmic::font::Font {
                        style: cosmic::iced::font::Style::Italic,
                        ..cosmic::font::default()
                    })
                    .into(),
            );
        } else {
            parts.push(widget::text(current).into());
        }
    }

    widget::row::with_children(parts)
        .wrap()
        .into()
}

fn render_text_with_formatting<'a>(text: &str, bold: bool, italic: bool) -> Element<'a, Message> {
    let mut txt = widget::text(text.to_string())
        .width(Length::Fill)
        .wrapping(cosmic::iced::widget::text::Wrapping::Word);

    if bold {
        txt = txt.font(cosmic::font::Font {
            weight: cosmic::iced::font::Weight::Bold,
            ..cosmic::font::default()
        });
    } else if italic {
        txt = txt.font(cosmic::font::Font {
            style: cosmic::iced::font::Style::Italic,
            ..cosmic::font::default()
        });
    }

    txt.into()
}

fn render_code_block<'a>(code: &str) -> Element<'a, Message> {
    widget::container(
        widget::text(code.to_string())
            .font(cosmic::font::Font::MONOSPACE)
            .size(13),
    )
    .padding(Padding::new(12.0))
    .style(|theme: &cosmic::Theme| {
        let cosmic = theme.cosmic();
        widget::container::Style {
            background: Some(cosmic::iced::Background::Color(
                cosmic.background.component.base.into(),
            )),
            border: cosmic::iced_core::Border {
                radius: cosmic.radius_s().into(),
                width: 1.0,
                color: cosmic.background.component.divider.into(),
            },
            ..Default::default()
        }
    })
    .width(Length::Fill)
    .into()
}

fn render_inline_code<'a>(code: &str) -> Element<'a, Message> {
    widget::container(
        widget::text(code.to_string())
            .font(cosmic::font::Font::MONOSPACE)
            .size(13),
    )
    .padding(Padding::from([2, 6]))
    .style(|theme: &cosmic::Theme| {
        let cosmic = theme.cosmic();
        widget::container::Style {
            background: Some(cosmic::iced::Background::Color(
                cosmic.background.component.base.into(),
            )),
            border: cosmic::iced_core::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .into()
}

fn render_list_item<'a>(indent: &str, bullet: &str, text: &str, bold: bool, italic: bool) -> Element<'a, Message> {
    let mut txt = widget::text(text.to_string())
        .width(Length::Fill)
        .wrapping(cosmic::iced::widget::text::Wrapping::Word);

    if bold {
        txt = txt.font(cosmic::font::Font {
            weight: cosmic::iced::font::Weight::Bold,
            ..cosmic::font::default()
        });
    } else if italic {
        txt = txt.font(cosmic::font::Font {
            style: cosmic::iced::font::Style::Italic,
            ..cosmic::font::default()
        });
    }

    widget::row::with_capacity(2)
        .align_y(Alignment::Start)
        .push(widget::text(format!("{}{}", indent, bullet)))
        .push(txt)
        .spacing(4)
        .into()
}

fn render_horizontal_rule<'a>() -> Element<'a, Message> {
    widget::container(widget::divider::horizontal::light())
        .padding(Padding::from([8, 0]))
        .width(Length::Fill)
        .into()
}
