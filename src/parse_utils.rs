use anyhow::{bail, Result};
use regex::Regex;
use slug::slugify;
use std::path::Path;
use std::sync::OnceLock;

use crate::types::{Book, Chapter, Paragraph};

#[derive(Debug, Clone)]
pub struct ParseOptions {
    pub min_paragraph_chars: usize,
    pub title_max_words: usize,
    pub short_heading_max_words: usize,
    pub txt_hard_linebreaks: bool,
    pub txt_split_on_sentence_end: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            min_paragraph_chars: 2,
            title_max_words: 12,
            short_heading_max_words: 8,
            txt_hard_linebreaks: false,
            txt_split_on_sentence_end: true,
        }
    }
}

impl ParseOptions {
    pub fn summary(&self) -> String {
        format!(
            "min-chars={} · title<= {} words · short-heading<= {} words · txt-hard-linebreaks={} · txt-sentence-split={}",
            self.min_paragraph_chars,
            self.title_max_words,
            self.short_heading_max_words,
            self.txt_hard_linebreaks,
            self.txt_split_on_sentence_end
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChapterSeed {
    pub title: Option<String>,
    pub paragraphs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BookBuilder {
    options: ParseOptions,
    default_title: String,
    book_title: Option<String>,
    chapters: Vec<ChapterSeed>,
    current: ChapterSeed,
}

impl BookBuilder {
    pub fn new(default_title: impl Into<String>, options: ParseOptions) -> Self {
        Self {
            options,
            default_title: default_title.into(),
            book_title: None,
            chapters: Vec::new(),
            current: ChapterSeed::default(),
        }
    }

    pub fn is_pristine(&self) -> bool {
        self.book_title.is_none()
            && self.chapters.is_empty()
            && self.current.title.is_none()
            && self.current.paragraphs.is_empty()
    }

    pub fn set_book_title_if_absent(&mut self, title: impl AsRef<str>) {
        let title = normalize_text(title.as_ref());
        if !title.is_empty() && self.book_title.is_none() {
            self.book_title = Some(title);
        }
    }

    pub fn push_chapter_title(&mut self, title: impl AsRef<str>) {
        let title = normalize_text(title.as_ref());
        if title.is_empty() {
            return;
        }

        if self.current.paragraphs.is_empty() {
            self.current.title = Some(title);
        } else {
            self.flush_current();
            self.current.title = Some(title);
        }
    }

    pub fn push_paragraph(&mut self, text: impl AsRef<str>) {
        let text = normalize_text(text.as_ref());
        if is_substantive_text(&text, &self.options) {
            self.current.paragraphs.push(text);
        }
    }

    pub fn finish(mut self, path: &Path) -> Result<Book> {
        self.flush_current();

        let title = self
            .book_title
            .clone()
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| self.default_title.clone());

        let slug = non_empty_slug(&title)
            .or_else(|| non_empty_slug(&self.default_title))
            .unwrap_or_else(|| "book".to_string());

        let mut chapters = Vec::new();

        for chapter in self.chapters {
            let mut paragraphs = Vec::new();
            for (para_idx, text) in chapter.paragraphs.into_iter().enumerate() {
                paragraphs.push(Paragraph {
                    id: format!("{}-ch{:03}-p{:04}", slug, chapters.len(), para_idx),
                    text,
                });
            }

            if paragraphs.is_empty() {
                continue;
            }

            chapters.push(Chapter {
                index: chapters.len(),
                title: chapter.title,
                paragraphs,
            });
        }

        if chapters.is_empty() {
            bail!("No readable paragraphs found in '{}'.", path.display());
        }

        Ok(Book {
            slug,
            title,
            chapters,
        })
    }

    fn flush_current(&mut self) {
        if self.current.title.is_some() || !self.current.paragraphs.is_empty() {
            self.chapters.push(std::mem::take(&mut self.current));
        }
    }
}

pub fn default_title_from_path(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Untitled".to_string())
}

pub fn normalize_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn is_substantive_text(text: &str, options: &ParseOptions) -> bool {
    let text = normalize_text(text);
    if text.is_empty() {
        return false;
    }
    if text.chars().all(|c| !c.is_alphanumeric()) {
        return false;
    }
    if page_number_re().is_match(&text) {
        return false;
    }
    if text.chars().count() < options.min_paragraph_chars && !has_sentence_punctuation(&text) {
        return false;
    }

    let word_count = text.split_whitespace().count();
    let has_alpha = text.chars().any(|c| c.is_alphabetic());
    let has_lowercase = text.chars().any(|c| c.is_lowercase());

    if word_count <= 4 && has_alpha && !has_lowercase && !has_sentence_punctuation(&text) {
        return false;
    }

    true
}

pub fn looks_like_chapter_heading(text: &str, options: &ParseOptions) -> bool {
    let text = strip_markdown_heading_prefix(text);
    let text = normalize_text(&text);
    if text.is_empty() {
        return false;
    }

    chapter_heading_re().is_match(&text)
        || chinese_heading_re().is_match(&text)
        || roman_heading_re().is_match(&text)
        || looks_like_short_upper_heading(&text, options)
}

pub fn looks_like_book_title_candidate(text: &str, options: &ParseOptions) -> bool {
    let text = normalize_text(text);
    !text.is_empty()
        && !looks_like_chapter_heading(&text, options)
        && text.split_whitespace().count() <= options.title_max_words
        && !has_sentence_punctuation(&text)
}

pub fn strip_markdown_heading_prefix(text: &str) -> String {
    let trimmed = text.trim();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) {
        let rest = trimmed[hashes..].trim();
        if !rest.is_empty() {
            return rest.trim_end_matches('#').trim().to_string();
        }
    }
    trimmed.to_string()
}

pub fn looks_like_sentence(text: &str) -> bool {
    let text = normalize_text(text);
    text.split_whitespace().count() >= 8 || has_sentence_punctuation(&text)
}

pub fn has_sentence_punctuation(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c, '.' | '!' | '?' | '。' | '！' | '？' | '…'))
}

