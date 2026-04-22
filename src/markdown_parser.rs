use anyhow::{Context, Result};
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag};
use std::path::Path;

use crate::parse_utils::{
    default_title_from_path, looks_like_chapter_heading, BookBuilder, ParseOptions,
};
use crate::types::Book;

#[derive(Debug, Clone, Copy)]
enum ActiveBlockKind {
    Heading(u8),
    Paragraph,
    ListItem,
}

#[derive(Debug, Clone)]
struct ActiveBlock {
    kind: ActiveBlockKind,
    depth: usize,
    text: String,
}

impl ActiveBlock {
    fn new(kind: ActiveBlockKind) -> Self {
        Self {
            kind,
            depth: 1,
            text: String::new(),
        }
    }
}

pub fn parse_markdown(path: &Path, options: &ParseOptions) -> Result<Book> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read markdown file '{}'.", path.display()))?;

    let (frontmatter_title, markdown) = split_frontmatter(&raw);
    let fallback_title = frontmatter_title
        .clone()
        .unwrap_or_else(|| default_title_from_path(path));

    parse_markdown_str(markdown, path, &fallback_title, frontmatter_title, options)
}

fn parse_markdown_str(
    markdown: &str,
    path: &Path,
    fallback_title: &str,
    frontmatter_title: Option<String>,
    options: &ParseOptions,
) -> Result<Book> {
    let (frontmatter_title, markdown) = if frontmatter_title.is_some() {
        (frontmatter_title, markdown)
    } else {
        let (title, body) = split_frontmatter(markdown);
        (title, body)
    };

    let mut builder = BookBuilder::new(fallback_title.to_string(), options.clone());

    if let Some(title) = frontmatter_title {
        builder.set_book_title_if_absent(title);
    }

    let mut active: Option<ActiveBlock> = None;

    for event in Parser::new(markdown) {
        match event {
            Event::Start(tag) => handle_start(tag, &mut active),
            Event::End(_) => {
                if let Some(block) = active.as_mut() {
                    block.depth = block.depth.saturating_sub(1);
                    if block.depth == 0 {
                        let finished = active.take().unwrap();
                        push_block(finished, &mut builder, options);
                    }
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(block) = active.as_mut() {
                    block.text.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(block) = active.as_mut() {
                    block.text.push(' ');
                }
            }
            _ => {}
        }
    }

    if let Some(block) = active.take() {
        push_block(block, &mut builder, options);
    }

    builder.finish(path)
}

fn handle_start(tag: Tag<'_>, active: &mut Option<ActiveBlock>) {
    if let Some(block) = active.as_mut() {
        block.depth += 1;
        return;
    }

    match tag {
        Tag::Heading { level, .. } => {
            *active = Some(ActiveBlock::new(ActiveBlockKind::Heading(heading_level(
                level,
            ))));
        }
        Tag::Paragraph => {
            *active = Some(ActiveBlock::new(ActiveBlockKind::Paragraph));
        }
        Tag::Item => {
            *active = Some(ActiveBlock::new(ActiveBlockKind::ListItem));
        }
        _ => {}
    }
}

fn push_block(block: ActiveBlock, builder: &mut BookBuilder, options: &ParseOptions) {
    let text = block.text.trim();
    if text.is_empty() {
        return;
    }

    match block.kind {
        ActiveBlockKind::Heading(level) if level == 1 && builder.is_pristine() => {
            if !looks_like_chapter_heading(text, options) {
                builder.set_book_title_if_absent(text);
            } else {
                builder.push_chapter_title(text);
            }
        }
        ActiveBlockKind::Heading(level) if level <= 3 => {
            builder.push_chapter_title(text);
        }
        ActiveBlockKind::Heading(_) | ActiveBlockKind::Paragraph | ActiveBlockKind::ListItem => {
            builder.push_paragraph(text);
        }
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn split_frontmatter(raw: &str) -> (Option<String>, &str) {
    let normalized = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    if !normalized.starts_with("---\n") {
        return (None, normalized);
    }

    let rest = &normalized[4..];
    let Some(close) = rest.find("\n---\n") else {
        return (None, normalized);
    };

    let frontmatter = &rest[..close];
    let body = &rest[close + 5..];
    let title = frontmatter.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.trim().eq_ignore_ascii_case("title") {
            let value = value.trim().trim_matches('"').trim_matches('\'');
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        } else {
            None
        }
    });

    (title, body)
}

#[cfg(test)]
mod tests {
    use super::parse_markdown_str;
    use crate::parse_utils::ParseOptions;
    use std::path::Path;

    #[test]
    fn parses_frontmatter_title_and_headings() {
        let raw = r#"---
title: Overlord Volume 1
---

## Prologue

The first paragraph.

- One list item
- Another list item

## Chapter 1

Second paragraph.
"#;

        let book = parse_markdown_str(
            raw,
            Path::new("book.md"),
            "book",
            None,
            &ParseOptions::default(),
        )
        .unwrap();
        assert_eq!(book.title, "Overlord Volume 1");
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].title.as_deref(), Some("Prologue"));
        assert_eq!(book.chapters[0].paragraphs.len(), 3);
        assert_eq!(book.chapters[1].title.as_deref(), Some("Chapter 1"));
    }

    #[test]
    fn uses_first_h1_as_book_title() {
        let raw = r#"# Custom Title

## Chapter 1

Hello there.
"#;

        let book = parse_markdown_str(
            raw,
            Path::new("book.md"),
            "book",
            None,
            &ParseOptions::default(),
        )
        .unwrap();
        assert_eq!(book.title, "Custom Title");
        assert_eq!(book.chapters[0].title.as_deref(), Some("Chapter 1"));
    }
}
