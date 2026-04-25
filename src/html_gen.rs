use crate::types::{Book, LlmResponse, Paragraph, ParagraphKind};
use html_escape::encode_text;
use std::fmt::Write as FmtWrite;
use std::io::Cursor;
use std::sync::OnceLock;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::html::highlighted_html_for_string;
use syntect::parsing::{SyntaxReference, SyntaxSet};

struct CodeHighlightAssets {
    syntax_set: SyntaxSet,
    theme: Theme,
}

/// Generate a full HTML page for the given book.
/// Each paragraph gets placeholder `<details>` sections.
pub fn generate_html(book: &Book) -> String {
    let mut body = String::new();
    let toc = render_toc(book);

    for chapter in &book.chapters {
        let ch_title = chapter
            .title
            .clone()
            .unwrap_or_else(|| format!("Chapter {}", chapter.index + 1));

        writeln!(
            body,
            r#"<section class="chapter" id="ch{:03}">"#,
            chapter.index
        )
        .unwrap();
        writeln!(
            body,
            r#"  <h2 class="chapter-title">{}</h2>"#,
            encode_text(&ch_title)
        )
        .unwrap();

        for para in &chapter.paragraphs {
            body.push_str(&render_chapter_block(para, None));
        }

        body.push_str("</section>\n");
    }

    HTML_TEMPLATE
        .replace("{{TITLE}}", &encode_text(&book.title))
        .replace("{{SLUG}}", &book.slug)
        .replace("{{TOC}}", &toc)
        .replace("{{BODY}}", &body)
}

fn render_chapter_block(para: &Paragraph, resp: Option<&LlmResponse>) -> String {
    match &para.kind {
        ParagraphKind::Text => render_para_block(para, resp),
        ParagraphKind::CodeBlock { language } => render_code_block(para, language.as_deref()),
    }
}

/// Render a single paragraph block. If `resp` is Some, fills in the LLM content.
pub fn render_para_block(para: &Paragraph, resp: Option<&LlmResponse>) -> String {
    let status = if resp.is_some() { "done" } else { "pending" };
    let original = encode_text(&para.text);

    let translation_html = match resp {
        Some(r) => format!("<p>{}</p>", encode_text(&r.translation)),
        None => "<!-- FILL:translation -->".to_string(),
    };

    let vocab_html = match resp {
        Some(r) => render_vocab(&r.vocabulary),
        None => "<!-- FILL:vocab -->".to_string(),
    };

    let chunks_html = match resp {
        Some(r) => render_chunks(&r.chunks),
        None => "<!-- FILL:chunks -->".to_string(),
    };

    format!(
        r#"<div class="para-block" id="{id}" data-status="{status}">
  <p class="original-text">{original}</p>
  <details class="ai-section translation-section" data-detail-key="{id}:translation">
    <summary><span class="section-icon">🈳</span> 译文</summary>
    <div class="ai-content">{translation_html}</div>
  </details>
  <details class="ai-section vocab-section" data-detail-key="{id}:vocab">
    <summary><span class="section-icon">📚</span> 词汇 (IELTS 6.5+)</summary>
    <div class="ai-content">{vocab_html}</div>
  </details>
  <details class="ai-section chunk-section" data-detail-key="{id}:chunks">
    <summary><span class="section-icon">🔗</span> 常用短语 / Chunks</summary>
    <div class="ai-content">{chunks_html}</div>
  </details>
</div>
"#,
        id = para.id,
        status = status,
        original = original,
        translation_html = translation_html,
        vocab_html = vocab_html,
        chunks_html = chunks_html,
    )
}

fn render_code_block(para: &Paragraph, language: Option<&str>) -> String {
    let label = language
        .map(str::trim)
        .filter(|lang| !lang.is_empty())
        .unwrap_or("code");
    let highlighted = highlight_code_html(&para.text, language);

    format!(
        r#"<figure class="code-block" id="{id}">
  <figcaption class="code-block-label">{label}</figcaption>
  <div class="code-block-html">{highlighted}</div>
</figure>
"#,
        id = para.id,
        label = encode_text(label),
        highlighted = highlighted,
    )
}