fn looks_like_short_upper_heading(text: &str, options: &ParseOptions) -> bool {
    let words = text.split_whitespace().count();
    words > 0
        && words <= options.short_heading_max_words
        && text.chars().any(|c| c.is_alphabetic())
        && text.chars().all(|c| !c.is_alphabetic() || c.is_uppercase())
        && !has_sentence_punctuation(text)
}

fn non_empty_slug(text: &str) -> Option<String> {
    let slug = slugify(text);
    if slug.is_empty() {
        None
    } else {
        Some(slug)
    }
}

fn page_number_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^(page\s+)?(?:\d+|[ivxlcdm]+)$").unwrap())
}

fn chapter_heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?ix)
            ^
            (chapter|chap\.?|book|part|volume|vol\.?|section|scene|prologue|epilogue|interlude|afterword|appendix|preface|foreword)
            \b
            [\s:.\-–—]*
            [\p{L}\p{N}\s:.\-–—,'"()!?]*$
        "#,
        )
        .unwrap()
    })
}

fn chinese_heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^第[0-9一二三四五六七八九十百千万零〇两]+[章节卷部篇回节集][：:、.\- ]*.*$")
            .unwrap()
    })
}

fn roman_heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?ix)^(book|part|volume|chapter)\s+[ivxlcdm]+\b[\s:.\-–—]*[\p{L}\p{N}\s:.\-–—,'"()!?]*$"#,
        )
        .unwrap()
    })
}

#[cfg(test)]
mod tests {
    use super::{
        has_sentence_punctuation, is_substantive_text, looks_like_book_title_candidate,
        looks_like_chapter_heading, normalize_text, strip_markdown_heading_prefix, ParseOptions,
    };

    #[test]
    fn keeps_short_dialogue_as_substantive() {
        let options = ParseOptions::default();
        assert!(is_substantive_text("\"Why?\"", &options));
        assert!(is_substantive_text("Yes.", &options));
    }

    #[test]
    fn rejects_page_numbers_and_banner_lines() {
        let options = ParseOptions::default();
        assert!(!is_substantive_text("42", &options));
        assert!(!is_substantive_text("CHAPTER", &options));
    }

    #[test]
    fn detects_common_heading_patterns() {
        let options = ParseOptions::default();
        assert!(looks_like_chapter_heading(
            "Chapter 12 - The Tomb",
            &options
        ));
        assert!(looks_like_chapter_heading("第十二章 王都", &options));
        assert!(looks_like_chapter_heading("PROLOGUE", &options));
        assert!(!looks_like_chapter_heading("He looked at her.", &options));
    }

    #[test]
    fn strips_markdown_heading_prefixes() {
        assert_eq!(
            strip_markdown_heading_prefix("## Chapter 1 ##"),
            "Chapter 1"
        );
    }

    #[test]
    fn title_candidates_are_short_and_not_sentences() {
        let options = ParseOptions::default();
        assert!(looks_like_book_title_candidate(
            "Overlord Volume 1",
            &options
        ));
        assert!(!looks_like_book_title_candidate(
            "He stepped into the throne room.",
            &options
        ));
        assert!(has_sentence_punctuation("Wait!"));
        assert_eq!(normalize_text(" a \n  b\tc "), "a b c");
    }

    #[test]
    fn title_limit_is_configurable() {
        let options = ParseOptions {
            title_max_words: 2,
            ..ParseOptions::default()
        };
        assert!(!looks_like_book_title_candidate(
            "Overlord Volume 1",
            &options
        ));
    }
}
