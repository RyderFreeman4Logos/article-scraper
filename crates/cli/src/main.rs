use anyhow::Result;
use clap::{Parser, Subcommand};
use config::ConfigWatcher;
use futures::stream::{self, StreamExt};
use std::io::{self, BufRead};
use std::path::PathBuf;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "article-scraper")]
#[command(about = "Article Scraper CLI tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Summarize, rename, and auto-commit files
    Summary {
        #[arg(long, help = "Rename the files using LLM generated filename")]
        rename: bool,

        #[arg(long, help = "Automatically commit the renamed files and summary")]
        auto_commit: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("article_scraper=info".parse()?),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Summary {
            rename,
            auto_commit,
        } => {
            if !rename || !auto_commit {
                anyhow::bail!("Currently only --rename and --auto-commit are supported together.");
            }

            let watcher = ConfigWatcher::new()?;
            let config_rx = watcher.rx;

            // Read lines from stdin
            let stdin = io::stdin();
            let mut paths = Vec::new();
            for line in stdin.lock().lines() {
                let line = line?;
                let path = line.trim();
                if !path.is_empty() {
                    paths.push(PathBuf::from(path));
                }
            }

            if paths.is_empty() {
                info!("No files provided in stdin.");
                return Ok(());
            }

            // Current config for parallelism
            let current_config = config_rx.borrow().clone();
            let worker_count = current_config.worker.count;

            info!(
                "Processing {} files with {} workers",
                paths.len(),
                worker_count
            );

            let config_rx_clone = config_rx.clone();

            stream::iter(paths)
                .for_each_concurrent(worker_count, |path| {
                    let config = config_rx_clone.borrow().clone();
                    async move {
                        info!("Processing file: {}", path.display());
                        if let Err(e) = core_lib::summarize_and_rename(path.clone(), config).await {
                            error!("Error processing {}: {}", path.display(), e);
                        }
                    }
                })
                .await;

            info!("Done processing files.");
        }
    }

    Ok(())
}
