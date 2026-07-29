//! Running the `git` binary.
//!
//! Anything touching a remote goes through here rather than through libgit2, so
//! ssh agents, credential helpers, askpass prompts, and signing behave exactly
//! as they do in the user's terminal. Failures carry git's own words: a message
//! the user can search for beats one jotter invented.

use std::path::Path;
use std::process::Command;

use crate::GitError;

/// Runs `git` in `root` and returns its stdout, trimmed.
///
/// # Errors
/// [`GitError::Command`] with git's stderr when git exits non-zero, or
/// [`GitError::Io`] when the binary cannot be run at all.
pub fn git(root: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        // A prompt from a subprocess with no terminal would hang the app forever.
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let said = if stderr.is_empty() { stdout } else { stderr };
    Err(GitError::Command(if said.is_empty() {
        format!("git {} failed", args.join(" "))
    } else {
        said
    }))
}
