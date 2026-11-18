//! A new bot's defaults, for a client's `createBot` and a bot's `create_bot` (`chat::manage`) alike.

use crate::hub::Hub;
use crate::market;
use crate::store::BotConfig;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::sync::Arc;

/// The bot or group `b` asks for, with the defaults for what the caller left out.
pub async fn with_defaults(hub: &Arc<Hub>, mut b: Value) -> Result<BotConfig> {
    b["id"] = "".into();
    // A lead is made by the host or picked by the user (`chat::lead`), never on creation.
    b["lead"] = false.into();
    // Connectors are on for a new bot unless the client picked them.
    if b["connectors"].is_null() && b["kind"] != "group" {
        b["connectors"] = default_connectors(hub).await?;
    }
    // Skills too: every installed one is on unless the client picked them, but a bot's skill stays with that bot.
    if b["skills"].is_null() && b["kind"] != "group" {
        b["skills"] = json!(market::Installed::Skills.ids(&hub.store)?);
    }
    // A bot works in its agent's Auto mode and asks about the rest, unless the client picked how it approves.
    if b["permission"].is_null() && b["mode"].is_null() && b["kind"] != "group" {
        b["mode"] = crate::agent::modes::AUTO.into();
    }
    // And the agent, when the client names none.
    if b["backend"].as_str().is_none_or(str::is_empty) && b["kind"] != "group" {
        b["backend"] = "codex".into();
    }
    serde_json::from_value(b).context("invalid bot")
}

/// Every connector a new bot gets. With none saved the credential store stays shut, which is what keeps the
/// lead bot of a new computer from opening it at start-up.
async fn default_connectors(hub: &Arc<Hub>) -> Result<Value> {
    if !market::connectors_saved(&hub.store)? {
        return Ok(json!([]));
    }
    // Without a credential store (headless Linux) no connector can run, so the bot starts with none.
    Ok(match market::vault::unlock(hub.clone()).await {
        Ok(()) => json!(market::connector_ids(&hub.store)?),
        Err(e) => {
            tracing::warn!(error = format!("{e:#}"), "credential store unavailable; new bot starts without connectors");
            json!([])
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn omitted_agent_and_model_use_codex_defaults_and_explicit_choices_are_preserved() {
        let hub = crate::chat::calls::context::tests::hub();
        let defaults = with_defaults(&hub, json!({"name": "Default"})).await.expect("new bot");
        assert_eq!(defaults.backend, "codex");
        assert_eq!(defaults.model, None);
        for backend in ["codex", "claude"] {
            let requested = json!({"name": "Explicit", "backend": backend, "model": "user-choice"});
            let cfg = with_defaults(&hub, requested).await.expect("new bot");
            assert_eq!(cfg.backend, backend);
            assert_eq!(cfg.model.as_deref(), Some("user-choice"));
        }
    }
}
