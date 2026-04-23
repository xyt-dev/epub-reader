mod epub_parser;
mod fs_utils;
mod html_gen;
mod llm_client;
mod markdown_parser;
mod parse_utils;
mod parser;
mod state;
mod text_parser;
mod types;
mod ui;

use anyhow::{Context, Result};
use clap::{ArgAction, Parser};
use indicatif::ProgressBar;
use llm_client::{LlmClient, TranslationRequest};
use parse_utils::ParseOptions;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::task::JoinSet;

#[derive(Parser, Debug)]
#[command(
    name = "epub-reader",
    version,
    about = "Convert EPUB/Markdown/Text books into annotated HTML with AI translation.",
    long_about = None,
    after_help = "Examples:\n  epub-reader ../Books\n  epub-reader novel.epub\n  epub-reader notes.md ./out\n  epub-reader --jobs 3 novel.epub\n  epub-reader --txt-hard-linebreaks notes.txt ./out\n  epub-reader --rebuild ../Books ./out"
)]
struct Args {
    #[arg(
        value_name = "INPUT",
        help = "Input file or directory (.epub/.md/.markdown/.txt)"
    )]
    input: PathBuf,

    #[arg(
        value_name = "OUTPUT",
        default_value = "output",
        help = "Output directory for HTML and state files"
    )]
    output_dir: PathBuf,

    #[arg(
        long,
        help = "Rebuild HTML from existing state files without API calls"
    )]
    rebuild: bool,

    #[arg(
        long,
        default_value_t = 2,
        help = "Maximum number of concurrent translation requests"
    )]
    jobs: usize,

    #[arg(
        long,
        default_value_t = 0,
        help = "Delay in milliseconds before launching each translation request"
    )]
    request_delay_ms: u64,

    #[arg(
        long,
        default_value_t = 2,
        help = "Minimum characters required for a text block without sentence punctuation"
    )]
    min_paragraph_chars: usize,

    #[arg(
        long,
        default_value_t = 12,
        help = "Maximum words to treat a short line as a book title candidate"
    )]
    title_max_words: usize,

    #[arg(
        long,
        default_value_t = 8,
        help = "Maximum words to treat an uppercase short line as a heading"
    )]
    heading_max_words: usize,

    #[arg(
        long,
        help = "In .txt files, treat each non-empty line as its own paragraph"
    )]
    txt_hard_linebreaks: bool,

    #[arg(
        long = "txt-no-sentence-split",
        action = ArgAction::SetFalse,
        default_value_t = true,
        help = "In .txt files, do not start a new paragraph after sentence-ending punctuation"
    )]
    txt_split_on_sentence_end: bool,
}

#[derive(Debug, Clone)]
struct JobOutcome {
    book_title: String,
    total_paragraphs: usize,
    completed: usize,
    html_path: PathBuf,
    state_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct TranslationOptions {
    jobs: usize,
    request_delay: Duration,
}

#[derive(Debug, Clone)]
struct PendingParagraph {
    para_id: String,
    para_text: String,
}

#[derive(Debug, Clone)]
struct PendingBatch {
    paragraphs: Vec<PendingParagraph>,
}

#[derive(Debug)]
struct ParagraphTaskResult {
    para_id: String,
    outcome: std::result::Result<types::LlmResponse, String>,
}

#[derive(Debug)]
struct TranslationTaskResult {
    items: Vec<ParagraphTaskResult>,
}

const BATCH_TARGET_CHARS: usize = 5_000;
const BATCH_HARD_MAX_CHARS: usize = 7_000;
const BATCH_MAX_ITEMS: usize = 10;
const SINGLE_PARAGRAPH_CHARS: usize = 2_800;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    validate_args(&args)?;
    std::fs::create_dir_all(&args.output_dir)?;

    let parse_options = parse_options_from_args(&args);
    let translation_options = TranslationOptions {
        jobs: args.jobs,
        request_delay: Duration::from_millis(args.request_delay_ms),
    };

