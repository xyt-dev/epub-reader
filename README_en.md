# epub-reader — EPUB / Markdown / TXT to HTML + AI Paragraph Translation

[中文](README.md)

> Convert `.epub`, `.md/.markdown`, and `.txt` into readable HTML, then use Claude to generate a translation, vocabulary notes, and chunk analysis for each paragraph. Supports resume-after-interrupt, offline rebuild, controlled concurrency, and configurable text segmentation.

![png](1.png)

## Features

- Supports `epub`, `md/markdown`, and `txt` input
- Works on a single file or recursively scans a directory
- Produces reader-friendly HTML with 3 collapsible AI sections per paragraph
- Calls Claude and expects structured JSON: translation / vocabulary / chunks
- Supports `Ctrl+C` interrupt and resume without redoing completed paragraphs
- Supports `--rebuild` to regenerate HTML from state files without API calls
- Supports `--jobs` for concurrent requests and `--request-delay-ms` for throttling
- TXT / Markdown segmentation behavior can be tuned from the CLI
- Both HTML and state files use atomic writes for safer crash recovery

## Installation

### Prerequisites

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Set your Anthropic API key
export ANTHROPIC_AUTH_TOKEN="sk-ant-..."

# Optional: custom compatible gateway
export ANTHROPIC_BASE_URL="https://api.anthropic.com"
```

### Build

```bash
cd epub-reader
cargo build --release
```

## Quick Start

### 1. Translate a single EPUB

```bash
cargo run --release -- ./books/vol1.epub
```

### 2. Translate a whole directory

```bash
cargo run --release -- ./books ./output
```

### 3. Process Markdown

```bash
cargo run --release -- ./notes/chapter01.md
```

### 4. Process TXT

```bash
cargo run --release -- ./draft.txt
```

### 5. Force TXT line-by-line paragraphs

Useful for poetry, dialogue scripts, or OCR-style short lines:

```bash
cargo run --release -- --txt-hard-linebreaks ./draft.txt
```

### 6. Control concurrency and request pacing

```bash
cargo run --release -- --jobs 3 --request-delay-ms 250 ./books
```

### 7. Rebuild HTML offline

No API calls. Recreate HTML only from existing `*_state.json` files:

```bash
cargo run --release -- --rebuild ./books ./output
```

> `--rebuild` must use the same input source and output directory as the original run so the matching state files can be found.

## CLI Usage

```text
Usage: epub-reader [OPTIONS] <INPUT> [OUTPUT]

Arguments:
  <INPUT>   Input file or directory (.epub/.md/.markdown/.txt)
  [OUTPUT]  Output directory for HTML and state files [default: output]

Options:
      --rebuild
          Rebuild HTML from existing state files without API calls
      --jobs <JOBS>
          Maximum number of concurrent translation requests [default: 2]
      --request-delay-ms <REQUEST_DELAY_MS>
          Delay in milliseconds before launching each translation request [default: 0]
      --min-paragraph-chars <MIN_PARAGRAPH_CHARS>
          Minimum characters required for a text block without sentence punctuation [default: 2]
      --title-max-words <TITLE_MAX_WORDS>
          Maximum words to treat a short line as a book title candidate [default: 12]
      --heading-max-words <HEADING_MAX_WORDS>
          Maximum words to treat an uppercase short line as a heading [default: 8]
      --txt-hard-linebreaks
          In .txt files, treat each non-empty line as its own paragraph
      --txt-no-sentence-split
          In .txt files, do not start a new paragraph after sentence-ending punctuation
  -h, --help
          Print help
  -V, --version
          Print version
```

## Supported Input Formats

### EPUB

- Reads content in spine order
- Prefers extracting `p`, `blockquote`, and `li` blocks
- Falls back to `div` extraction when the document structure is unusual
- Filters some TOC, page-number, and navigation-like pages

### Markdown

- Reads `title` from YAML frontmatter when present
- If there is no frontmatter title, the first suitable `# H1` can become the book title
- `H1-H3` headings are treated as chapter candidates
- Normal paragraphs and list items become translatable text blocks

### TXT

- Blank lines and scene breaks create paragraph boundaries
- Tries to recognize headings such as `Chapter 1`, `第十二章`, and `PROLOGUE`
- By default, splits on sentence endings and indented lines
- You can adjust this with `--txt-hard-linebreaks` and `--txt-no-sentence-split`

## Common Use Cases

### Light novels / web novels in EPUB

```bash
cargo run --release -- --jobs 3 ./novels
```

### Markdown notes from Obsidian / Typora

```bash
cargo run --release -- ./notes/book-summary.md
```

### OCR-exported plain text

```bash
cargo run --release -- --txt-hard-linebreaks --min-paragraph-chars 1 ./ocr.txt
```

### Continue a partially finished run

```bash
cargo run --release -- ./books ./output
```

Run the same command again. The program reads `*_state.json` and only requests missing paragraphs.

## Output Files

The default output directory is `./output`.

```text
output/
├── book-slug.html
├── book-slug_state.json
├── another-book.html
└── another-book_state.json
```

- `*.html`
  Final reading file
- `*_state.json`
  Resume state file containing the AI responses for completed paragraphs

> Do not delete `*_state.json` unless you intentionally want to restart from scratch.

## Resume / Restart

### Continue from where you left off

Just rerun the same command:

```bash
cargo run --release -- ./books ./output
```

### Rebuild HTML without calling the API

```bash
cargo run --release -- --rebuild ./books ./output
```

### Start over completely

Delete the matching:

- `output/<slug>.html`
- `output/<slug>_state.json`

Then run the command again.

## How It Works

The core idea is not “match by position”, but “match by paragraph ID”.

```text
input file
  └─→ parse_*()
        └─→ Book / Chapter / Paragraph(id, text)
                      │
                      ├─→ html_gen: build paragraph skeleton
                      ├─→ pending: paragraphs that still need LLM calls
                      └─→ state.json: para_id -> LlmResponse
```

Current pipeline:

1. Parse the input file into a unified `Book` structure
2. Generate an HTML skeleton with placeholders for translation / vocabulary / chunks
3. Request Claude with bounded concurrency
4. Patch HTML by `para_id` as results arrive
5. Atomically write HTML first, then write `*_state.json`

This gives you:

- No paragraph misalignment even when requests finish out of order
- Safe crash behavior, where the worst case is usually redoing one paragraph
- Full `--rebuild` support without any API call

## Project Structure

```text
src/
├── main.rs            # CLI args, main flow, concurrent translation scheduling
├── parser.rs          # Input format dispatch
├── parse_utils.rs     # Shared segmentation rules, heading detection, BookBuilder
├── epub_parser.rs     # EPUB parsing
├── markdown_parser.rs # Markdown parsing
├── text_parser.rs     # TXT parsing
├── html_gen.rs        # HTML generation and paragraph patching
├── llm_client.rs      # Anthropic Messages API client
├── state.rs           # state.json read/write
├── fs_utils.rs        # Atomic file writing
├── ui.rs              # Terminal presentation
└── types.rs           # Book / Paragraph / LlmResponse structures
```

## Notes

- `ANTHROPIC_AUTH_TOKEN` is only required for translation mode; it is not needed for `--rebuild`
- If you modify the source input after starting a run, paragraph IDs may change and old state may no longer align perfectly
- `--jobs` is not always “higher is better”; `2~4` is usually a reasonable range
- For messy TXT input, try:
  - `--txt-hard-linebreaks`
  - `--min-paragraph-chars 1`
  - `--txt-no-sentence-split`

## Development

```bash
cargo fmt
cargo check
cargo test
```

To inspect the live CLI help:

```bash
cargo run -- --help
```
