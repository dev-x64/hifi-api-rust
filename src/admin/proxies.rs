use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::proxy_manager::ProxyManager;
use crate::AppState;

#[derive(Deserialize)]
pub struct UpdateProxiesRequest {
    enabled: bool,
    proxies: Vec<String>,
}

pub async fn proxy_status(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!({
        "proxies": state.proxy_manager.status().await,
        "persistent": state.db.is_some(),
    })))
}

pub async fn update_proxies(
    State(state): State<AppState>,
    Json(body): Json<UpdateProxiesRequest>,
) -> Result<Json<Value>, AppError> {
    let proxies = ProxyManager::normalize_proxies(body.proxies, body.enabled)?;
    if let Some(db) = &state.db {
        let mut tx = db.begin().await?;
        sqlx::query("INSERT INTO settings (key, value) VALUES ('proxy_enabled', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
            .bind(if body.enabled { "true" } else { "false" })
            .execute(&mut *tx).await?;
        sqlx::query("INSERT INTO settings (key, value) VALUES ('proxy_urls', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
            .bind(serde_json::to_string(&proxies).map_err(|e| AppError::Internal(e.to_string()))?)
            .execute(&mut *tx).await?;
        tx.commit().await?;
    }
    state.proxy_manager.configure(body.enabled, proxies).await?;
    if body.enabled { state.proxy_manager.spawn_initial_resolve(); }
    Ok(Json(json!({
        "message": if body.enabled { "Proxy mode enabled" } else { "Proxy mode disabled" },
        "proxies": state.proxy_manager.status().await,
        "persistent": state.db.is_some(),
    })))
}
