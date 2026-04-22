use anyhow::{Context, Result};
use std::path::Path;

use crate::parse_utils::{
    default_title_from_path, looks_like_book_title_candidate, looks_like_chapter_heading,
    strip_markdown_heading_prefix, BookBuilder, ParseOptions,
};
use crate::types::Book;

#[derive(Debug, Clone, PartialEq, Eq)]
enum TextBlock {
    Heading(String),
    Paragraph(String),
}

pub fn parse_text(path: &Path, options: &ParseOptions) -> Result<Book> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read text file '{}'.", path.display()))?;
    parse_text_str(&raw, path, &default_title_from_path(path), options)
}

fn parse_text_str(
    raw: &str,
    path: &Path,
    fallback_title: &str,
    options: &ParseOptions,
) -> Result<Book> {
    let blocks = segment_text_blocks(raw, options);
    let mut builder = BookBuilder::new(fallback_title.to_string(), options.clone());
    let mut iter = blocks.into_iter().peekable();

    if let Some(TextBlock::Heading(title)) = iter.peek() {
        if !looks_like_chapter_heading(title, options) {
            builder.set_book_title_if_absent(title);
            iter.next();
        }
    } else if let Some(TextBlock::Paragraph(title)) = iter.peek() {
        if looks_like_book_title_candidate(title, options) {
            builder.set_book_title_if_absent(title);
            iter.next();
        }
    }

    for block in iter {
        match block {
            TextBlock::Heading(title) => builder.push_chapter_title(title),
            TextBlock::Paragraph(text) => builder.push_paragraph(text),
        }
    }

    builder.finish(path)
}

fn segment_text_blocks(raw: &str, options: &ParseOptions) -> Vec<TextBlock> {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let mut blocks = Vec::new();
    let mut current = Vec::new();
    let mut previous_ended_sentence = false;

    for raw_line in normalized.lines() {
        let trimmed = raw_line.trim();

        if trimmed.is_empty() || is_scene_break(trimmed) {
            flush_current(&mut current, &mut blocks);
            previous_ended_sentence = false;
            continue;
        }

        if let Some(heading) = classify_heading(trimmed, options) {
            flush_current(&mut current, &mut blocks);
            blocks.push(TextBlock::Heading(heading));
            previous_ended_sentence = false;
            continue;
        }

        let indented = raw_line.starts_with(' ') || raw_line.starts_with('\t');
        let should_split = options.txt_hard_linebreaks
            || indented
            || (options.txt_split_on_sentence_end && previous_ended_sentence);
        if !current.is_empty() && should_split {
            flush_current(&mut current, &mut blocks);
        }

        current.push(trimmed.to_string());
        previous_ended_sentence = ends_sentence(trimmed);
    }

    flush_current(&mut current, &mut blocks);
    blocks
}

fn classify_heading(line: &str, options: &ParseOptions) -> Option<String> {
    let markdown_heading = strip_markdown_heading_prefix(line);
    if markdown_heading != line {
        return Some(markdown_heading);
    }

    if looks_like_chapter_heading(line, options) {
        Some(line.trim().to_string())
    } else {
        None
    }
}

fn flush_current(current: &mut Vec<String>, blocks: &mut Vec<TextBlock>) {
    if current.is_empty() {
        return;
    }

    blocks.push(TextBlock::Paragraph(current.join(" ")));
    current.clear();
}

fn is_scene_break(line: &str) -> bool {
    let chars: Vec<char> = line.chars().filter(|c| !c.is_whitespace()).collect();
    chars.len() >= 3
        && chars
            .iter()
            .all(|c| matches!(c, '*' | '•' | '-' | '—' | '~' | '_'))
}

fn ends_sentence(line: &str) -> bool {
    line.trim_end()
        .chars()
        .last()
        .map(|c| {
            matches!(
                c,
                '.'
                    | '!'
                    | '?'
                    | '。'
                    | '！'
                    | '？'
                    | '…'
                    | '"'
                    | '\''
                    | '”'
                    | '’'
                    | ')'
                    | '」'
                    | '』'
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{parse_text_str, segment_text_blocks, TextBlock};
    use crate::parse_utils::ParseOptions;
    use std::path::Path;

    #[test]
    fn segments_titles_headings_and_wrapped_paragraphs() {
        let raw = r#"Overlord Volume 1

Chapter 1
The first line
continues here.

The next paragraph ends here.
Another standalone paragraph.
"#;

        let blocks = segment_text_blocks(raw, &ParseOptions::default());
        assert_eq!(
            blocks[0],
            TextBlock::Paragraph("Overlord Volume 1".to_string())
        );
        assert_eq!(blocks[1], TextBlock::Heading("Chapter 1".to_string()));
        assert_eq!(
            blocks[2],
            TextBlock::Paragraph("The first line continues here.".to_string())
        );
    }

    #[test]
    fn parses_title_and_chapters() {
        let raw = r#"Overlord Volume 1

Chapter 1

The first paragraph.

Chapter 2

The second paragraph.
"#;

        let book = parse_text_str(raw, Path::new("book.txt"), "book", &ParseOptions::default())
            .unwrap();
        assert_eq!(book.title, "Overlord Volume 1");
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].title.as_deref(), Some("Chapter 1"));
        assert_eq!(book.chapters[1].title.as_deref(), Some("Chapter 2"));
    }

    #[test]
    fn txt_hard_linebreaks_are_configurable() {
        let raw = "Chapter 1\nOne line\nTwo line";
        let options = ParseOptions {
            txt_hard_linebreaks: true,
            txt_split_on_sentence_end: false,
            ..ParseOptions::default()
        };

        let blocks = segment_text_blocks(raw, &options);
        assert_eq!(blocks[0], TextBlock::Heading("Chapter 1".to_string()));
        assert_eq!(blocks[1], TextBlock::Paragraph("One line".to_string()));
        assert_eq!(blocks[2], TextBlock::Paragraph("Two line".to_string()));
    }
}
