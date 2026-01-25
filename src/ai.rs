// SPDX-License-Identifier: GPL-3.0-only

//! AI-powered features using the Anthropic API.
//!
//! This module provides:
//! - Filename suggestions for pinned notes based on content analysis

use misanthropy::{Anthropic, Content, MessagesRequest};
use serde::Deserialize;
use std::path::Path;
use std::sync::OnceLock;

/// AI configuration loaded from ai_config.toml
#[derive(Debug, Deserialize)]
pub struct AiConfig {
    pub filename_suggestion: FilenameSuggestionConfig,
}

#[derive(Debug, Deserialize)]
pub struct FilenameSuggestionConfig {
    /// Model to use (default: claude-3-5-haiku-latest for fast responses)
    #[serde(default = "default_model")]
    pub model: String,
    /// Maximum tokens for the response
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Maximum content length (in bytes) to send to the AI
    #[serde(default = "default_max_content_length")]
    pub max_content_length: usize,
    /// Prompt template ({content} is replaced with document content)
    pub prompt: String,
}

fn default_model() -> String {
    "claude-3-5-haiku-latest".to_string()
}

fn default_max_tokens() -> u32 {
    50
}

fn default_max_content_length() -> usize {
    50_000 // ~50KB of text for analysis
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            filename_suggestion: FilenameSuggestionConfig {
                model: default_model(),
                max_tokens: default_max_tokens(),
                max_content_length: default_max_content_length(),
                prompt: r#"Based on the following document content, suggest a short, descriptive filename (without extension).
The filename should:
- Be 2-5 words, lowercase, separated by hyphens
- Capture the main topic or purpose of the document
- Be suitable for a markdown note file

Respond with ONLY the suggested filename, nothing else. No quotes, no extension, no explanation.

Document content:
---
{content}
---"#.to_string(),
            },
        }
    }
}

/// Global AI config, loaded once at startup
static AI_CONFIG: OnceLock<AiConfig> = OnceLock::new();

/// Load AI configuration from TOML file.
/// Falls back to defaults if file doesn't exist or is invalid.
pub fn load_config() {
    let config = load_config_from_file().unwrap_or_else(|e| {
        log::warn!("ai: failed to load ai_config.toml, using defaults: {}", e);
        AiConfig::default()
    });
    let _ = AI_CONFIG.set(config);
}

fn load_config_from_file() -> Result<AiConfig, Box<dyn std::error::Error>> {
    // Try to find config in several locations:
    // 1. Next to the executable (for development)
    // 2. In XDG config dir
    // 3. Embedded in the source directory (for development builds)

    let config_locations = [
        // Development: src/ai_config.toml relative to manifest dir
        option_env!("CARGO_MANIFEST_DIR").map(|d| Path::new(d).join("src/ai_config.toml")),
        // XDG config
        dirs::config_dir().map(|d| d.join("com.system76.CosmicEdit/ai_config.toml")),
        // Current directory fallback
        Some(Path::new("ai_config.toml").to_path_buf()),
    ];

    for location in config_locations.into_iter().flatten() {
        if location.exists() {
            let content = std::fs::read_to_string(&location)?;
            let config: AiConfig = toml::from_str(&content)?;
            log::info!("ai: loaded config from {:?}", location);
            return Ok(config);
        }
    }

    Err("ai_config.toml not found in any location".into())
}

/// Get the AI configuration (loads defaults if not yet initialized)
fn get_config() -> &'static AiConfig {
    AI_CONFIG.get_or_init(|| {
        load_config_from_file().unwrap_or_else(|e| {
            log::warn!("ai: config not loaded, using defaults: {}", e);
            AiConfig::default()
        })
    })
}

/// Suggest a filename based on document content using Claude.
///
/// Returns `None` if:
/// - No API key is configured
/// - Content is empty
/// - API call fails
///
/// The suggestion is a short, descriptive filename without extension.
pub async fn suggest_filename(api_key: &str, content: &str) -> Option<String> {
    if content.trim().is_empty() {
        return None;
    }

    let config = get_config();
    let max_len = config.filename_suggestion.max_content_length;

    // Truncate content if too long (keep first portion for context)
    let content_for_analysis = if content.len() > max_len {
        &content[..max_len]
    } else {
        content
    };

    let client = Anthropic::new(api_key);

    // Build prompt from template
    let prompt = config
        .filename_suggestion
        .prompt
        .replace("{content}", content_for_analysis);

    let mut request = MessagesRequest::default();
    request.model = config.filename_suggestion.model.clone();
    request.max_tokens = config.filename_suggestion.max_tokens;
    request.add_user(Content::text(&prompt));

    match client.messages(&request).await {
        Ok(response) => {
            // Extract text from response
            let suggestion: Option<String> = response
                .content
                .iter()
                .filter_map(|c| {
                    if let Content::Text(text) = c {
                        Some(text.text.trim().to_string())
                    } else {
                        None
                    }
                })
                .next();

            if let Some(name) = suggestion {
                // Clean up the suggestion
                let cleaned = sanitize_suggested_filename(&name);
                if !cleaned.is_empty() {
                    log::info!("ai: suggested filename: {}", cleaned);
                    return Some(cleaned);
                }
            }
            None
        }
        Err(e) => {
            log::warn!("ai: API request failed: {}", e);
            None
        }
    }
}

/// Clean and sanitize an AI-suggested filename.
fn sanitize_suggested_filename(name: &str) -> String {
    // Remove any quotes, extensions, or extra whitespace
    let name = name
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`');

    // Remove common extensions if present
    let name = name
        .strip_suffix(".md")
        .or_else(|| name.strip_suffix(".markdown"))
        .or_else(|| name.strip_suffix(".txt"))
        .unwrap_or(name);

    // Convert to lowercase and replace spaces/underscores with hyphens
    let cleaned: String = name
        .to_lowercase()
        .chars()
        .map(|c| match c {
            ' ' | '_' => '-',
            c if c.is_alphanumeric() || c == '-' => c,
            _ => '-',
        })
        .collect();

    // Remove consecutive hyphens and trim
    let mut result = String::new();
    let mut last_was_hyphen = false;
    for c in cleaned.chars() {
        if c == '-' {
            if !last_was_hyphen && !result.is_empty() {
                result.push(c);
                last_was_hyphen = true;
            }
        } else {
            result.push(c);
            last_was_hyphen = false;
        }
    }

    result.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_suggested_filename("My Document"), "my-document");
        assert_eq!(sanitize_suggested_filename("\"quoted-name\""), "quoted-name");
        assert_eq!(sanitize_suggested_filename("name.md"), "name");
        assert_eq!(sanitize_suggested_filename("Hello World.markdown"), "hello-world");
        assert_eq!(sanitize_suggested_filename("test__file"), "test-file");
        assert_eq!(sanitize_suggested_filename("--leading--"), "leading");
    }
}