    ui::print_banner(&args.output_dir, args.rebuild);
    ui::print_kv("parse-rules", parse_options.summary());
    if !args.rebuild {
        ui::print_kv(
            "llm",
            format!(
                "{} job(s) · {}ms launch delay · adaptive batches target {} / max {} chars",
                translation_options.jobs,
                args.request_delay_ms,
                BATCH_TARGET_CHARS,
                BATCH_HARD_MAX_CHARS
            ),
        );
    }

    let inputs = collect_inputs(&args.input)?;
    if inputs.is_empty() {
        ui::print_error(format!(
            "No supported input files ({}) found under {}",
            parser::supported_extensions_summary(),
            args.input.display()
        ));
        return Ok(());
    }
    ui::print_input_summary(&args.input, inputs.len());

    let client = if args.rebuild {
        None
    } else {
        Some(LlmClient::new(
            std::env::var("ANTHROPIC_AUTH_TOKEN")
                .context("ANTHROPIC_AUTH_TOKEN env var not set")?,
        ))
    };

    let mut succeeded = 0usize;
    let mut failed = 0usize;

    for (idx, input_path) in inputs.iter().enumerate() {
        ui::print_job_header(idx + 1, inputs.len(), input_path);

        let result = if args.rebuild {
            rebuild_html(input_path, &args.output_dir, &parse_options)
        } else {
            process_input(
                input_path,
                &args.output_dir,
                client.as_ref().unwrap(),
                &parse_options,
                &translation_options,
            )
            .await
        };

        match result {
            Ok(outcome) => {
                succeeded += 1;
                ui::print_success(format!(
                    "{} · {}/{} paragraphs ready",
                    outcome.book_title, outcome.completed, outcome.total_paragraphs
                ));
                ui::print_kv("html", outcome.html_path.display().to_string());
                if let Some(state_path) = outcome.state_path {
                    ui::print_kv("state", state_path.display().to_string());
                }
            }
            Err(err) => {
                failed += 1;
                ui::print_error(format!("{}: {:#}", input_path.display(), err));
            }
        }
    }

    ui::print_run_summary(succeeded, failed);
    Ok(())
}

fn rebuild_html(
    input_path: &Path,
    output_dir: &Path,
    parse_options: &ParseOptions,
) -> Result<JobOutcome> {
    ui::print_step("parse", "reading source content");
    let book = parser::parse_book(input_path, parse_options)?;
    let total_paragraphs: usize = book
        .chapters
        .iter()
        .map(|c| c.paragraphs.iter().filter(|p| p.is_translatable()).count())
        .sum();
    ui::print_book_summary(&book.title, book.chapters.len(), total_paragraphs);

    let state_path = state::state_path(output_dir, &book.slug);
    let html_path = output_dir.join(format!("{}.html", book.slug));

    ui::print_step("state", "loading saved responses");
    let st = state::load_state(&state_path)?;
    ui::print_kv(
        "loaded",
        format!("{} cached paragraph(s)", st.completed.len()),
    );

    ui::print_step("html", "rebuilding from skeleton");
    let mut html = html_gen::generate_html(&book);
    let para_map = build_para_map(&book);

    let pb = ProgressBar::new(st.completed.len() as u64);
    pb.set_style(ui::progress_style(false));
    pb.enable_steady_tick(Duration::from_millis(80));

    for (para_id, resp) in &st.completed {
        if let Some(para) = para_map.get(para_id.as_str()) {
            html = html_gen::patch_html(&html, para, resp);
        }
        pb.inc(1);
    }
    pb.finish_and_clear();

    fs_utils::atomic_write(&html_path, html.as_bytes())?;

    Ok(JobOutcome {
        book_title: book.title,
        total_paragraphs,
        completed: st.completed.len(),
        html_path,
        state_path: state_path.exists().then_some(state_path),
    })
}

