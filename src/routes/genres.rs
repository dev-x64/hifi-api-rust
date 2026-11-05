use std::collections::HashMap;

use std::sync::atomic::Ordering;

use axum::extract::{RawQuery, State};
use axum::http::HeaderValue;
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::future::try_join_all;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::genre_cache::Entry;
use crate::AppState;

/// Tidal's `filter[...]` accepts at most 20 values per request.
const CHUNK: usize = 20;
/// Values per call to this route: five parallel upstream requests.
const MAX_VALUES: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Key {
    Isrc,
    Id,
}

impl Key {
    fn filter(self) -> &'static str {
        match self {
            Key::Isrc => "filter[isrc]",
            Key::Id => "filter[id]",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Key::Isrc => "isrc",
            Key::Id => "id",
        }
    }
}

/// Track genres for up to 100 ISRCs or track ids per call:
/// `/genres/?isrc=A,B` or `/genres/?id=1&id=2`. Tracks found in the genre
/// cache skip Tidal; the rest are fetched in chunks of 20 and cached.
pub async fn get_genres(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
) -> Result<Response, AppError> {
    let (key, values) = parse_query(query.as_deref())?;
    let cache = state.genre_cache.as_deref().filter(|c| c.enabled());
    let mut found: HashMap<String, Value> = HashMap::new();
    if let Some(cache) = cache {
        for (value, hit) in cache.lookup(key == Key::Isrc, &values).await {
            found.insert(value, item(hit.track_id, hit.isrc.as_deref(), &hit.genres));
        }
        cache.hits.fetch_add(found.len() as u64, Ordering::Relaxed);
    }
    let missing: Vec<String> = values.iter().filter(|v| !found.contains_key(*v)).cloned().collect();
    if !missing.is_empty() {
        let url = "https://openapi.tidal.com/v2/tracks";
        let country = state.config.country_code.as_str();
        let pages = try_join_all(missing.chunks(CHUNK).map(|chunk| {
            let mut params: Vec<(&str, &str)> =
                chunk.iter().map(|v| (key.filter(), v.as_str())).collect();
            params.push(("countryCode", country));
            params.push(("include", "genres"));
            state.tidal_client.make_catalog_authed_request(url, Some(params))
        }))
        .await?;
        let mut entries = Vec::new();
        let mut names = HashMap::new();
        for page in &pages {
            let (page_entries, page_names) = parse_page(page);
            for entry in page_entries {
                let lookup = match key {
                    Key::Isrc => entry.isrc.clone(),
                    Key::Id => Some(entry.track_id.to_string()),
                };
                if let Some(lookup) = lookup {
                    let genres: Vec<String> = entry
                        .genre_ids
                        .iter()
                        .filter_map(|id| page_names.get(id).cloned())
                        .collect();
                    found
                        .entry(lookup)
                        .or_insert_with(|| item(entry.track_id, entry.isrc.as_deref(), &genres));
                }
                entries.push(entry);
            }
            names.extend(page_names);
        }
        if let Some(cache) = cache {
            cache.misses.fetch_add(missing.len() as u64, Ordering::Relaxed);
            cache.store(entries, names);
        }
    }

    let mut items = Vec::new();
    let mut not_found = Vec::new();
    for value in values {
        match found.remove(&value) {
            Some(item) => items.push(item),
            None => not_found.push(value),
        }
    }
    let mut response = Json(json!({
        "version": state.config.api_version,
        "items": items,
        "notFound": not_found,
    }))
    .into_response();
    if cache.is_some() {
        // Read by the request log: a full cache answer is not Tidal traffic.
        let label = if missing.is_empty() { "HIT" } else { "MISS" };
        response.headers_mut().insert("X-Cache", HeaderValue::from_static(label));
    }
    Ok(response)
}

fn item(track_id: i64, isrc: Option<&str>, genres: &[String]) -> Value {
    json!({"id": track_id, "isrc": isrc, "genres": genres})
}

