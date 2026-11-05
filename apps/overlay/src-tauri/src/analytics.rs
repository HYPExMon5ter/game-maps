//! Anonymous usage counts, sent to PostHog; the README's Privacy section lists them all.
//! The launcher asks the player first and nothing goes out until they agree. Settings →
//! General changes the answer.
//!
//! The free plan counts events, not what's in them, so details ride on a few events: what
//! happens during play adds up in a [`PlaySession`], sent once when the game closes, and
//! a failure counts once however often it repeats. Only fixed categories go out, because
//! error messages can hold file paths.
//!
//! The install id is a random id in its own file, not in the settings: those go into
//! problem reports, which players post publicly. Development builds send nothing.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

use crate::mappacks::MapPacks;
use crate::settings::{self, HookMode};

const ENDPOINT: &str = "https://us.i.posthog.com/i/v0/e/";
/// Write-only project token, made to ship inside apps.
const PROJECT_TOKEN: &str = "phc_wwwvcvyf3HxA9PfpJjXaEPsdiZFYLhC4VsYndQxRmaYW";
const STORE_FILE: &str = "analytics.json";
const STORE_KEY: &str = "install_id";

/// Why a game couldn't be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookFailure {
    /// The agent couldn't be read or staged (often removed by antivirus).
    AgentFiles,
    /// The game process couldn't be opened (usually it runs as administrator).
    ProcessAccess,
    Injection,
    Pipe,
    /// The agent was injected but never answered.
    AgentTimeout,
    ProtocolMismatch,
    /// The agent is running but doesn't recognize the game: likely a game update.
    GameNotRecognized,
    /// The agent connected but no data ever came: likely a game update.
    NoGameData,
}

impl HookFailure {
    fn reason(self) -> &'static str {
        match self {
            Self::AgentFiles => "agent_files",
            Self::ProcessAccess => "process_access",
            Self::Injection => "injection",
            Self::Pipe => "pipe",
            Self::AgentTimeout => "agent_timeout",
            Self::ProtocolMismatch => "protocol_mismatch",
            Self::GameNotRecognized => "game_not_recognized",
            Self::NoGameData => "no_game_data",
        }
    }
}

/// Something that went wrong outside a game connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Problem {
    UpdateInstall,
    MapDownload,
    WebviewCrash,
}

impl Problem {
    fn kind(self) -> &'static str {
        match self {
            Self::UpdateInstall => "update_install",
            Self::MapDownload => "map_download",
            Self::WebviewCrash => "webview_crash",
        }
    }
}

/// A feature used during play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    MapOpen,
    Teleport,
    Tracked,
    Multiplayer,
}

/// One run of a game, from connecting to it closing.
struct PlaySession {
    game_id: String,
    started: Instant,
    map_opens: u32,
    teleports: u32,
    tracked: bool,
    multiplayer: bool,
}

impl PlaySession {
    fn new(game_id: &str) -> Self {
        Self { game_id: game_id.to_owned(), started: Instant::now(), map_opens: 0, teleports: 0, tracked: false, multiplayer: false }
    }

    fn count(&mut self, feature: Feature) {
        match feature {
            Feature::MapOpen => self.map_opens += 1,
            Feature::Teleport => self.teleports += 1,
            Feature::Tracked => self.tracked = true,
            Feature::Multiplayer => self.multiplayer = true,
        }
    }

    fn properties(&self) -> Value {
        json!({
            "game": self.game_id,
            "minutes": self.started.elapsed().as_secs() / 60,
            "map_opens": self.map_opens,
            "teleports": self.teleports,
            "tracked": self.tracked,
            "multiplayer": self.multiplayer,
        })
    }
}

struct InstallId {
    id: String,
    /// Made during this run: this computer hasn't been counted before.
    new: bool,
}

pub struct Analytics {
    client: reqwest::Client,
    install_id: OnceLock<InstallId>,
    /// `app_started` went out: it can come at launch or when the setting is turned on.
    started: AtomicBool,
    session: Mutex<Option<PlaySession>>,
    /// Problems already counted in this run.
    problems: Mutex<HashSet<Problem>>,
}

impl Analytics {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("GameMaps/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(15))
            .build()
            .expect("http client");
        Self { client, install_id: OnceLock::new(), started: AtomicBool::new(false), session: Mutex::new(None), problems: Mutex::new(HashSet::new()) }
    }
}

/// The id saved on this computer, made the first time it's needed.
fn load_install_id(app: &AppHandle) -> InstallId {
    let store = app.store(STORE_FILE).map_err(|e| log::warn!("cannot open {STORE_FILE}: {e}")).ok();
    if let Some(id) = store.as_ref().and_then(|s| s.get(STORE_KEY)).and_then(|v| v.as_str().map(str::to_owned)) {
        return InstallId { id, new: false };
    }
    let id = uuid::Uuid::new_v4().to_string();
    if let Some(store) = store {
        store.set(STORE_KEY, id.clone());
        if let Err(e) = store.save() {
            log::warn!("cannot save {STORE_FILE}: {e}");
        }
    }
    InstallId { id, new: true }
}

/// The request for `event`. No person profile is made, and no location is looked up.
fn event_body(install_id: &str, event: &str, mut properties: Value) -> Value {
    properties["$process_person_profile"] = json!(false);
    properties["$geoip_disable"] = json!(true);
    properties["app_version"] = json!(env!("CARGO_PKG_VERSION"));
    json!({ "api_key": PROJECT_TOKEN, "event": event, "distinct_id": install_id, "properties": properties })
}