async fn process_input(
    input_path: &Path,
    output_dir: &Path,
    client: &LlmClient,
    parse_options: &ParseOptions,
    translation_options: &TranslationOptions,
) -> Result<JobOutcome> {
    ui::print_step("parse", "reading source content");
    let book = parser::parse_book(input_path, parse_options)?;
    let total_paragraphs: usize = book
        .chapters
        .iter()
        .map(|c| c.paragraphs.iter().filter(|p| p.is_translatable()).count())
        .sum();
    ui::print_book_summary(&book.title, book.chapters.len(), total_paragraphs);

    let html_path = output_dir.join(format!("{}.html", book.slug));
    let state_path = state::state_path(output_dir, &book.slug);

    ui::print_step("html", "loading or creating skeleton");
    let mut html_content = if html_path.exists() {
        std::fs::read_to_string(&html_path)?
    } else {
        let initial_html = html_gen::generate_html(&book);
        fs_utils::atomic_write(&html_path, initial_html.as_bytes())?;
        initial_html
    };

    ui::print_step("state", "loading resumable progress");
    let mut st = state::load_state(&state_path)?;
    ui::print_kv(
        "loaded",
        format!("{} cached paragraph(s)", st.completed.len()),
    );

    let pending: Vec<PendingParagraph> = book
        .chapters
        .iter()
        .flat_map(|c| c.paragraphs.iter())
        .filter(|p| p.is_translatable() && !st.is_done(&p.id) && !p.text.trim().is_empty())
        .map(|p| PendingParagraph {
            para_id: p.id.clone(),
            para_text: p.text.clone(),
        })
        .collect();

    let already_done = total_paragraphs.saturating_sub(pending.len());
    ui::print_kv(
        "progress",
        format!("{} done · {} remaining", already_done, pending.len()),
    );

    if pending.is_empty() {
        return Ok(JobOutcome {
            book_title: book.title,
            total_paragraphs,
            completed: already_done,
            html_path,
            state_path: state_path.exists().then_some(state_path),
        });
    }

    ui::print_step("translate", "requesting Claude in adaptive batches");
    let pb = ProgressBar::new(pending.len() as u64);
    pb.set_style(ui::progress_style(true));
    pb.enable_steady_tick(Duration::from_millis(80));

    let para_map = build_para_map(&book);
    let pending_batches = build_translation_batches(pending);
    ui::print_kv(
        "batching",
        format!(
            "{} request(s) queued from remaining paragraphs",
            pending_batches.len()
        ),
    );
    let mut join_set = JoinSet::new();
    let mut pending_iter = pending_batches.into_iter();
    let mut launched_any = false;

    fill_translation_queue(
        &mut join_set,
        &mut pending_iter,
        client,
        translation_options,
        &mut launched_any,
    )
    .await;

    while let Some(joined) = join_set.join_next().await {
        let task = joined.context("translation worker panicked")?;
        let batch_size = task.items.len();
        let last_id = task
            .items
            .last()
            .map(|item| abbreviate_para_id(&item.para_id))
            .unwrap_or_else(|| "-".to_string());

        for item in task.items {
            match item.outcome {
                Ok(resp) => {
                    if let Some(para) = para_map.get(item.para_id.as_str()) {
                        html_content = html_gen::patch_html(&html_content, para, &resp);
                    }

                    fs_utils::atomic_write(&html_path, html_content.as_bytes())?;
                    st.mark_done(item.para_id.clone(), resp);
                    state::save_state(&state_path, &st)?;
                }
                Err(err) => {
                    pb.println(ui::warn_text(format!("skipping {}: {}", item.para_id, err)));
                }
            }
            pb.inc(1);
        }

        fill_translation_queue(
            &mut join_set,
            &mut pending_iter,
            client,
            translation_options,
            &mut launched_any,
        )
        .await;

        if join_set.is_empty() {
            pb.set_message("finalizing".to_string());
        } else {
            pb.set_message(format!(
                "active={} · batch={} · last={}",
                join_set.len(),
                batch_size,
                last_id
            ));
        }
    }
    pb.finish_and_clear();

    Ok(JobOutcome {
        book_title: book.title,
        total_paragraphs,
        completed: st.completed.len(),
        html_path,
        state_path: state_path.exists().then_some(state_path),
    })
}

