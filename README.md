# epub-reader — EPUB / Markdown / TXT 转 HTML + AI 逐段翻译工具

[English](README_en.md)

> 把 `.epub`、`.md/.markdown`、`.txt` 转成可阅读 HTML，并调用 Claude 为每段生成译文、词汇讲解和短语分析。支持断点续传、离线重建、可控并发翻译和可配置文本分段。

![png](1.png)

## 功能特性

- 支持 `epub`、`md/markdown`、`txt` 三类输入
- 支持单文件处理，也支持递归扫描整个目录
- 输出阅读友好的 HTML，每段带 3 个折叠区块
- 调用 Claude 返回结构化 JSON：译文 / 词汇 / chunks
- 支持 `Ctrl+C` 中断后继续跑，已完成段落不会重复请求
- 支持 `--rebuild` 离线重建 HTML，不调用 API
- 支持 `--jobs` 控制并发请求数，支持 `--request-delay-ms` 节流
- TXT / Markdown 分段规则可通过 CLI 参数调节
- HTML 和 state 都走原子写入，崩溃时更安全

## 安装

### 前置条件

```bash
# 安装 Rust（若未安装）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 设置 Anthropic API Key
export ANTHROPIC_AUTH_TOKEN="sk-ant-..."

# 可选：自定义兼容网关
export ANTHROPIC_BASE_URL="https://api.anthropic.com"
```

### 编译

```bash
cd epub-reader
cargo build --release
```

## 快速开始

### 1. 翻译单个 EPUB

```bash
cargo run --release -- ./books/vol1.epub
```

### 2. 翻译整个目录

```bash
cargo run --release -- ./books ./output
```

### 3. 处理 Markdown

```bash
cargo run --release -- ./notes/chapter01.md
```

### 4. 处理 TXT

```bash
cargo run --release -- ./draft.txt
```

### 5. TXT 按每行强制分段

适合诗歌、逐行台词、OCR 后的短句文本：

```bash
cargo run --release -- --txt-hard-linebreaks ./draft.txt
```

### 6. 控制并发和节流

```bash
cargo run --release -- --jobs 3 --request-delay-ms 250 ./books
```

### 7. 离线重建 HTML

不调 API，只根据已有 `*_state.json` 重新生成 HTML：

```bash
cargo run --release -- --rebuild ./books ./output
```

> `--rebuild` 需要使用和之前相同的输入源与输出目录，才能找到对应的 state 文件。

## 命令行用法

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

## 支持的输入格式

### EPUB

- 读取 spine 顺序内容
- 优先提取 HTML 中的 `p`、`blockquote`、`li`
- 如果正文结构比较怪，会尝试用 `div` 做回退提取
- 会过滤部分目录页、页码页、导航项

### Markdown

- 支持读取 YAML frontmatter 中的 `title`
- 如果没有 frontmatter 标题，首个合适的 `# H1` 会作为书名
- `H1-H3` 会优先视为章节标题
- 普通段落和列表项会被当作可翻译文本块

### TXT

- 空行、场景分隔符会触发分段
- 会尝试识别诸如 `Chapter 1`、`第十二章`、`PROLOGUE` 之类标题
- 默认会在句末和缩进位置切段
- 可通过 `--txt-hard-linebreaks` 和 `--txt-no-sentence-split` 调整规则

## 常用场景

### 网络小说 / 轻小说 EPUB

```bash
cargo run --release -- --jobs 3 ./novels
```

### Obsidian / Typora 的 Markdown 笔记

```bash
cargo run --release -- ./notes/book-summary.md
```

### OCR 导出的纯文本

```bash
cargo run --release -- --txt-hard-linebreaks --min-paragraph-chars 1 ./ocr.txt
```

### 已经跑了一半，继续执行

```bash
cargo run --release -- ./books ./output
```

重新执行相同命令即可。程序会自动读取 `*_state.json`，只翻译未完成段落。

## 输出文件

默认输出目录是 `./output`。

```text
output/
├── book-slug.html
├── book-slug_state.json
├── another-book.html
└── another-book_state.json
```

- `*.html`
  最终阅读文件
- `*_state.json`
  断点续传状态文件，保存每个段落的 AI 响应

> 不要随意删除 `*_state.json`，除非你确定要从头重跑。

## 如何继续 / 重跑

### 继续跑

直接重复上次命令：

```bash
cargo run --release -- ./books ./output
```

### 重新生成 HTML，但不重调 API

```bash
cargo run --release -- --rebuild ./books ./output
```

### 完全重来

删除对应的：

- `output/<slug>.html`
- `output/<slug>_state.json`

然后再重新运行。

## 工作原理

核心思路不是“靠位置对齐”，而是“靠段落 ID 对齐”。

```text
输入文件
  └─→ parse_*()
        └─→ Book / Chapter / Paragraph(id, text)
                      │
                      ├─→ html_gen: 生成段落骨架
                      ├─→ pending: 需要请求 LLM 的段落
                      └─→ state.json: para_id -> LlmResponse
```

当前流程：

1. 解析输入文件，得到统一的 `Book` 结构
2. 生成 HTML 骨架，每段预留译文 / 词汇 / chunks 占位区
3. 并发请求 Claude，返回结构化 JSON
4. 收到结果后按 `para_id` patch HTML
5. 先原子写入 HTML，再写入 `*_state.json`

这样做的结果是：

- 即使并发请求完成顺序不同，也不会错位
- 中途崩溃时，最坏情况通常只是某段需要重翻
- `--rebuild` 可以完全跳过 API，只根据 state 恢复 HTML

## 项目结构

```text
src/
├── main.rs            # CLI 参数、主流程、并发翻译调度
├── parser.rs          # 输入格式分发
├── parse_utils.rs     # 通用分段规则、标题识别、BookBuilder
├── epub_parser.rs     # EPUB 解析
├── markdown_parser.rs # Markdown 解析
├── text_parser.rs     # TXT 解析
├── html_gen.rs        # HTML 生成与段落 patch
├── llm_client.rs      # Anthropic Messages API 客户端
├── state.rs           # state.json 读写
├── fs_utils.rs        # 原子写文件
├── ui.rs              # 终端输出样式
└── types.rs           # Book / Paragraph / LlmResponse 等结构
```

## 注意事项

- `ANTHROPIC_AUTH_TOKEN` 只在正常翻译模式下需要；`--rebuild` 不需要
- 如果你修改了原始输入文件，段落 ID 可能变化，旧 state 可能无法完全复用
- `--jobs` 不是越大越好，通常 `2~4` 比较稳
- 对排版特别碎的 TXT，建议试试：
  - `--txt-hard-linebreaks`
  - `--min-paragraph-chars 1`
  - `--txt-no-sentence-split`

## 开发与验证

```bash
cargo fmt
cargo check
cargo test
```

如果想看当前 CLI 帮助：

```bash
cargo run -- --help
```
