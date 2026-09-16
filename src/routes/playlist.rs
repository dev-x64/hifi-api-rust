use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct PlaylistParams {
    pub id: String,
    #[serde(default = "default_playlist_limit")]
    #[allow(dead_code)]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_playlist_limit() -> i64 {
    100
}

pub async fn get_playlist(
    State(state): State<AppState>,
    Query(params): Query<PlaylistParams>,
) -> Result<Json<Value>, AppError> {
    let cc = state.config.country_code.clone();
    let offset_str = params.offset.to_string();

    let playlist_url = format!("https://api.tidal.com/v1/playlists/{}", params.id);
    let items_url = format!("https://api.tidal.com/v1/playlists/{}/items", params.id);

    let tc1 = state.tidal_client.clone();
    let tc2 = state.tidal_client.clone();
    let cc1 = cc.clone();
    let cc2 = cc.clone();
    let offset2 = offset_str.clone();
    let playlist_fut = async move {
        tc1.make_catalog_authed_request(
            &playlist_url,
            Some(vec![("countryCode", &cc1)]),
        )
        .await
    };
    let items_fut = async move {
        tc2.make_catalog_authed_request(
            &items_url,
            Some(vec![
                ("countryCode", &cc2),
                ("limit", "100"),
                ("offset", &offset2),
            ]),
        )
        .await
    };

    let (playlist_result, items_result) = tokio::join!(playlist_fut, items_fut);

    let playlist_data = match playlist_result {
        Ok(d) => d,
        Err(e) => return Err(e),
    };

    let items_data = match items_result {
        Ok(d) => d,
        Err(e) => return Err(e),
    };

    let items = items_data
        .get("items")
        .cloned()
        .unwrap_or_else(|| items_data.clone());

    Ok(Json(json!({
        "version": state.config.api_version,
        "playlist": playlist_data,
        "items": items
    })))
}
