//! Claude Magpie uses the Claude ACP adapter with a dedicated API-key profile.

use super::{Kind, Profile, all, find, keys, only_own_login};
use crate::agent::bot::Cmd;
use crate::hub::Hub;
use crate::keeper::client::Grants;
use crate::store::Store;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

pub const BACKEND: &str = "claude-magpie";
pub const NAME: &str = "Claude Magpie";
const PROFILE: &str = "Magpie (Codync)";
const BASE: &str = "ANTHROPIC_BASE_URL";
const TOKEN: &str = "ANTHROPIC_AUTH_TOKEN";
const KEY: &str = "ANTHROPIC_API_KEY";
pub(crate) const MODEL: &str = "ANTHROPIC_MODEL";
const HINT: &str = "In Claude Code's Login profiles, set up one API key profile named Magpie (Codync) with a gateway URL and an Anthropic API key or auth token.";

pub(crate) fn profile(store: &Store, pinned: Option<&str>) -> Result<Profile> {
    let eligible = |p: &Profile| {
        let has = |name| p.vars.iter().any(|var| var == name);
        p.kind == Kind::ApiKey
            && p.account.agent == "claude"
            && p.account.label.as_deref() == Some(PROFILE)
            && has(BASE)
            && (has(TOKEN) || has(KEY))
    };
    if let Some(id) = pinned {
        return find(store, id).ok().filter(eligible).ok_or_else(|| anyhow!(HINT));
    }
    let mut matches = all(store).into_iter().filter(eligible);
    let selected = matches.next().filter(|_| matches.next().is_none()).ok_or_else(|| anyhow!(HINT))?;
    find(store, &selected.account.id)
}

/// Keep Claude Code's marketplace entry and login controls for existing bots and profile setup.
pub fn add_choice(store: &Store, agents: &mut Vec<Value>) {
    let Some(claude) = agents.iter_mut().find(|a| a["id"] == "claude") else { return };
    claude["botSelectable"] = false.into();
    let profile = profile(store, None).ok();
    let configured = profile.is_some();
    let choice = json!({
        "id": BACKEND,
        "name": NAME,
        "registry": claude["registry"],
        "available": configured && claude["available"] == true,
        "installed": claude["installed"],
        "command": "",
        "installHint": if configured { "Install Claude Code on this computer to run Claude Magpie." } else { HINT },
        "signedIn": configured,
        "profileBacked": true,
        "modelFromProfile": profile.as_ref().is_some_and(sets_model),
    });
    agents.push(choice);
}

pub(crate) fn env(store: &Store, pinned: Option<&str>) -> Result<(Vec<(String, String)>, Grants)> {
    let profile = profile(store, pinned)?;
    let (mut env, grants) = keys::env(store, &profile.account.id)?;
    let value = |key| env.iter().find(|(name, _)| name == key).map(|(_, value)| value.as_str());
    let gateway = value(BASE).and_then(|base| url::Url::parse(base).ok()).is_some_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some_and(|host| !host.trim_end_matches('.').eq_ignore_ascii_case("api.anthropic.com"))
            && url.username().is_empty()
            && url.password().is_none()
    });
    if !gateway
        || !([TOKEN, KEY]
            .into_iter()
            .any(|name| value(name).is_some_and(|v| !v.trim().is_empty() && v != crate::keeper::PLACEHOLDER)))
    {
        bail!(HINT);
    }
    // Isolate Claude's settings and native credentials from this gateway login. Strip alternate
    // provider switches and OAuth even when the profile or the Host environment sets them.
    let isolated = profile.account.slot(&crate::service::data_dir().join("claude-magpie"))?;
    std::fs::create_dir_all(&isolated).context("couldn't prepare Claude Magpie's configuration directory")?;
    for (name, value) in [
        ("CLAUDE_CONFIG_DIR", isolated.to_string_lossy().into_owned()),
        ("CLAUDE_CODE_OAUTH_TOKEN", String::new()),
        ("CLAUDE_CODE_USE_BEDROCK", String::new()),
        ("CLAUDE_CODE_USE_VERTEX", String::new()),
        ("CLAUDE_CODE_USE_FOUNDRY", String::new()),
    ] {
        env.retain(|(key, _)| key != name);
        env.push((name.to_owned(), value));
    }
    for name in [
        MODEL,
        "ANTHROPIC_CUSTOM_HEADERS",
        "CLAUDE_MODEL_CONFIG",
        "ANTHROPIC_SMALL_FAST_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    ] {
        if !env.iter().any(|(key, _)| key == name) {
            env.push((name.to_owned(), String::new()));
        }
    }
    only_own_login("claude", &mut env);
    Ok((env, grants))
}

/// Agent processes take a snapshot of the profile at start. Restart the ones that may be using it.
pub(crate) fn keys_changed(hub: &Hub, id: &str) {
    let rows = hub.store.bots().unwrap_or_default();
    let tasks = crate::chat::tasks::working_tasks(&hub.store);
    let configs = rows.into_iter().filter(|row| !row.deleted).map(|row| row.config).chain(tasks);
    for cfg in configs {
        let uses = |backend: &str, profile: Option<&str>| backend == BACKEND && profile == Some(id);
        if uses(&cfg.backend, cfg.login_profile.as_deref())
            || cfg.fallbacks.iter().any(|step| uses(&step.backend, step.login_profile.as_deref()))
        {
            // A bot without an actor will read the new keys when it starts.
            let _ = hub.send_cmd(&cfg.id, Cmd::RefreshTools);
        }
    }
}

pub(crate) fn sets_model(profile: &Profile) -> bool {
    profile.vars.iter().any(|key| key == MODEL)
}

pub(crate) fn overridden_models(profile: &Profile) -> Option<Value> {
    if !sets_model(profile) {
        return None;
    }
    // Model values are vault contents. One default entry tells the picker how the model
    // is chosen without sending the configured value or starting a discovery process.
    Some(json!({"models": [{"id": "default", "name": "Default · Login profile"}],
        "currentModelId": "default", "fastMode": false}))
}