fn build_para_map<'a>(book: &'a types::Book) -> HashMap<&'a str, &'a types::Paragraph> {
    book.chapters
        .iter()
        .flat_map(|c| c.paragraphs.iter())
        .filter(|p| p.is_translatable())
        .map(|p| (p.id.as_str(), p))
        .collect()
}

async fn fill_translation_queue(
    join_set: &mut JoinSet<TranslationTaskResult>,
    pending_iter: &mut std::vec::IntoIter<PendingBatch>,
    client: &LlmClient,
    options: &TranslationOptions,
    launched_any: &mut bool,
) {
    while join_set.len() < options.jobs {
        let Some(job) = pending_iter.next() else {
            break;
        };

        if *launched_any && !options.request_delay.is_zero() {
            tokio::time::sleep(options.request_delay).await;
        }

        let client = client.clone();
        join_set.spawn(async move {
            let request_items = job
                .paragraphs
                .iter()
                .map(|paragraph| TranslationRequest {
                    id: paragraph.para_id.as_str(),
                    text: paragraph.para_text.as_str(),
                })
                .collect::<Vec<_>>();

            let items = match client.translate_batch(&request_items).await {
                Ok(responses) => responses
                    .into_iter()
                    .map(|response| ParagraphTaskResult {
                        para_id: response.id,
                        outcome: Ok(response.response),
                    })
                    .collect(),
                Err(batch_err) if job.paragraphs.len() > 1 => {
                    eprintln!(
                        "  [llm] batch of {} failed, retrying individually: {:#}",
                        job.paragraphs.len(),
                        batch_err
                    );
                    let mut fallback = Vec::with_capacity(job.paragraphs.len());
                    for paragraph in job.paragraphs {
                        let para_id = paragraph.para_id;
                        let outcome = client
                            .translate_paragraph(&para_id, &paragraph.para_text)
                            .await
                            .map_err(|err| format!("{:#}", err));
                        fallback.push(ParagraphTaskResult { para_id, outcome });
                    }
                    fallback
                }
                Err(batch_err) => job
                    .paragraphs
                    .into_iter()
                    .map(|paragraph| ParagraphTaskResult {
                        para_id: paragraph.para_id,
                        outcome: Err(format!("{:#}", batch_err)),
                    })
                    .collect(),
            };

            TranslationTaskResult { items }
        });
        *launched_any = true;
    }
}

fn build_translation_batches(pending: Vec<PendingParagraph>) -> Vec<PendingBatch> {
    let mut batches = Vec::new();
    let mut iter = pending.into_iter().peekable();

    while let Some(first) = iter.next() {
        let mut total_chars = effective_text_chars(&first.para_text);
        let mut paragraphs = vec![first];

        if total_chars > SINGLE_PARAGRAPH_CHARS {
            batches.push(PendingBatch { paragraphs });
            continue;
        }

        while paragraphs.len() < BATCH_MAX_ITEMS {
            if paragraphs.len() >= 2 && total_chars >= BATCH_TARGET_CHARS {
                break;
            }

            let Some(next) = iter.peek() else {
                break;
            };

            let next_chars = effective_text_chars(&next.para_text);
            if next_chars > SINGLE_PARAGRAPH_CHARS {
                break;
            }

            if total_chars + next_chars > BATCH_HARD_MAX_CHARS {
                break;
            }

            total_chars += next_chars;
            paragraphs.push(iter.next().unwrap());
        }

        batches.push(PendingBatch { paragraphs });
    }

    batches
}

