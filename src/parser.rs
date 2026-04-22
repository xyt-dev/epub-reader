use anyhow::{bail, Result};
use std::path::Path;

use crate::{parse_utils::ParseOptions, types::Book};

const ENABLED_EXTENSIONS: &[&str] = &[".epub", ".md", ".markdown", ".txt"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Epub,
    Markdown,
    Text,
}

impl InputFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
        match ext.as_str() {
            "epub" => Some(Self::Epub),
            "md" | "markdown" => Some(Self::Markdown),
            "txt" => Some(Self::Text),
            _ => None,
        }
    }

    fn is_implemented(self) -> bool {
        true
    }
}

pub fn parse_book(path: &Path, options: &ParseOptions) -> Result<Book> {
    match InputFormat::from_path(path) {
        Some(InputFormat::Epub) => crate::epub_parser::parse_epub(path, options),
        Some(InputFormat::Markdown) => crate::markdown_parser::parse_markdown(path, options),
        Some(InputFormat::Text) => crate::text_parser::parse_text(path, options),
        None => bail!(
            "Unsupported input file '{}'. Currently supported: {}.",
            path.display(),
            ENABLED_EXTENSIONS.join(", "),
        ),
    }
}

pub fn validate_requested_input(path: &Path) -> Result<()> {
    match InputFormat::from_path(path) {
        Some(fmt) if fmt.is_implemented() => Ok(()),
        None => bail!(
            "'{}' is not a supported input file. Currently supported: {}.",
            path.display(),
            ENABLED_EXTENSIONS.join(", "),
        ),
        Some(_) => unreachable!("all declared input formats are implemented"),
    }
}

pub fn is_enabled_input(path: &Path) -> bool {
    InputFormat::from_path(path)
        .map(InputFormat::is_implemented)
        .unwrap_or(false)
}

pub fn supported_extensions_summary() -> String {
    ENABLED_EXTENSIONS.join(", ")
}

#[cfg(test)]
mod tests {
    use super::{is_enabled_input, InputFormat};
    use std::path::Path;

    #[test]
    fn classifies_known_extensions_case_insensitively() {
        assert_eq!(
            InputFormat::from_path(Path::new("book.EPUB")),
            Some(InputFormat::Epub)
        );
        assert_eq!(
            InputFormat::from_path(Path::new("notes.MD")),
            Some(InputFormat::Markdown)
        );
        assert_eq!(
            InputFormat::from_path(Path::new("chapter.Txt")),
            Some(InputFormat::Text)
        );
    }

    #[test]
    fn all_declared_formats_are_enabled() {
        assert!(is_enabled_input(Path::new("book.epub")));
        assert!(is_enabled_input(Path::new("notes.md")));
        assert!(is_enabled_input(Path::new("chapter.txt")));
    }
}