/// Accepts either `isrc` or `id`, each comma-separated and/or repeated.
/// Values are trimmed, deduplicated in order, and ISRCs upper-cased.
fn parse_query(query: Option<&str>) -> Result<(Key, Vec<String>), AppError> {
    let mut isrcs = Vec::new();
    let mut ids = Vec::new();
    for (k, v) in form_urlencoded::parse(query.unwrap_or("").as_bytes()) {
        let target = match k.as_ref() {
            "isrc" => &mut isrcs,
            "id" => &mut ids,
            _ => continue,
        };
        target.extend(v.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from));
    }
    let (key, raw) = match (isrcs.is_empty(), ids.is_empty()) {
        (false, true) => (Key::Isrc, isrcs),
        (true, false) => (Key::Id, ids),
        (true, true) => return Err(AppError::BadRequest("Pass isrc or id".into())),
        (false, false) => return Err(AppError::BadRequest("Pass either isrc or id, not both".into())),
    };
    let mut values: Vec<String> = Vec::with_capacity(raw.len());
    for value in raw {
        let value = match key {
            Key::Isrc => value.to_ascii_uppercase(),
            Key::Id => value,
        };
        let valid = match key {
            Key::Isrc => value.len() == 12 && value.bytes().all(|b| b.is_ascii_alphanumeric()),
            Key::Id => value.bytes().all(|b| b.is_ascii_digit()) && value.parse::<i64>().is_ok(),
        };
        if !valid {
            return Err(AppError::BadRequest(format!("Invalid {}: {}", key.name(), value)));
        }
        if !values.contains(&value) {
            values.push(value);
        }
    }
    if values.len() > MAX_VALUES {
        return Err(AppError::BadRequest(format!(
            "At most {} values per request, got {}",
            MAX_VALUES,
            values.len()
        )));
    }
    Ok((key, values))
}

/// Tracks of an Open API v2 page as cache entries, plus genre id → name
/// from `included`.
fn parse_page(page: &Value) -> (Vec<Entry>, HashMap<String, String>) {
    let names: HashMap<String, String> = page
        .get("included")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|inc| inc.get("type").and_then(Value::as_str) == Some("genres"))
        .filter_map(|inc| {
            Some((
                inc.get("id")?.as_str()?.to_string(),
                inc.get("attributes")?.get("genreName")?.as_str()?.to_string(),
            ))
        })
        .collect();
    let entries = page
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|track| {
            let track_id = track.get("id")?.as_str()?.parse::<i64>().ok()?;
            let isrc = track
                .get("attributes")
                .and_then(|a| a.get("isrc"))
                .and_then(Value::as_str)
                .map(str::to_ascii_uppercase);
            let genre_ids = track
                .get("relationships")
                .and_then(|r| r.get("genres"))
                .and_then(|g| g.get("data"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|g| g.get("id")?.as_str().map(String::from))
                .filter(|id| names.contains_key(id))
                .collect();
            Some(Entry { track_id, isrc, genre_ids })
        })
        .collect();
    (entries, names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commas_and_repeats_dedups_and_uppercases() {
        let (key, values) = parse_query(Some("isrc=gbaye9200070,%20USSM15900113&isrc=GBAYE9200070")).unwrap();
        assert_eq!(key, Key::Isrc);
        assert_eq!(values, ["GBAYE9200070", "USSM15900113"]);
        let (key, values) = parse_query(Some("id=1,2&id=3")).unwrap();
        assert_eq!(key, Key::Id);
        assert_eq!(values, ["1", "2", "3"]);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse_query(None).is_err());
        assert!(parse_query(Some("isrc=")).is_err());
        assert!(parse_query(Some("isrc=SHORT")).is_err());
        assert!(parse_query(Some("id=12a")).is_err());
        assert!(parse_query(Some("isrc=GBAYE9200070&id=1")).is_err());
        let many = (0..=MAX_VALUES).map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        assert!(parse_query(Some(&format!("id={many}"))).is_err());
        let enough = (0..MAX_VALUES).map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        assert_eq!(parse_query(Some(&format!("id={enough}"))).unwrap().1.len(), MAX_VALUES);
    }

    #[test]
    fn resolves_genre_names_from_included() {
        let page = json!({
            "data": [
                {"id": "58990486", "type": "tracks", "attributes": {"isrc": "GBAYE9200070"},
                 "relationships": {"genres": {"data": [{"id": "7", "type": "genres"}]}}},
                {"id": "2178486", "type": "tracks", "attributes": {"isrc": "USSM15900113"},
                 "relationships": {"genres": {"data": [{"id": "3", "type": "genres"}, {"id": "9", "type": "genres"}]}}},
                {"id": "5", "type": "tracks", "attributes": {"isrc": "XX0000000001"},
                 "relationships": {"genres": {"data": []}}}
            ],
            "included": [
                {"id": "7", "type": "genres", "attributes": {"genreName": "Alternative"}},
                {"id": "3", "type": "genres", "attributes": {"genreName": "Jazz"}},
                {"id": "9", "type": "genres", "attributes": {"genreName": "Cool Jazz"}}
            ]
        });
        let (entries, names) = parse_page(&page);
        assert_eq!(entries[0], Entry { track_id: 58990486, isrc: Some("GBAYE9200070".into()), genre_ids: vec!["7".into()] });
        assert_eq!(entries[1].genre_ids, ["3", "9"]);
        assert!(entries[2].genre_ids.is_empty());
        assert_eq!(names["9"], "Cool Jazz");
    }
}
