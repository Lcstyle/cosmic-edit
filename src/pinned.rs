// SPDX-License-Identifier: GPL-3.0-only

//! Pinned tabs functionality for saving notes to a dedicated directory.
//!
//! This module provides:
//! - Pin/unpin operations for tabs
//! - Scanning the pinned notes directory at startup
//! - File management for pinned markdown notes

use std::{
    fmt,
    fs,
    io,
    path::{Path, PathBuf},
};

/// Errors that can occur during pinned tab operations.
#[derive(Debug)]
pub enum PinError {
    /// The pinned notes directory could not be created.
    DirectoryCreationFailed(io::Error),
    /// A file with the same name already exists.
    FileAlreadyExists(PathBuf),
    /// Failed to write the pinned file.
    WriteFailed(io::Error),
    /// Failed to read the pinned file.
    ReadFailed(io::Error),
    /// The file name is invalid (empty or contains invalid characters).
    InvalidFileName(String),
}

impl fmt::Display for PinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DirectoryCreationFailed(e) => {
                write!(f, "failed to create pinned notes directory: {}", e)
            }
            Self::FileAlreadyExists(path) => {
                write!(f, "file already exists: {}", path.display())
            }
            Self::WriteFailed(e) => write!(f, "failed to write pinned file: {}", e),
            Self::ReadFailed(e) => write!(f, "failed to read pinned file: {}", e),
            Self::InvalidFileName(name) => write!(f, "invalid file name: {}", name),
        }
    }
}

impl std::error::Error for PinError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::DirectoryCreationFailed(e)
            | Self::WriteFailed(e)
            | Self::ReadFailed(e) => Some(e),
            _ => None,
        }
    }
}

/// Ensure the pinned notes directory exists.
pub fn ensure_pinned_dir(pinned_dir: &Path) -> Result<(), PinError> {
    if !pinned_dir.exists() {
        fs::create_dir_all(pinned_dir).map_err(PinError::DirectoryCreationFailed)?;
        log::info!("Created pinned notes directory: {:?}", pinned_dir);
    }
    Ok(())
}

/// Sanitize a file name by removing or replacing invalid characters.
fn sanitize_filename(name: &str) -> Result<String, PinError> {
    let name = name.trim();

    if name.is_empty() {
        return Err(PinError::InvalidFileName("name cannot be empty".to_string()));
    }

    // Remove or replace invalid characters for file names
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect();

    // Ensure it doesn't start with a dot (hidden file)
    let sanitized = if sanitized.starts_with('.') {
        format!("_{}", sanitized)
    } else {
        sanitized
    };

    Ok(sanitized)
}

/// Build the full path for a pinned note file.
fn build_pinned_path(pinned_dir: &Path, name: &str) -> Result<PathBuf, PinError> {
    let sanitized = sanitize_filename(name)?;

    // Add .md extension if not present
    let filename = if sanitized.ends_with(".md") || sanitized.ends_with(".markdown") {
        sanitized
    } else {
        format!("{}.md", sanitized)
    };

    Ok(pinned_dir.join(filename))
}

/// Pin a tab by saving its content to the pinned notes directory.
///
/// # Arguments
/// * `pinned_dir` - The directory where pinned notes are stored
/// * `name` - The name for the pinned note (will be sanitized and .md added)
/// * `content` - The content to save
/// * `overwrite` - If true, overwrite existing file; if false, return error
///
/// # Returns
/// The path to the created/updated pinned file.
pub fn pin_tab(
    pinned_dir: &Path,
    name: &str,
    content: &str,
    overwrite: bool,
) -> Result<PathBuf, PinError> {
    ensure_pinned_dir(pinned_dir)?;

    let path = build_pinned_path(pinned_dir, name)?;

    if path.exists() && !overwrite {
        return Err(PinError::FileAlreadyExists(path));
    }

    fs::write(&path, content).map_err(PinError::WriteFailed)?;
    log::info!("Pinned tab saved to: {:?}", path);

    Ok(path)
}

/// Check if a pinned note with the given name already exists.
pub fn pinned_note_exists(pinned_dir: &Path, name: &str) -> bool {
    build_pinned_path(pinned_dir, name)
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// Information about a pinned note file.
#[derive(Clone, Debug)]
pub struct PinnedNote {
    /// Full path to the file.
    pub path: PathBuf,
    /// Display name (file stem without extension).
    pub name: String,
}

/// Scan the pinned notes directory for markdown files.
///
/// Returns a list of paths to markdown files in the pinned notes directory.
pub fn scan_pinned_notes(pinned_dir: &Path) -> Vec<PinnedNote> {
    let mut notes = Vec::new();

    // If directory doesn't exist, return empty list
    if !pinned_dir.exists() {
        return notes;
    }

    let entries = match fs::read_dir(pinned_dir) {
        Ok(e) => e,
        Err(err) => {
            log::warn!("Failed to read pinned notes directory: {}", err);
            return notes;
        }
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();

        // Only include markdown files
        let is_markdown = path
            .extension()
            .map(|ext| ext == "md" || ext == "markdown")
            .unwrap_or(false);

        if !is_markdown || !path.is_file() {
            continue;
        }

        // Extract name from file stem
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Untitled".to_string());

        notes.push(PinnedNote { path, name });
    }

    // Sort by name for consistent ordering
    notes.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    log::info!("Found {} pinned notes", notes.len());
    notes
}

/// Check if a file path is in the pinned notes directory.
pub fn is_in_pinned_dir(pinned_dir: &Path, file_path: &Path) -> bool {
    file_path
        .parent()
        .map(|parent| parent == pinned_dir)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("hello").unwrap(), "hello");
        assert_eq!(sanitize_filename("hello world").unwrap(), "hello world");
        assert_eq!(sanitize_filename("hello/world").unwrap(), "hello_world");
        assert_eq!(sanitize_filename("hello:world").unwrap(), "hello_world");
        assert_eq!(sanitize_filename(".hidden").unwrap(), "_.hidden");
        assert!(sanitize_filename("").is_err());
        assert!(sanitize_filename("   ").is_err());
    }

    #[test]
    fn test_build_pinned_path() {
        let dir = PathBuf::from("/tmp/notes");
        assert_eq!(
            build_pinned_path(&dir, "test").unwrap(),
            PathBuf::from("/tmp/notes/test.md")
        );
        assert_eq!(
            build_pinned_path(&dir, "test.md").unwrap(),
            PathBuf::from("/tmp/notes/test.md")
        );
    }
}