fn effective_text_chars(text: &str) -> usize {
    text.chars().filter(|c| !c.is_whitespace()).count()
}

fn parse_options_from_args(args: &Args) -> ParseOptions {
    ParseOptions {
        min_paragraph_chars: args.min_paragraph_chars,
        title_max_words: args.title_max_words,
        short_heading_max_words: args.heading_max_words,
        txt_hard_linebreaks: args.txt_hard_linebreaks,
        txt_split_on_sentence_end: args.txt_split_on_sentence_end,
    }
}

fn validate_args(args: &Args) -> Result<()> {
    if args.jobs == 0 {
        anyhow::bail!("--jobs must be at least 1");
    }
    if args.jobs > 16 {
        anyhow::bail!("--jobs must be 16 or smaller");
    }
    if args.min_paragraph_chars == 0 {
        anyhow::bail!("--min-paragraph-chars must be at least 1");
    }
    if args.title_max_words == 0 {
        anyhow::bail!("--title-max-words must be at least 1");
    }
    if args.heading_max_words == 0 {
        anyhow::bail!("--heading-max-words must be at least 1");
    }
    Ok(())
}

fn abbreviate_para_id(para_id: &str) -> String {
    const MAX_LEN: usize = 28;
    if para_id.len() <= MAX_LEN {
        return para_id.to_string();
    }
    format!("…{}", &para_id[para_id.len() - (MAX_LEN - 1)..])
}

fn collect_inputs(path: &Path) -> Result<Vec<PathBuf>> {
    if !path.exists() {
        anyhow::bail!("Path '{}' does not exist.", path.display());
    }

    if path.is_file() {
        parser::validate_requested_input(path)?;
        return Ok(vec![path.to_path_buf()]);
    }

    let mut inputs = Vec::new();
    visit_dir(path, &mut inputs)?;
    if inputs.is_empty() {
        anyhow::bail!(
            "No supported input files ({}) found in '{}'.",
            parser::supported_extensions_summary(),
            path.display()
        );
    }

    inputs.sort();
    Ok(inputs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(id: &str, len: usize) -> PendingParagraph {
        PendingParagraph {
            para_id: id.to_string(),
            para_text: "a".repeat(len),
        }
    }

    #[test]
    fn batching_respects_max_items_cap() {
        let pending = (0..10)
            .map(|idx| pending(&format!("p{}", idx), 300))
            .collect::<Vec<_>>();

        let batches = build_translation_batches(pending);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].paragraphs.len(), 10);
    }

    #[test]
    fn batching_stops_near_target_total_chars() {
        let pending = vec![
            pending("p1", 2_600),
            pending("p2", 2_400),
            pending("p3", 300),
        ];

        let batches = build_translation_batches(pending);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].paragraphs.len(), 2);
        assert_eq!(batches[1].paragraphs.len(), 1);
    }

    #[test]
    fn batching_respects_hard_char_limit() {
        let pending = (0..3)
            .map(|idx| pending(&format!("p{}", idx), 2_400))
            .collect::<Vec<_>>();

        let batches = build_translation_batches(pending);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].paragraphs.len(), 2);
        assert_eq!(batches[1].paragraphs.len(), 1);
    }

    #[test]
    fn oversized_paragraphs_are_sent_alone() {
        let pending = vec![pending("p1", 2_900), pending("p2", 2_950)];
        let batches = build_translation_batches(pending);

        assert_eq!(batches.len(), 2);
        assert!(batches.iter().all(|batch| batch.paragraphs.len() == 1));
    }

    #[test]
    fn effective_text_chars_ignores_whitespace() {
        assert_eq!(effective_text_chars("ab c\n d\t"), 4);
    }
}

fn visit_dir(dir: &Path, inputs: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            visit_dir(&path, inputs)?;
        } else if parser::is_enabled_input(&path) {
            inputs.push(path);
        }
    }
    Ok(())
}
