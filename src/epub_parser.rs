use anyhow::{Context, Result};
use epub::doc::EpubDoc;
use scraper::{ElementRef, Html, Selector};
use slug::slugify;

use crate::parse_utils::{
    default_title_from_path, is_substantive_text, looks_like_chapter_heading, looks_like_sentence,
    normalize_text, ParseOptions,
};
use crate::types::{Book, Chapter, Paragraph};

pub fn parse_epub(epub_path: &std::path::Path, options: &ParseOptions) -> Result<Book> {
    let mut doc = EpubDoc::new(epub_path)
        .with_context(|| format!("Failed to open epub: {}", epub_path.display()))?;

    let title = doc
        .mdata("title")
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| default_title_from_path(epub_path));

    let slug = {
        let candidate = slugify(&title);
        if candidate.is_empty() {
            let fallback = slugify(default_title_from_path(epub_path));
            if fallback.is_empty() {
                "book".to_string()
            } else {
                fallback
            }
        } else {
            candidate
        }
    };

    let spine_len = doc.get_num_pages();
    let mut chapters = Vec::new();

    for page_idx in 0..spine_len {
        let _ = doc.set_current_page(page_idx);

        let content = match doc.get_current_str() {
            Ok(s) => s,
            Err(_) => continue,
        };

        let chapter_index = chapters.len();
        let paras = extract_paragraphs(&content, &slug, chapter_index, options);
        if paras.is_empty() {
            continue;
        }

        let title_opt = extract_chapter_title(&content);

        chapters.push(Chapter {
            index: chapter_index,
            title: title_opt,
            paragraphs: paras,
        });
    }

    Ok(Book {
        slug,
        title,
        chapters,
    })
}

fn extract_paragraphs(
    html: &str,
    book_slug: &str,
    chapter_idx: usize,
    options: &ParseOptions,
) -> Vec<Paragraph> {
    let document = Html::parse_document(html);
    let texts = extract_text_blocks(&document, options);

    texts
        .into_iter()
        .enumerate()
        .map(|(para_idx, text)| Paragraph {
            id: format!("{}-ch{:03}-p{:04}", book_slug, chapter_idx, para_idx),
            text,
        })
        .collect()
}

fn extract_text_blocks(document: &Html, options: &ParseOptions) -> Vec<String> {
    let primary_sel = Selector::parse("p, blockquote, li").unwrap();
    let mut texts: Vec<String> = document
        .select(&primary_sel)
        .filter_map(|element| extract_primary_block_text(element, options))
        .collect();

    if texts.is_empty() {
        let div_sel = Selector::parse("div").unwrap();
        texts = document
            .select(&div_sel)
            .filter_map(|element| extract_div_fallback_text(element, options))
            .collect();
    }

    if looks_like_navigation_page(&texts, options) {
        return Vec::new();
    }

    texts
}

fn extract_chapter_title(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("h1, h2, h3, h4, title").unwrap();

    for element in document.select(&selector) {
        let text = normalize_text(&element.text().collect::<Vec<_>>().join(" "));
        if !text.is_empty() && !text.eq_ignore_ascii_case("contents") {
            return Some(text);
        }
    }

    None
}

fn extract_primary_block_text(element: ElementRef<'_>, options: &ParseOptions) -> Option<String> {
    if has_skipped_ancestor(element) {
        return None;
    }

    let tag = element.value().name();
    if matches!(tag, "li" | "blockquote") && has_descendant_tag(element, &["p", "li", "blockquote"])
    {
        return None;
    }

    let text = normalize_text(&element.text().collect::<Vec<_>>().join(" "));
    if !is_substantive_text(&text, options) || looks_like_navigation_entry(&text, options) {
        None
    } else {
        Some(text)
    }
}

fn extract_div_fallback_text(element: ElementRef<'_>, options: &ParseOptions) -> Option<String> {
    if has_skipped_ancestor(element)
        || has_descendant_tag(
            element,
            &[
                "p",
                "li",
                "blockquote",
                "div",
                "section",
                "article",
                "ul",
                "ol",
                "table",
            ],
        )
    {
        return None;
    }

    let text = normalize_text(&element.text().collect::<Vec<_>>().join(" "));
    if !is_substantive_text(&text, options) || text.split_whitespace().count() < 4 {
        return None;
    }

    Some(text)
}

fn has_skipped_ancestor(element: ElementRef<'_>) -> bool {
    element.ancestors().skip(1).any(|node| {
        ElementRef::wrap(node)
            .map(|ancestor| matches!(ancestor.value().name(), "nav" | "header" | "footer"))
            .unwrap_or(false)
    })
}

fn has_descendant_tag(element: ElementRef<'_>, tags: &[&str]) -> bool {
    element.descendants().skip(1).any(|node| {
        ElementRef::wrap(node)
            .map(|child| tags.contains(&child.value().name()))
            .unwrap_or(false)
    })
}

fn looks_like_navigation_page(texts: &[String], options: &ParseOptions) -> bool {
    texts.len() >= 4
        && texts.iter().all(|text| {
            looks_like_navigation_entry(text, options)
                || (text.split_whitespace().count() <= 12 && !looks_like_sentence(text))
        })
}

fn looks_like_navigation_entry(text: &str, options: &ParseOptions) -> bool {
    let text = normalize_text(text);
    looks_like_chapter_heading(&text, options) || text.eq_ignore_ascii_case("contents")
}

#[cfg(test)]
mod tests {
    use super::{extract_chapter_title, extract_text_blocks};
    use crate::parse_utils::ParseOptions;
    use scraper::Html;

    #[test]
    fn extracts_multiple_block_types() {
        let html = Html::parse_document(
            r#"<html><body>
            <h1>Chapter 1</h1>
            <p>"Short dialogue."</p>
            <blockquote>The grave was silent.</blockquote>
            <ul><li>The throne room was cold.</li></ul>
            </body></html>"#,
        );

        let blocks = extract_text_blocks(&html, &ParseOptions::default());
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0], "\"Short dialogue.\"");
    }

    #[test]
    fn skips_navigation_like_pages() {
        let html = Html::parse_document(
            r#"<html><body><ul>
            <li>Chapter 1</li>
            <li>Chapter 2</li>
            <li>Chapter 3</li>
            <li>Chapter 4</li>
            </ul></body></html>"#,
        );

        let blocks = extract_text_blocks(&html, &ParseOptions::default());
        assert!(blocks.is_empty());
    }

    #[test]
    fn extracts_heading_title() {
        let title = extract_chapter_title("<html><body><h2>Prologue</h2></body></html>");
        assert_eq!(title.as_deref(), Some("Prologue"));
    }
}
