use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ParagraphKind {
    #[default]
    Text,
    CodeBlock {
        language: Option<String>,
    },
}

/// A single paragraph extracted from a source chapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paragraph {
    /// Unique ID: "{book_slug}-ch{chapter:03}-p{para:04}"
    pub id: String,
    /// The raw English text of this paragraph.
    pub text: String,
    #[serde(default)]
    pub kind: ParagraphKind,
}

impl Paragraph {
    pub fn is_translatable(&self) -> bool {
        matches!(self.kind, ParagraphKind::Text)
    }
}

/// A chapter extracted from the source document.
#[derive(Debug, Clone)]
pub struct Chapter {
    pub index: usize,
    pub title: Option<String>,
    pub paragraphs: Vec<Paragraph>,
}

/// A parsed book, regardless of input format.
#[derive(Debug, Clone)]
pub struct Book {
    pub slug: String,
    pub title: String,
    pub chapters: Vec<Chapter>,
}

/// The structured response the LLM must return for each paragraph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub translation: String,
    pub vocabulary: Vec<VocabEntry>,
    pub chunks: Vec<ChunkEntry>,
}

/// An IELTS 6.5+ vocabulary entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VocabEntry {
    pub word: String,
    pub ipa: String,
    pub pos: String,
    pub cn: String,
    pub example: String,
}

/// A useful language chunk / collocations entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkEntry {
    pub chunk: String,
    pub cn: String,
    pub example: String,
}
