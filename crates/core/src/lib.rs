use anyhow::{Context, Result};
use config::AppConfig;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::info;

#[derive(Deserialize)]
struct LlmResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: String,
}

#[derive(Deserialize)]
struct OutputJson {
    summary: String,
    filename: String,
}

/// Truncate a summary to ~200 characters.
fn trim_summary(text: &str) -> String {
    let text = text.replace('\n', " ");
    let text = text.trim();
    if text.chars().count() > 200 {
        text.chars().take(200).collect::<String>()
    } else {
        text.to_string()
    }
}

/// Extract JSON output from the LLM's response.
fn extract_json_object(text: &str) -> Option<OutputJson> {
    if let Ok(parsed) = serde_json::from_str::<OutputJson>(text) {
        return Some(parsed);
    }

    // Attempt to extract JSON block using regex-like behavior if directly parsing fails
    if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            let json_str = &text[start..=end];
            if let Ok(parsed) = serde_json::from_str::<OutputJson>(json_str) {
                return Some(parsed);
            }
        }
    }
    None
}

/// Call the LLM to summarize and rename the article.
pub async fn summarize_and_rename(path: PathBuf, config: Arc<AppConfig>) -> Result<()> {
    let content = tokio::fs::read_to_string(&path)
        .await
        .context("Failed to read file")?;

    let llm_config = &config.llm;

    let client = Client::builder()
        .timeout(Duration::from_millis(llm_config.timeout_ms))
        .build()?;

    let prompt = format!(
        "你是中文编辑。根据文章内容生成。必须只输出 JSON，不要 Markdown。\n\
        请输出 JSON：\n\
        {{\"summary\":\"200字以内中文摘要\",\"filename\":\"不带扩展名的中文文件名\"}}。\n\
        要求：summary 不超过200个汉字/字符；filename 不超过显示宽度60\n\
        内容：\n{}",
        content
            .chars()
            .take(llm_config.max_tokens as usize)
            .collect::<String>()
    );

    let payload = json!({
        "model": llm_config.model,
        "messages": [
            {
                "role": "system",
                "content": "你是中文编辑。必须只输出 JSON，不要 Markdown。"
            },
            {
                "role": "user",
                "content": prompt
            }
        ],
        "max_tokens": llm_config.max_tokens,
        "temperature": 0.2
    });

    let resp = client
        .post(format!(
            "{}/chat/completions",
            llm_config.base_url.trim_end_matches('/')
        ))
        .header("Authorization", format!("Bearer {}", llm_config.api_key))
        .json(&payload)
        .send()
        .await
        .context("Failed to send LLM request")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("LLM request failed with status: {}, body: {}", status, body);
    }

    let response_data: LlmResponse = resp
        .json()
        .await
        .context("Failed to parse LLM JSON response")?;
    let llm_content = response_data
        .choices
        .first()
        .context("No choices in LLM response")?
        .message
        .content
        .clone();

    let parsed = extract_json_object(&llm_content).context(format!(
        "Failed to extract JSON from LLM response: {}",
        llm_content
    ))?;

    let summary = trim_summary(&parsed.summary);

    let mut filename = parsed.filename;
    filename = filename.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "");
    filename = filename.replace(' ', "_");
    if filename.is_empty() {
        filename = "article".to_string();
    }

    let new_path = path.with_file_name(format!("{}.md", filename));

    tokio::fs::rename(&path, &new_path)
        .await
        .context("Failed to rename file")?;

    info!("Renamed {} to {}", path.display(), new_path.display());

    // Write summary
    let summary_path = new_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("summary.md");
    let summary_content = format!("# Summary\n\n{}\n", summary);
    tokio::fs::write(&summary_path, summary_content)
        .await
        .context("Failed to write summary.md")?;

    // Auto commit
    commit_changes(&new_path, &summary_path, &summary)?;

    Ok(())
}

fn commit_changes(article_path: &Path, summary_path: &Path, summary: &str) -> Result<()> {
    // Stage files
    let status = Command::new("git")
        .arg("add")
        .arg(article_path)
        .arg(summary_path)
        .status()
        .context("Failed to execute git add")?;

    if !status.success() {
        anyhow::bail!("git add failed");
    }

    // Commit
    let commit_msg = summary.replace('\n', " ");
    let status = Command::new("git")
        .arg("commit")
        .arg("-m")
        .arg(&commit_msg)
        .status()
        .context("Failed to execute git commit")?;

    if !status.success() {
        anyhow::bail!("git commit failed");
    }

    info!("Successfully committed changes: {}", commit_msg);
    Ok(())
}
