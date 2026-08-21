//! Kantega LLM proxy client.
//!
//! All Anthropic traffic from Kantega projects goes through the Kantega LLM
//! proxy — never `api.anthropic.com` directly. The wire format is the standard
//! Anthropic Messages API; only the host and the `x-api-key` auth header differ.
//!
//! Used by the Meeting feature to summarise a transcript. The caller is
//! responsible for the mandatory no-key fallback: if no key is configured the
//! transcript itself is the useful artifact and this client is not called.

use log::{error, info};
use serde::Deserialize;
use serde_json::json;

/// `SecretMap` key under which the meeting LLM proxy API key is stored.
pub const MEETING_LLM_KEY_ID: &str = "kantega_llmproxy";

/// Kantega LLM proxy Messages endpoint.
const KANTEGA_LLMPROXY_URL: &str = "https://llmproxy.kantega.no/v1/messages";

/// Anthropic Messages API version pin required by the proxy.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Upper bound on summary length. Meetings can be long, but the summary is a
/// structured recap, so a few thousand tokens is plenty.
const MAX_TOKENS: u32 = 4000;

#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(default)]
    text: String,
    #[serde(rename = "type", default)]
    block_type: String,
}

/// Summarise `transcript` using the Kantega LLM proxy.
///
/// `system_prompt` is the editable summary instruction; `transcript` is sent as
/// the user message. Returns the assistant's text, or an error string suitable
/// for surfacing to the UI. Never logs the API key or the transcript body.
pub async fn summarize(
    api_key: &str,
    model: &str,
    system_prompt: &str,
    transcript: &str,
) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Err("No Kantega LLM proxy API key configured".to_string());
    }
    if transcript.trim().is_empty() {
        return Err("Transcript is empty".to_string());
    }

    let body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "system": system_prompt,
        "messages": [{ "role": "user", "content": transcript }],
    });

    let client = reqwest::Client::new();
    info!(
        "Requesting meeting summary from Kantega LLM proxy (model: {})",
        model
    );

    let response = client
        .post(KANTEGA_LLMPROXY_URL)
        .header("content-type", "application/json")
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            error!("Kantega LLM proxy request failed: {}", e);
            format!("Could not reach the LLM proxy: {}", e)
        })?;

    let status = response.status();
    if !status.is_success() {
        let detail = response.text().await.unwrap_or_default();
        error!("Kantega LLM proxy returned {}: {}", status, detail);
        // 401/403 almost always means a bad or missing key — give a hint.
        let hint = if status.as_u16() == 401 || status.as_u16() == 403 {
            " (check your API key)"
        } else {
            ""
        };
        return Err(format!("LLM proxy error {}{}", status.as_u16(), hint));
    }

    let parsed: MessagesResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse LLM proxy response: {}", e))?;

    let text = parsed
        .content
        .into_iter()
        .filter(|b| b.block_type == "text" || b.block_type.is_empty())
        .map(|b| b.text)
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string();

    if text.is_empty() {
        return Err("LLM proxy returned an empty summary".to_string());
    }

    info!("Meeting summary received ({} chars)", text.len());
    Ok(text)
}