fn highlight_code_html(code: &str, language: Option<&str>) -> String {
    let assets = code_highlight_assets();
    let syntax = pick_syntax(&assets.syntax_set, language, code);

    highlighted_html_for_string(code, &assets.syntax_set, syntax, &assets.theme).unwrap_or_else(
        |_| {
            format!(
                "<pre><code>{}</code></pre>",
                encode_text(code.trim_matches('\n'))
            )
        },
    )
}

fn pick_syntax<'a>(
    syntax_set: &'a SyntaxSet,
    language: Option<&str>,
    code: &str,
) -> &'a SyntaxReference {
    language
        .and_then(|lang| normalized_language_token(lang))
        .and_then(|lang| syntax_set.find_syntax_by_token(&lang))
        .or_else(|| syntax_set.find_syntax_by_first_line(code))
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text())
}

fn normalized_language_token(language: &str) -> Option<String> {
    let token = language.trim().to_ascii_lowercase();
    if token.is_empty() {
        return None;
    }

    let normalized = match token.as_str() {
        "c++" => "cpp",
        "c#" => "cs",
        "js" => "javascript",
        "ts" => "typescript",
        "py" => "python",
        "rb" => "ruby",
        "rs" => "rust",
        "sh" => "bash",
        "shell" => "bash",
        "zsh" => "bash",
        "ps1" => "powershell",
        "yml" => "yaml",
        "md" => "markdown",
        other => other,
    };

    Some(normalized.to_string())
}

fn code_highlight_assets() -> &'static CodeHighlightAssets {
    static ASSETS: OnceLock<CodeHighlightAssets> = OnceLock::new();
    ASSETS.get_or_init(|| {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let mut reader = Cursor::new(include_bytes!("../assets/catppuccin-mocha.tmTheme"));
        let theme =
            ThemeSet::load_from_reader(&mut reader).expect("failed to load Catppuccin Mocha theme");

        CodeHighlightAssets { syntax_set, theme }
    })
}

fn render_toc(book: &Book) -> String {
    let mut s = String::from(r#"<nav class="toc-nav" aria-label="Chapter navigation">"#);

    for chapter in &book.chapters {
        let title = chapter
            .title
            .clone()
            .unwrap_or_else(|| format!("Chapter {}", chapter.index + 1));
        let chapter_id = format!("ch{:03}", chapter.index);

        s.push_str(&format!(
            r##"<a class="toc-link" href="#{id}" data-chapter-id="{id}"><span class="toc-link-index">{index:02}</span><span class="toc-link-title">{title}</span><span class="toc-link-meta">{para_count} p</span></a>"##,
            id = chapter_id,
            index = chapter.index + 1,
            title = encode_text(&title),
            para_count = chapter.paragraphs.iter().filter(|p| p.is_translatable()).count(),
        ));
    }

    s.push_str("</nav>");
    s
}

fn render_vocab(entries: &[crate::types::VocabEntry]) -> String {
    if entries.is_empty() {
        return "<p class=\"empty\">—</p>".to_string();
    }
    let mut s = String::from(
        r#"<div class="vocab-scroll"><table class="vocab-table"><thead><tr><th>单词</th><th>音标</th><th>词性</th><th>释义</th><th>例句</th></tr></thead><tbody>"#,
    );
    for e in entries {
        s.push_str(&format!(
            "<tr><td class=\"word\">{}</td><td class=\"ipa\">{}</td><td class=\"pos\">{}</td><td>{}</td><td class=\"example\"><em>{}</em></td></tr>",
            encode_text(&e.word),
            encode_text(&e.ipa),
            encode_text(&e.pos),
            encode_text(&e.cn),
            encode_text(&e.example),
        ));
    }
    s.push_str("</tbody></table></div>");
    s
}

fn render_chunks(entries: &[crate::types::ChunkEntry]) -> String {
    if entries.is_empty() {
        return "<p class=\"empty\">—</p>".to_string();
    }
    let mut s = String::from(r#"<ul class="chunk-list">"#);
    for e in entries {
        s.push_str(&format!(
            r#"<li><span class="chunk-phrase">{}</span> <span class="chunk-cn">（{}）</span><br><em class="chunk-example">{}</em></li>"#,
            encode_text(&e.chunk),
            encode_text(&e.cn),
            encode_text(&e.example),
        ));
    }
    s.push_str("</ul>");
    s
}

/// Update a single paragraph block inside an existing HTML string in-place.
/// Finds the `<div class="para-block" id="{id}" ...>` block and replaces it.
pub fn patch_html(html: &str, para: &Paragraph, resp: &LlmResponse) -> String {
    if !para.is_translatable() {
        return html.to_string();
    }

    let new_block = render_para_block(para, Some(resp));

    // Find the start tag by id attribute
    let id_marker = format!("id=\"{}\"", para.id);
    let start = match html.find(&id_marker) {
        Some(pos) => {
            // Walk back to find the `<div`
            match html[..pos].rfind("<div") {
                Some(p) => p,
                None => return html.to_string(),
            }
        }
        None => return html.to_string(),
    };

    // Find the matching closing `</div>` — count nesting depth.
    // Operate on raw bytes so emoji/multibyte chars never cause a slice-boundary panic.
    let after_start = &html[start..];
    let bytes = after_start.as_bytes();
    let mut depth = 0usize;
    let mut end = start;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"<div") {
            depth += 1;
            i += 4;
        } else if bytes[i..].starts_with(b"</div>") {
            if depth == 1 {
                end = start + i + 6; // include `</div>`
                break;
            }
            depth -= 1;
            i += 6;
        } else {
            i += 1;
        }
    }

    if end == start {
        return html.to_string();
    }

    format!("{}{}{}", &html[..start], new_block, &html[end..])
}

