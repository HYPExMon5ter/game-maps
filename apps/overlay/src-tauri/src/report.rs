//! Problem reports: everything needed to look into a problem, in one file a player can
//! attach to an issue. They read it first and post it themselves, so nothing leaves the
//! computer on its own; the home folder and account name are taken out.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::link::AgentLink;
use crate::{map_window, settings};

/// Lines of the app's log to include (a few sessions' worth).
const APP_LOG_LINES: usize = 1500;
const AGENT_LOG_LINES: usize = 300;
/// How many of the most recent log files to read.
const LOG_FILES: usize = 2;
/// Older builds logged the session id, which is also the room's password, after these.
const SESSION_LINES: [&str; 2] = ["play together: joining ", "Multiplayer session joined: "];

/// `text` with `word` replaced, only as a whole word: a short account name must not
/// rewrite ordinary words in the log.
fn replace_word(text: &str, word: &str, with: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut before = None;
    while let Some(at) = rest.find(word) {
        let (head, tail) = rest.split_at(at);
        out.push_str(head);
        let previous = head.chars().next_back().or(before);
        let after = tail[word.len()..].chars().next();
        let whole = !previous.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric);
        out.push_str(if whole { with } else { word });
        before = word.chars().next_back();
        rest = &tail[word.len()..];
    }
    out.push_str(rest);
    out
}

/// `text` without the player's home folder, account name or session ids.
fn scrub(text: &str, home: &str, user: &str) -> String {
    let mut clean = text.to_owned();
    if !home.is_empty() {
        // As written, with forward slashes, and escaped as in debug output.
        for form in [home.to_owned(), home.replace('\\', "/"), home.replace('\\', r"\\")] {
            clean = clean.replace(&form, "%USERPROFILE%");
        }
    }
    if !user.is_empty() {
        clean = replace_word(&clean, user, "<user>");
    }
    clean
        .lines()
        .map(|line| match SESSION_LINES.iter().find_map(|marker| line.find(marker).map(|at| (at, *marker))) {
            Some((at, marker)) => format!("{}{marker}<session>", &line[..at]),
            None => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The last `count` lines of `text`.
fn tail(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(count)..].join("\n")
}

/// The newest log files in `dir`, oldest of them first.
fn newest_logs(dir: &Path) -> Vec<PathBuf> {
    let mut logs: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    logs.sort();
    logs.into_iter().rev().take(LOG_FILES).rev().map(|(_, path)| path).collect()
}

fn read_lossy(path: &Path) -> String {
    std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
}

pub fn windows_version() -> String {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("cmd")
        .args(["/c", "ver"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|e| format!("unknown ({e})"))
}

/// The report's text. `ui` is what the window asking for it knows.
pub fn build(app: &AppHandle, ui: &str) -> String {
    let link = app.state::<Arc<AgentLink>>();
    let json = |value: serde_json::Result<String>| value.unwrap_or_else(|e| format!("unavailable: {e}"));
    let app_log = app.path().app_log_dir().map(|dir| newest_logs(&dir).iter().map(|p| read_lossy(p)).collect::<Vec<_>>().join("\n")).unwrap_or_default();
    let agent_log = read_lossy(&std::env::temp_dir().join("game-maps-agent.log"));
    let report = format!(
        "Game Maps problem report\n\
         Read through it before posting: it has player names from your sessions.\n\n\
         App: {version}\nWindows: {windows}\nWebView2: {webview}\nLive data flowing: {live}\n\n\
         --- Game connection ---\n{status}\n\n\
         --- Monitors ---\n{monitors}\n\n\
         --- Settings ---\n{settings}\n\n\
         --- Window ---\n{ui}\n\n\
         --- App log ---\n{app_log}\n\n\
         --- Agent log ---\n{agent_log}\n",
        version = env!("CARGO_PKG_VERSION"),
        windows = windows_version(),
        webview = tauri::webview_version().unwrap_or_else(|e| format!("unknown ({e})")),
        live = link.is_live(),
        status = json(serde_json::to_string_pretty(&link.status())),
        monitors = json(serde_json::to_string_pretty(&map_window::monitors(app))),
        settings = json(serde_json::to_string_pretty(&settings::get(app))),
        app_log = tail(&app_log, APP_LOG_LINES),
        agent_log = tail(&agent_log, AGENT_LOG_LINES),
    );
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    scrub(&report, &home, &std::env::var("USERNAME").unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_removes_the_home_folder_and_account_name() {
        let text = r"loaded C:\Users\sam\game and C:/Users/sam/maps as sam";
        assert_eq!(scrub(text, r"C:\Users\sam", "sam"), "loaded %USERPROFILE%\\game and %USERPROFILE%/maps as <user>");
    }

    #[test]
    fn scrub_removes_session_ids_from_older_logs() {
        let text = "[INFO] play together: joining sons-of-the-forest/lobby:123\n[INFO] next";
        assert_eq!(scrub(text, "", ""), "[INFO] play together: joining <session>\n[INFO] next");
        let text = r#"[agent] Multiplayer session joined: 1234 as Some("Sam")"#;
        assert_eq!(scrub(text, "", ""), "[agent] Multiplayer session joined: <session>");
    }

    #[test]
    fn scrub_finds_paths_escaped_in_debug_output() {
        assert_eq!(scrub(r#"path "C:\\Users\\sam\\x""#, r"C:\Users\sam", ""), r#"path "%USERPROFILE%\\x""#);
    }

    #[test]
    fn a_short_account_name_leaves_other_words_alone() {
        assert_eq!(scrub("final al alpha (al)", "", "al"), "final <user> alpha (<user>)");
    }

    #[test]
    fn tail_keeps_the_last_lines() {
        assert_eq!(tail("a\nb\nc", 2), "b\nc");
        assert_eq!(tail("a", 5), "a");
    }

    #[test]
    fn newest_logs_skips_older_files_and_other_kinds() {
        let dir = std::env::temp_dir().join(format!("gm-report-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["old.log", "mid.log", "new.log", "notes.txt"] {
            std::fs::write(dir.join(name), name).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let names: Vec<_> = newest_logs(&dir).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["mid.log", "new.log"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
