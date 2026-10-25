use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct LyricsParams {
    pub id: i64,
}

pub async fn get_lyrics(
    State(state): State<AppState>,
    Query(params): Query<LyricsParams>,
) -> Result<Json<Value>, AppError> {
    let url = format!("https://api.tidal.com/v1/tracks/{}/lyrics", params.id);
    let v1 = state
        .tidal_client
        .make_catalog_authed_request(
            &url,
            Some(vec![
                ("countryCode", &state.config.country_code),
                ("locale", "en_US"),
                ("deviceType", "BROWSER"),
            ]),
        )
        .await;

    // v1 misses lyrics that openapi v2 has; a busy pool is not worth a second request.
    let v1_error = match v1 {
        Ok(data) if !data.is_null() && data.as_object().is_some_and(|o| !o.is_empty()) => {
            return Ok(lyrics_response(&state, data));
        }
        Ok(_) => AppError::NotFound("Lyrics not found".into()),
        Err(e @ (AppError::RateLimited(_)
        | AppError::AccountsBusy(..)
        | AppError::ServiceUnavailableRetry(..))) => return Err(e),
        Err(e) => e,
    };

    let url = format!("https://openapi.tidal.com/v2/tracks/{}", params.id);
    let v2 = state
        .tidal_client
        .make_catalog_authed_request(
            &url,
            Some(vec![
                ("countryCode", &state.config.country_code),
                ("include", "lyrics"),
            ]),
        )
        .await;
    match v2.map(|body| lyrics_from_openapi(params.id, &body)) {
        Ok(Some(data)) => Ok(lyrics_response(&state, data)),
        Ok(None) => Err(v1_error),
        Err(e) => {
            tracing::debug!(track = params.id, error = %e, "openapi v2 lyrics fallback failed");
            Err(v1_error)
        }
    }
}

fn lyrics_response(state: &AppState, data: Value) -> Json<Value> {
    Json(json!({
        "version": state.config.api_version,
        "lyrics": data
    }))
}

/// Lyrics from an openapi v2 `tracks/{id}?include=lyrics` answer, in the v1
/// shape clients already read: `lyrics` is plain text, `subtitles` the LRC.
/// None when Tidal has no text for the track.
fn lyrics_from_openapi(track_id: i64, body: &Value) -> Option<Value> {
    let included = body["included"].as_array()?;
    let item = included
        .iter()
        .find(|item| item["type"].as_str() == Some("lyrics"))
        .or_else(|| included.first())?;
    let attributes = &item["attributes"];
    let non_empty = |key: &str| attributes[key].as_str().map(str::trim).filter(|s| !s.is_empty());
    let lrc = non_empty("lrcText");
    let text = non_empty("text")
        .map(str::to_string)
        .or_else(|| lrc.map(lrc_to_text))
        .filter(|s| !s.is_empty())?;
    Some(json!({
        "trackId": track_id,
        "lyrics": text,
        "subtitles": lrc,
        "isRightToLeft": attributes["direction"].as_str() == Some("RIGHT_TO_LEFT"),
        "source": "openapi_v2",
    }))
}

/// Plain text from LRC: time tags are dropped, as are tag-only lines like `[ar:…]`.
fn lrc_to_text(lrc: &str) -> String {
    lrc.lines()
        .filter_map(|line| {
            let mut rest = line.trim();
            while let Some(tag) = rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
                if !tag.0.starts_with(|c: char| c.is_ascii_digit()) {
                    return None;
                }
                rest = tag.1.trim_start();
            }
            Some(rest)
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_lyrics_take_the_v1_shape() {
        let body = json!({
            "data": {"id": "1", "type": "tracks"},
            "included": [
                {"id": "x", "type": "artists", "attributes": {}},
                {"id": "1", "type": "lyrics", "attributes": {
                    "text": "Hello\nWorld", "lrcText": "[00:01.00]Hello\n[00:02.50]World", "direction": "LEFT_TO_RIGHT"
                }}
            ]
        });
        let lyrics = lyrics_from_openapi(1, &body).unwrap();
        assert_eq!(lyrics["trackId"], 1);
        assert_eq!(lyrics["lyrics"], "Hello\nWorld");
        assert_eq!(lyrics["subtitles"], "[00:01.00]Hello\n[00:02.50]World");
        assert_eq!(lyrics["isRightToLeft"], false);
    }

    #[test]
    fn plain_text_comes_from_lrc_when_missing() {
        let body = json!({"included": [{"type": "lyrics", "attributes": {
            "lrcText": "[ar:Someone]\n[00:01.00][00:30.00]Chorus\n[00:05.00]\n[00:06.00] Line"
        }}]});
        assert_eq!(lyrics_from_openapi(7, &body).unwrap()["lyrics"], "Chorus\n\nLine");
    }

    #[test]
    fn no_text_means_no_lyrics() {
        assert_eq!(lyrics_from_openapi(1, &json!({"data": {}})), None);
        assert_eq!(lyrics_from_openapi(1, &json!({"included": []})), None);
        let blank = json!({"included": [{"type": "lyrics", "attributes": {"text": " ", "lrcText": ""}}]});
        assert_eq!(lyrics_from_openapi(1, &blank), None);
    }
}