// ─── HTML Template ────────────────────────────────────────────────────────────

const HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>{{TITLE}}</title>
  <style>
    /* ── Reset & Base ───────────────────────────────────────────── */
    *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
    :root {
      --bg:        #1a1b26;
      --bg2:       #24283b;
      --bg3:       #2d3149;
      --surface:   #1f2335;
      --border:    #3b4168;
      --text:      #c0caf5;
      --text-dim:  #565f89;
      --accent:    #d6b36a;
      --accent-bright: #f0d08c;
      --accent-soft: rgba(214, 179, 106, .16);
      --accent-border: rgba(214, 179, 106, .28);
      --green:     #9ece6a;
      --yellow:    #e0af68;
      --red:       #f7768e;
      --cyan:      #d9c0ff;
      --purple:    #a875ff;
      --rare:      #8a52db;
      --rare-soft: rgba(138, 82, 219, .18);
      --rare-deep: rgba(72, 34, 104, .74);
      --orange:    #ff9e64;
      --radius:    8px;
      font-size:   17px;
    }
    body {
      background: var(--bg);
      color: var(--text);
      font-family: 'Georgia', 'Noto Serif SC', serif;
      line-height: 1.85;
      max-width: 860px;
      margin: 0 auto;
      padding: 2rem 1.5rem 6rem;
    }
    body.toc-open { overflow: hidden; }
    a { color: var(--accent); }

    /* ── Floating UI ────────────────────────────────────────────── */
    #toc-toggle {
      position: fixed;
      top: 1rem;
      right: 1rem;
      z-index: 140;
      border: 1px solid var(--accent-border);
      background:
        linear-gradient(135deg, rgba(47, 30, 67, .92) 0%, rgba(28, 24, 44, .92) 100%);
      color: var(--accent-bright);
      font: 600 .82rem/1 'Segoe UI', system-ui, sans-serif;
      letter-spacing: .04em;
      border-radius: 999px;
      padding: .58rem .9rem;
      cursor: pointer;
      backdrop-filter: blur(12px);
      box-shadow:
        0 12px 30px rgba(0,0,0,.26),
        inset 0 0 0 1px rgba(255, 228, 163, .05),
        0 0 24px rgba(138, 82, 219, .12);
    }
    #toc-toggle:focus-visible {
      outline: none;
      box-shadow:
        0 0 0 1px rgba(240, 208, 140, .55),
        0 0 0 4px rgba(138, 82, 219, .26),
        0 12px 30px rgba(0,0,0,.26);
    }
    #toc-backdrop {
      position: fixed;
      inset: 0;
      z-index: 118;
      background: rgba(7, 10, 20, .48);
      opacity: 0;
      pointer-events: none;
      transition: opacity .2s ease;
    }
    body.toc-open #toc-backdrop {
      opacity: 1;
      pointer-events: auto;
    }
    #toc-panel {
      position: fixed;
      top: 0;
      right: 0;
      z-index: 130;
      width: min(24rem, 92vw);
      height: 100vh;
      padding: 1.25rem 1rem 1.4rem;
      background:
        linear-gradient(180deg, rgba(34, 24, 50, .98) 0%, rgba(19, 16, 31, .98) 100%);
      border-left: 1px solid rgba(214, 179, 106, .14);
      box-shadow: -24px 0 48px rgba(0,0,0,.34);
      overflow-y: auto;
      transform: translateX(104%);
      transition: transform .25s ease;
      backdrop-filter: blur(18px);
    }
    body.toc-open #toc-panel { transform: translateX(0); }
    .toc-head {
      margin-bottom: 1rem;
      padding-bottom: .9rem;
      border-bottom: 1px solid rgba(214, 179, 106, .12);
    }
    .toc-kicker {
      font: 700 .68rem/1 'Segoe UI', system-ui, sans-serif;
      letter-spacing: .18em;
      text-transform: uppercase;
      color: var(--text-dim);
      margin-bottom: .45rem;
    }
    .toc-book-title {
      color: var(--accent);
      font-size: 1.02rem;
      line-height: 1.45;
      margin-bottom: .55rem;
      word-break: break-word;
    }
    #toc-loc-inline {
      color: var(--text-dim);
      font: 600 .76rem/1.35 'Segoe UI', system-ui, sans-serif;
      min-height: 1.2rem;
    }
    #reading-loc {
      display: inline-flex;
      align-items: center;
      gap: .4rem;
      border: 1px solid var(--accent-border);
      background:
        linear-gradient(135deg, rgba(39, 27, 55, .82) 0%, rgba(20, 19, 33, .8) 100%);
      color: var(--text);
      font: 600 .76rem/1.25 'Segoe UI', system-ui, sans-serif;
      padding: .5rem .75rem;
      border-radius: 999px;
      max-width: min(32rem, calc(100vw - 2rem));
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
      backdrop-filter: blur(10px);
      box-shadow:
        0 8px 24px rgba(0,0,0,.24),
        inset 0 0 0 1px rgba(255, 228, 163, .04),
        0 0 24px rgba(138, 82, 219, .1);
    }
    #reading-loc.is-floating {
      position: fixed;
      left: max(1rem, calc(50% - 500px));
      bottom: 1.75rem;
      z-index: 105;
    }
    .toc-nav {
      display: flex;
      flex-direction: column;
      gap: .45rem;
      padding-bottom: 4rem;
    }
    .toc-link {
      display: grid;
      grid-template-columns: auto 1fr auto;
      align-items: baseline;
      gap: .7rem;
      padding: .68rem .8rem;
      border-radius: 12px;
      text-decoration: none;
      color: var(--text);
      background: rgba(255,255,255,.02);
      border: 1px solid transparent;
      transition: transform .16s ease, border-color .16s ease, background .16s ease;
    }
    .toc-link:hover {
      transform: translateX(-2px);
      background: linear-gradient(90deg, rgba(138, 82, 219, .12) 0%, rgba(214, 179, 106, .05) 100%);
      border-color: rgba(214, 179, 106, .14);
    }
    .toc-link.is-current {
      background:
        linear-gradient(90deg, rgba(91, 47, 150, .24) 0%, rgba(214, 179, 106, .08) 100%);
      border-color: var(--accent-border);
      box-shadow:
        inset 0 0 0 1px rgba(214, 179, 106, .08),
        0 0 20px rgba(138, 82, 219, .08);
    }
    .toc-link:focus-visible {
      outline: none;
      border-color: rgba(240, 208, 140, .44);
      box-shadow:
        inset 0 0 0 1px rgba(240, 208, 140, .1),
        0 0 0 3px rgba(138, 82, 219, .18);
    }
    .toc-link-index {
      color: var(--text-dim);
      font: 700 .72rem/1 'Segoe UI', system-ui, sans-serif;
      letter-spacing: .08em;
      min-width: 2ch;
    }
    .toc-link-title {
      color: var(--text);
      font: 600 .86rem/1.35 'Segoe UI', system-ui, sans-serif;
    }
    .toc-link-meta {
      color: var(--text-dim);
      font: 600 .72rem/1 'Segoe UI', system-ui, sans-serif;
    }

    /* ── Progress bar (bottom) ──────────────────────────────────── */
    #progress-bar-wrap {
      position: fixed; bottom: 1.2rem; left: 50%;
      transform: translateX(-50%);
      width: min(1000px, 90%); height: 5px;
      background: rgba(255,255,255,.08);
      border-radius: 9999px;
      z-index: 100;
      box-shadow: 0 0 12px rgba(0,0,0,.5);
    }
    #progress-pct {
      position: fixed; bottom: 1.9rem; right: calc(50% - min(500px, 45%) + 0px);
      font-size: .7rem; font-family: monospace;
      color: rgba(255,255,255,.35);
      z-index: 100;
      pointer-events: none;
      user-select: none;
    }
    #progress-bar {
      height: 100%;
      width: 0%;
      border-radius: 9999px;
      transition: width .25s ease;
      background: linear-gradient(90deg, #8c63e8 0%, #c18fff 40%, #e2bf79 78%, #f3e1b2 100%);
      box-shadow:
        0 0 12px rgba(168, 117, 255, .24),
        0 0 28px rgba(214, 179, 106, .18);
      position: relative;
      overflow: hidden;
    }
    #progress-bar::before {
      content: '';
      position: absolute;
      inset: 0;
      background: linear-gradient(180deg, rgba(255,255,255,.22) 0%, rgba(255,255,255,0) 74%);
      pointer-events: none;
    }
    #progress-bar::after {
      content: '';
      position: absolute;
      right: -1px; top: 50%;
      transform: translateY(-50%);
      width: 5px; height: 5px;
      border-radius: 50%;
      background: #ffe066;
      box-shadow: 0 0 8px 3px #ffe077, 0 0 20px 6px rgba(255,210,50,.7), 0 0 36px 8px rgba(255,180,0,.35);
      opacity: 1;
      transition: opacity .25s;
    }
    #progress-bar[style*="width: 0"]::after { opacity: 0; }

    /* ── Chapter ───────────────────────────────────────────────── */
    .chapter {
      margin-bottom: 4rem;
      scroll-margin-top: 1rem;
    }
    .chapter-title {
      font-size: 1.6rem; color: #c89cff;
      border-bottom: 2px solid var(--border);
      padding-bottom: .4rem; margin-bottom: 2rem;
    }

    /* ── Paragraph block ───────────────────────────────────────── */
    .para-block {
      margin-bottom: 2rem;
      border-left: 3px solid var(--border);
      padding-left: 1rem;
      transition: border-color .2s;
      scroll-margin-top: 1rem;
    }
    .para-block[data-status="done"] { border-left-color: var(--green); }
    .para-block[data-status="pending"] { border-left-color: var(--border); }
    .para-block.is-current {
      border-left-color: var(--accent);
      background: linear-gradient(90deg, rgba(138, 82, 219, .08) 0%, rgba(214, 179, 106, .035) 100%);
      box-shadow:
        -10px 0 24px rgba(138, 82, 219, .12),
        inset 1px 0 0 rgba(214, 179, 106, .1);
    }

    .original-text {
      font-size: 1rem;
      color: var(--text);
      margin-bottom: .6rem;
      text-align: justify;
    }
    .para-block.is-current .original-text {
      color: #f1e6cb;
      text-shadow:
        0 0 10px rgba(214, 179, 106, .08),
        0 0 18px rgba(138, 82, 219, .05);
    }
    /* ── Collapsible AI sections ───────────────────────────────── */
    .ai-section {
      margin-top: .35rem;
      border-radius: var(--radius);
      overflow: hidden;
    }
    .ai-section > summary {
      cursor: pointer;
      padding: .3rem .7rem;
      font-size: .82rem;
      font-family: 'Segoe UI', system-ui, sans-serif;
      font-weight: 600;
      letter-spacing: .03em;
      list-style: none;
      display: flex; align-items: center; gap: .4rem;
      user-select: none;
    }
    .ai-section > summary::-webkit-details-marker { display: none; }
    .ai-section > summary::before {
      content: '▶'; font-size: .6rem; transition: transform .15s;
    }
    .ai-section[open] > summary::before { transform: rotate(90deg); }
    .ai-section > summary:focus-visible {
      outline: none;
      box-shadow:
        inset 0 0 0 1px rgba(240, 208, 140, .12),
        0 0 0 3px rgba(138, 82, 219, .16);
    }

    .translation-section > summary { background: #1e2940; color: var(--cyan); }
    .vocab-section      > summary { background: #201e30; color: var(--purple); }
    .chunk-section      > summary { background: #1e2a20; color: var(--green); }

    .ai-content {
      padding: .7rem 1rem;
      font-size: .9rem;
      font-family: 'Segoe UI', system-ui, sans-serif;
      line-height: 1.7;
      background: var(--surface);
    }

    /* Translation */
    .translation-section .ai-content p { color: var(--cyan); }

    /* Code block */
    .code-block {
      margin: 1.1rem 0 1.8rem;
      border: 1px solid rgba(122, 162, 247, .12);
      border-radius: 14px;
      overflow: hidden;
      background: rgba(16, 19, 31, .92);
      box-shadow: 0 14px 30px rgba(0,0,0,.18);
    }
    .code-block-label {
      padding: .45rem .8rem;
      color: var(--text-dim);
      background: rgba(122, 162, 247, .06);
      border-bottom: 1px solid rgba(122, 162, 247, .08);
      font: 700 .73rem/1 'Segoe UI', system-ui, sans-serif;
      letter-spacing: .08em;
      text-transform: uppercase;
    }
    .code-block-html {
      overflow-x: auto;
    }
    .code-block-html pre {
      overflow-x: auto;
      margin: 0;
      padding: .95rem 1rem 1.05rem;
    }
    .code-block-html code {
      display: block;
      color: #d5defc;
      font: .86rem/1.6 'JetBrains Mono', 'Fira Code', 'SFMono-Regular', Consolas, monospace;
      white-space: pre;
      tab-size: 2;
    }
    .code-block-html pre[style] {
      border-radius: 0;
      margin: 0;
      background: #161821 !important;
    }

    /* Vocab table */
    .vocab-scroll {
      overflow-x: auto;
      -webkit-overflow-scrolling: touch;
    }
    .vocab-table {
      width: 100%; border-collapse: collapse;
      font-size: .82rem;
    }
    .vocab-table th {
      background: var(--bg2); color: var(--text-dim);
      font-weight: 600; text-align: left;
      padding: .3rem .5rem;
      border-bottom: 1px solid var(--border);
    }
    .vocab-table td {
      padding: .3rem .5rem;
      border-bottom: 1px solid var(--bg3);
      vertical-align: top;
    }
    .vocab-table tr:last-child td { border-bottom: none; }
    .vocab-table .word    { color: var(--yellow); font-weight: 700; }
    .vocab-table .ipa     { color: var(--text-dim); font-family: monospace; }
    .vocab-table .pos     { color: var(--orange); font-style: italic; }
    .vocab-table .example { color: var(--text-dim); }

    /* Chunk list */
    .chunk-list { list-style: none; }
    .chunk-list li { margin-bottom: .6rem; }
    .chunk-phrase { color: var(--green); font-weight: 700; }
    .chunk-cn     { color: var(--text-dim); font-size: .82rem; }
    .chunk-example { color: var(--text-dim); font-size: .85rem; }

    .empty { color: var(--text-dim); font-style: italic; }

    /* ── Scrollbar ─────────────────────────────────────────────── */
    ::-webkit-scrollbar { width: 6px; }
    ::-webkit-scrollbar-track { background: var(--bg); }
    ::-webkit-scrollbar-thumb { background: var(--border); border-radius: 3px; }

    /* ── Responsive ────────────────────────────────────────────── */
    @media (max-width: 760px) {
      #toc-panel { width: min(26rem, 96vw); }
      #reading-loc.is-floating {
        left: .8rem;
        right: .8rem;
        bottom: 3rem;
        max-width: none;
      }
      #progress-pct { right: 1rem; bottom: 1.9rem; }
    }
    @media (max-width: 600px) {
      body { font-size: 15px; padding: 1rem .8rem 5.6rem; }
      #toc-toggle { top: .8rem; right: .8rem; }
      .vocab-table { font-size: .75rem; }
    }
  </style>
</head>
<body>
  <button id="toc-toggle" type="button" aria-expanded="false" aria-controls="toc-panel">Chapters</button>
  <div id="toc-backdrop" aria-hidden="true"></div>
  <aside id="toc-panel" aria-hidden="true">
    <div class="toc-head">
      <div class="toc-kicker">Navigator</div>
      <div class="toc-book-title">{{TITLE}}</div>
      <div id="toc-loc-inline"></div>
    </div>
    {{TOC}}
  </aside>

  <div id="progress-bar-wrap"><div id="progress-bar"></div></div>
  <div id="progress-pct">0.00%</div>
  <div id="reading-loc" class="is-floating">Locating…</div>

  <h1 style="color:var(--accent);margin-bottom:2.5rem;font-size:2rem;">{{TITLE}}</h1>

  {{BODY}}

  <script>
    // Reading state and navigation
    const bar = document.getElementById('progress-bar');
    const pctEl = document.getElementById('progress-pct');
    const readingLocEl = document.getElementById('reading-loc');
    const tocLocInlineEl = document.getElementById('toc-loc-inline');
    const tocToggle = document.getElementById('toc-toggle');
    const tocPanel = document.getElementById('toc-panel');
    const tocBackdrop = document.getElementById('toc-backdrop');
    const tocLinks = Array.from(document.querySelectorAll('.toc-link'));
    const paraBlocks = Array.from(document.querySelectorAll('.para-block'));
    const detailSections = Array.from(document.querySelectorAll('.ai-section[data-detail-key]'));
    const POSITION_KEY = 'reading-position:{{SLUG}}';
    const DETAILS_KEY = 'open-details:{{SLUG}}';
    const VIEWPORT_ANCHOR_RATIO = 0.5;
    let currentParaId = null;
    let rafPending = false;

    function safeParse(raw, fallback) {
      if (!raw) return fallback;
      try {
        return JSON.parse(raw);
      } catch (_) {
        return fallback;
      }
    }

    function saveJson(key, value) {
      try {
        localStorage.setItem(key, JSON.stringify(value));
      } catch (_) {}
    }

    function getScrollTop() {
      return window.scrollY || document.documentElement.scrollTop || 0;
    }

    function setTocOpen(open) {
      document.body.classList.toggle('toc-open', open);
      tocToggle.setAttribute('aria-expanded', open ? 'true' : 'false');
      tocPanel.setAttribute('aria-hidden', open ? 'false' : 'true');
    }

    function getCurrentParagraph() {
      if (!paraBlocks.length) return null;
      const anchorY = getScrollTop() + window.innerHeight * VIEWPORT_ANCHOR_RATIO;
      let low = 0;
      let high = paraBlocks.length - 1;
      let best = 0;

      while (low <= high) {
        const mid = Math.floor((low + high) / 2);
        if (paraBlocks[mid].offsetTop <= anchorY) {
          best = mid;
          low = mid + 1;
        } else {
          high = mid - 1;
        }
      }

      return paraBlocks[best];
    }

    function updateCurrentHighlight(para) {
      if (currentParaId === para.id) return;
      if (currentParaId) {
        document.getElementById(currentParaId)?.classList.remove('is-current');
      }
      para.classList.add('is-current');
      currentParaId = para.id;
    }

    function updateChapterHighlight(chapterId) {
      for (const link of tocLinks) {
        const active = link.dataset.chapterId === chapterId;
        link.classList.toggle('is-current', active);
        if (active) {
          link.setAttribute('aria-current', 'location');
        } else {
          link.removeAttribute('aria-current');
        }
      }
    }

    function saveReadingPosition(para) {
      const viewportAnchor = window.innerHeight * VIEWPORT_ANCHOR_RATIO;
      const payload = {
        paraId: para.id,
        paraIndex: Number(para.dataset.paraIndex || '0'),
        withinParaOffset: Math.max(0, Math.round(getScrollTop() + viewportAnchor - para.offsetTop)),
      };
      saveJson(POSITION_KEY, payload);
    }

    function updateReadingState() {
      const current = getCurrentParagraph();
      if (!current) return;

      const total = paraBlocks.length || 1;
      const index = Number(current.dataset.paraIndex || '1');
      const pct = total <= 1 ? 100 : ((index - 1) / (total - 1)) * 100;
      const chapter = current.closest('.chapter');
      const chapterId = chapter?.id || '';
      const chapterTitle = chapter?.querySelector('.chapter-title')?.textContent?.trim() || 'Current position';
      const locText = `${chapterTitle} · ${index}/${total}`;

      bar.style.width = pct + '%';
      pctEl.textContent = pct.toFixed(2) + '%';
      readingLocEl.textContent = locText;
      tocLocInlineEl.textContent = locText;
      updateCurrentHighlight(current);
      updateChapterHighlight(chapterId);
      saveReadingPosition(current);
    }

    function scheduleReadingState() {
      if (rafPending) return;
      rafPending = true;
      requestAnimationFrame(() => {
        rafPending = false;
        updateReadingState();
      });
    }

    function restoreOpenDetails() {
      const openKeys = new Set(safeParse(localStorage.getItem(DETAILS_KEY), []));
      for (const section of detailSections) {
        section.open = openKeys.has(section.dataset.detailKey);
      }
    }

    function persistOpenDetails() {
      const openKeys = detailSections
        .filter(section => section.open)
        .map(section => section.dataset.detailKey);
      saveJson(DETAILS_KEY, openKeys);
    }

    function restoreReadingPosition() {
      if (window.location.hash) {
        scheduleReadingState();
        return;
      }

      const saved = safeParse(localStorage.getItem(POSITION_KEY), null);
      if (!saved) {
        scheduleReadingState();
        return;
      }

      const target =
        document.getElementById(saved.paraId) ||
        paraBlocks[Math.max(0, Number(saved.paraIndex || 1) - 1)];

      if (!target) {
        scheduleReadingState();
        return;
      }

      const viewportAnchor = window.innerHeight * VIEWPORT_ANCHOR_RATIO;
      const top = Math.max(
        0,
        target.offsetTop + Number(saved.withinParaOffset || 0) - viewportAnchor
      );
      window.scrollTo({ top, behavior: 'auto' });
      scheduleReadingState();
    }

    paraBlocks.forEach((para, index) => {
      para.dataset.paraIndex = String(index + 1);
    });

    restoreOpenDetails();

    detailSections.forEach(section => {
      section.addEventListener('toggle', () => {
        persistOpenDetails();
        scheduleReadingState();
      });
    });

    tocToggle.addEventListener('click', () => {
      setTocOpen(!document.body.classList.contains('toc-open'));
    });
    tocBackdrop.addEventListener('click', () => setTocOpen(false));
    tocLinks.forEach(link => {
      link.addEventListener('click', () => setTocOpen(false));
    });

    window.addEventListener('keydown', event => {
      if (event.key === 'Escape') setTocOpen(false);
    });
    window.addEventListener('scroll', scheduleReadingState, { passive: true });
    window.addEventListener('resize', scheduleReadingState);
    window.addEventListener('hashchange', scheduleReadingState);
    window.addEventListener('load', () => {
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          restoreReadingPosition();
        });
      });
    });
  </script>
</body>
</html>
"#;