async fn post(client: reqwest::Client, body: Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    let request = client.post(ENDPOINT).header(reqwest::header::CONTENT_TYPE, "application/json").body(bytes);
    request.send().await.and_then(reqwest::Response::error_for_status).map_err(|e| e.to_string())?;
    Ok(())
}

fn enabled(app: &AppHandle) -> bool {
    !cfg!(debug_assertions) && settings::get(app).analytics == Some(true)
}

fn capture(app: &AppHandle, event: &'static str, properties: Value) {
    if !enabled(app) {
        return;
    }
    let analytics = app.state::<Analytics>();
    let install_id = analytics.install_id.get_or_init(|| load_install_id(app));
    let (client, body) = (analytics.client.clone(), event_body(&install_id.id, event, properties));
    tauri::async_runtime::spawn(async move {
        if let Err(e) = post(client, body).await {
            log::info!("analytics: {event} not sent: {e}");
        }
    });
}

/// The build number out of `ver`'s output, which ends `[Version 10.0.26100.2605]` in
/// any language but with the word translated.
fn windows_build(ver: &str) -> Option<&str> {
    ver.trim().trim_end_matches(']').rsplit(' ').next()?.split('.').nth(2)
}

/// `en` from `en-US`: enough to decide on translations, without the country.
fn language(locale: &str) -> &str {
    locale.split(['-', '_']).next().unwrap_or(locale)
}

fn app_started_properties(app: &AppHandle) -> Value {
    let settings = settings::get(app);
    let new = app.state::<Analytics>().install_id.get_or_init(|| load_install_id(app)).new;
    let ver = crate::report::windows_version();
    json!({
        "first_run": new,
        "at_login": std::env::args().any(|a| a == crate::BACKGROUND_ARG),
        "windows_build": windows_build(&ver),
        "language": sys_locale::get_locale().as_deref().map(language),
        "auto_update": settings.auto_update,
        "second_monitor": settings.map_window.is_some(),
    })
}

/// Once per launch, as soon as it's allowed: call at startup and when the player answers.
pub fn app_started(app: &AppHandle) {
    if !enabled(app) || app.state::<Analytics>().started.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    // Asking Windows for its version starts a process.
    tauri::async_runtime::spawn_blocking(move || capture(&app, "app_started", app_started_properties(&app)));
}

/// A game connected: its session runs until [`game_closed`].
pub fn game_connected(app: &AppHandle, game_id: &str) {
    *app.state::<Analytics>().session.lock().unwrap() = Some(PlaySession::new(game_id));
    let map_version = app.state::<MapPacks>().statuses().into_iter().find(|p| p.game_id == game_id).and_then(|p| p.installed);
    let hook_mode = match settings::get(app).hook_mode(game_id) {
        HookMode::Auto => "auto",
        HookMode::Manual => "manual",
    };
    capture(app, "game_connected", json!({ "game": game_id, "map_version": map_version, "hook_mode": hook_mode }));
}

/// Adds to the session being played, if there is one.
pub fn count(app: &AppHandle, feature: Feature) {
    if let Some(session) = app.state::<Analytics>().session.lock().unwrap().as_mut() {
        session.count(feature);
    }
}

/// No game is running: sends the summary of the session that just ended, if any.
pub fn game_closed(app: &AppHandle) {
    let session = app.state::<Analytics>().session.lock().unwrap().take();
    if let Some(session) = session {
        capture(app, "game_session_ended", session.properties());
    }
}

pub fn hook_failed(app: &AppHandle, game_id: &str, failure: HookFailure) {
    capture(app, "hook_failed", json!({ "game": game_id, "reason": failure.reason() }));
}

/// Counts `problem` once per run. Web views can fail while the app is still starting.
pub fn problem(app: &AppHandle, problem: Problem) {
    let first = app.try_state::<Analytics>().is_some_and(|a| a.problems.lock().unwrap().insert(problem));
    if first {
        capture(app, "app_problem", json!({ "kind": problem.kind() }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_carry_only_what_the_readme_says() {
        let body = event_body("id", "hook_failed", json!({ "game": "a", "reason": HookFailure::NoGameData.reason() }));
        assert_eq!(body["distinct_id"], "id");
        assert_eq!(body["event"], "hook_failed");
        assert_eq!(
            body["properties"],
            json!({
                "game": "a",
                "reason": "no_game_data",
                "app_version": env!("CARGO_PKG_VERSION"),
                "$process_person_profile": false,
                "$geoip_disable": true,
            })
        );
    }

    #[test]
    fn a_session_adds_up_what_was_used() {
        let mut session = PlaySession::new("a");
        for feature in [Feature::MapOpen, Feature::MapOpen, Feature::Teleport, Feature::Tracked, Feature::Tracked] {
            session.count(feature);
        }
        assert_eq!(
            session.properties(),
            json!({ "game": "a", "minutes": 0, "map_opens": 2, "teleports": 1, "tracked": true, "multiplayer": false })
        );
    }

    #[test]
    fn reads_the_build_from_ver_output_in_any_language() {
        assert_eq!(windows_build("Microsoft Windows [Version 10.0.26100.2605]\r\n"), Some("26100"));
        assert_eq!(windows_build("Microsoft Windows [Versión 10.0.19045.3803]"), Some("19045"));
        assert_eq!(windows_build("unknown (no cmd)"), None);
    }

    #[test]
    fn language_leaves_out_the_country() {
        assert_eq!(language("en-US"), "en");
        assert_eq!(language("pt_BR"), "pt");
        assert_eq!(language("de"), "de");
    }
}
