# Article Scraper

A Rust CLI tool to summarize, rename, and auto-commit article files.

## Installation

```bash
cargo install --path crates/cli
```

## Configuration

The tool reads configuration from `~/.config/article-scraper/config.toml`.

Example `config.toml`:

```toml
[llm]
base_url = "http://localhost:8317/v1"
api_key = "sk-placeholder"
model = "qwen3.5-35b-a3b"
max_tokens = 50000
timeout_ms = 3600000

[worker]
count = 8
```

## Usage

```bash
fd article.md /path/to/articles | article-scraper summary --rename --auto-commit
```
