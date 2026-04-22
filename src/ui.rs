use console::style;
use indicatif::ProgressStyle;
use std::path::Path;

pub fn print_banner(output_dir: &Path, rebuild: bool) {
    let rule = style("─".repeat(72)).color256(238);
    println!("{}", rule);
    println!(
        "{} {}",
        style("epub-reader").bold().color256(153),
        style(format!("v{}", env!("CARGO_PKG_VERSION"))).color256(244)
    );
    println!(
        "{} {}",
        style("formats").color256(244),
        style("EPUB · Markdown · Text").color256(250)
    );
    println!(
        "{} {}",
        style("mode").color256(244),
        style(if rebuild { "rebuild" } else { "translate" }).color256(151)
    );
    println!(
        "{} {}",
        style("output").color256(244),
        style(output_dir.display()).color256(250)
    );
    println!("{}", rule);
}

pub fn print_input_summary(input_root: &Path, count: usize) {
    println!(
        "{} {} {}",
        style("scan").color256(244),
        style(input_root.display()).color256(250),
        style(format!("· {} file(s)", count)).color256(244)
    );
}

pub fn print_job_header(index: usize, total: usize, input_path: &Path) {
    let rule = style("─".repeat(72)).color256(238);
    println!();
    println!("{}", rule);
    println!(
        "{} {} {}",
        style(format!("[{}/{}]", index, total)).color256(244),
        style("processing").bold().color256(153),
        style(input_path.display()).color256(250)
    );
}

pub fn print_book_summary(title: &str, chapters: usize, paragraphs: usize) {
    println!(
        "{} {} {} {} {} {} {} {}",
        style("book").color256(244),
        style(title).color256(230),
        style("·").color256(240),
        style(chapters).color256(151),
        style("chapter(s) ·").color256(244),
        style(paragraphs).color256(151),
        style("paragraph(s)").color256(244),
        style("ready").color256(244)
    );
}

pub fn print_step(label: &str, detail: impl AsRef<str>) {
    println!(
        "{} {}",
        style(label).bold().color256(153),
        style(detail.as_ref()).color256(250)
    );
}

pub fn print_kv(label: &str, value: impl AsRef<str>) {
    println!(
        "{} {}",
        style(label).color256(244),
        style(value.as_ref()).color256(250)
    );
}

pub fn print_success(message: impl AsRef<str>) {
    println!(
        "{} {}",
        style("done").bold().color256(151),
        style(message.as_ref()).color256(250)
    );
}

pub fn print_error(message: impl AsRef<str>) {
    eprintln!(
        "{} {}",
        style("error").bold().color256(203),
        style(message.as_ref()).color256(252)
    );
}

pub fn print_run_summary(succeeded: usize, failed: usize) {
    let rule = style("─".repeat(72)).color256(238);
    println!();
    println!("{}", rule);
    println!(
        "{} {} {} {}",
        style("completed").bold().color256(153),
        style(format!("{} succeeded", succeeded)).color256(151),
        style("·").color256(240),
        style(format!("{} failed", failed)).color256(if failed == 0 { 244 } else { 203 })
    );
}

pub fn progress_style(with_message: bool) -> ProgressStyle {
    let template = if with_message {
        "{spinner:.cyan} {bar:34.cyan/blue} {pos:>4}/{len:<4} {msg}"
    } else {
        "{spinner:.cyan} {bar:34.cyan/blue} {pos:>4}/{len:<4}"
    };

    ProgressStyle::with_template(template)
        .unwrap()
        .progress_chars("█▉▊▋▌▍▎▏  ")
        .tick_strings(&["⠁", "⠂", "⠄", "⡀", "⢀", "⠠", "⠐", "⠈", "⠁"])
}

pub fn warn_text(message: impl AsRef<str>) -> String {
    format!(
        "{} {}",
        style("warn").bold().color256(214),
        style(message.as_ref()).color256(252)
    )
}
