use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

pub async fn get_settings(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "settings": state.settings.snapshot() }))
}

pub async fn update_settings(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let updates = body.get("settings").or(body.get("rate_limits")).unwrap_or(&body);

    state
        .settings
        .apply(updates)
        .map_err(AppError::BadRequest)?;

    if let Some(db) = &state.db {
        state.settings.save_to_db(db).await;
    }
    // Publish fleet-wide so sibling instances adopt the change.
    state.settings.save_to_redis().await;

    Ok(Json(json!({
        "message": "Settings updated",
        "settings": state.settings.snapshot()
    })))
}
