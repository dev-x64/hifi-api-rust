use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::AppState;
use crate::error::AppError;

pub async fn cache_stats(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    use std::sync::atomic::Ordering;
    Ok(Json(json!({ "cache": {
        "hits": state.cache.hits.load(Ordering::Relaxed),
        "misses": state.cache.misses.load(Ordering::Relaxed),
        "coalesced": state.cache.coalesced.load(Ordering::Relaxed),
        "stale": state.cache.stale.load(Ordering::Relaxed),
        "negative": state.cache.negative_hits.load(Ordering::Relaxed),
    } })))
}

pub async fn cache_clear(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    state.cache.invalidate_all().await;
    Ok(Json(json!({"message": "Response cache cleared"})))
}
