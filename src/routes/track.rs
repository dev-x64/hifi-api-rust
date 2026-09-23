use axum::extract::{Path, Query, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::AppState;

#[derive(Deserialize)]
pub struct TrackParams {
    pub id: i64,
    #[serde(default = "default_quality")]
    pub quality: String,
    #[serde(default)]
    pub immersiveaudio: bool,
}

#[derive(Deserialize)]
pub struct TrackPathParams {
    #[serde(default = "default_quality")]
    pub quality: String,
    #[serde(default)]
    pub immersiveaudio: bool,
}

#[derive(Deserialize)]
pub struct TrackQualityPathParams {
    #[serde(default)]
    pub immersiveaudio: bool,
}

fn default_quality() -> String {
    "HIGH".to_string()
}

pub async fn get_track(
    State(state): State<AppState>,
    Query(params): Query<TrackParams>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    dispatch_track(&state, params.id, &params.quality, params.immersiveaudio, &headers).await
}

pub async fn get_track_path(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<TrackPathParams>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    dispatch_track(&state, id, &params.quality, params.immersiveaudio, &headers).await
}

pub async fn get_track_quality_path(
    State(state): State<AppState>,
    Path((id, quality)): Path<(i64, String)>,
    Query(params): Query<TrackQualityPathParams>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    dispatch_track(&state, id, &quality, params.immersiveaudio, &headers).await
}

#[derive(Debug, PartialEq, Eq)]
enum TrackFormat {
    High,
    V2(&'static str),
}

/// v1 serves HIGH; all other supported qualities use one exact v2 format.
fn track_format(quality: &str) -> Option<TrackFormat> {
    match quality.trim().to_ascii_uppercase().as_str() {
        "HIGH" => Some(TrackFormat::High),
        "LOW" | "HEAACV1" => Some(TrackFormat::V2("HEAACV1")),
        "AACLC" => Some(TrackFormat::V2("AACLC")),
        "LOSSLESS" | "FLAC" => Some(TrackFormat::V2("FLAC")),
        "HI_RES_LOSSLESS" | "FLAC_HIRES" => Some(TrackFormat::V2("FLAC_HIRES")),
        "DOLBY_ATMOS" | "ATMOS" | "EAC3_JOC" => Some(TrackFormat::V2("EAC3_JOC")),
        _ => None,
    }
}

fn unsupported_quality_info(id: i64, quality: &str) -> Value {
    json!({
        "status": "unsupported_quality",
        "trackId": id,
        "requestedQuality": quality,
        "defaultQuality": "HIGH",
        "supportedQualities": [
            {"quality": "HIGH", "api": "v1", "format": "AAC", "bitrate": "up to 320 kbps"},
            {"quality": "LOW", "aliases": ["HEAACV1"], "api": "v2", "format": "HEAACV1"},
            {"quality": "AACLC", "api": "v2", "format": "AACLC"},
            {"quality": "LOSSLESS", "aliases": ["FLAC"], "api": "v2", "format": "FLAC"},
            {"quality": "HI_RES_LOSSLESS", "aliases": ["FLAC_HIRES"], "api": "v2", "format": "FLAC_HIRES"},
            {"quality": "DOLBY_ATMOS", "aliases": ["ATMOS", "EAC3_JOC"], "api": "v2", "format": "EAC3_JOC"}
        ]
    })
}

fn unsupported_quality_response(id: i64, quality: &str) -> Response {
    Json(unsupported_quality_info(id, quality)).into_response()
}

async fn dispatch_track(
    state: &AppState,
    id: i64,
    quality: &str,
    immersive: bool,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let Some(format) = track_format(quality) else {
        return Ok(unsupported_quality_response(id, quality));
    };
    let op = match format {
        TrackFormat::High => crate::playback::PlaybackOp::Track {
            id,
            quality: "HIGH".into(),
            immersive,
        },
        TrackFormat::V2(format) => crate::playback::PlaybackOp::Manifest {
            track_id: id.to_string(),
            params: TrackManifestsParams {
                adaptive: default_adaptive(),
                manifestType: default_manifest_type(),
                uriScheme: default_uri_scheme(),
                usage: default_usage(),
                countryCode: None,
                atmos: None,
            },
            raw_query: Some(format!("formats={}", format)),
            host: headers
                .get("host")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("localhost")
                .to_string(),
        },
    };
    state.playback.dispatch(state, op).await
}

/// Core /track/ fetch (shared by immediate and queued execution).
pub(crate) async fn fetch_track_playback(
    state: &AppState,
    id: i64,
    quality: &str,
    immersive: bool,
) -> Result<Value, AppError> {
    let url = format!("https://api.tidal.com/v1/tracks/{}/playbackinfo", id);
    let immersive_str = if immersive { "true" } else { "false" };
    let result = state
        .tidal_client
        .make_request(
            &url,
            Some(vec![
                ("audioquality", quality),
                ("playbackmode", "STREAM"),
                ("assetpresentation", "FULL"),
                ("immersiveaudio", immersive_str),
            ]),
        )
        .await?;
    if result
        .pointer("/data/assetPresentation")
        .and_then(|v| v.as_str())
        == Some("PREVIEW")
    {
        let reason = result
            .pointer("/data/previewReason")
            .and_then(|v| v.as_str())
            .unwrap_or("FULL_REQUIRES_SUBSCRIPTION");
        return Err(AppError::ServiceUnavailable(format!(
            "Preview only ({}): track {} requires subscription or is not available as FULL in this region",
            reason, id
        )));
    }
    // Tidal silently substitutes the stereo sibling's audio when a stereo
    // tier is asked for an Atmos-only id (e.g. 527739156 → 479222720,
    // signalled by `trackId` naming the sibling). Never serve track Y for
    // track X — fail so callers try the next tier/account instead of
    // mislabeling another recording's bytes.
    for ptr in ["/data/trackId", "/data/data/trackId"] {
        if let Some(seen) = result.pointer(ptr).and_then(json_track_id) {
            if seen != id.to_string() {
                return Err(AppError::UpstreamError(
                    StatusCode::CONFLICT,
                    format!(
                        "Tidal returned audio for track {} for requested {}: refusing to serve a different track",
                        seen, id
                    ),
                ));
            }
            break;
        }
    }
    Ok(result)
}

/// A track id from Tidal JSON: number or string.
fn json_track_id(v: &Value) -> Option<String> {
    if let Some(n) = v.as_i64() {
        return Some(n.to_string());
    }
    if let Some(n) = v.as_u64() {
        return Some(n.to_string());
    }
    v.as_str().map(|s| s.trim().to_string())
}

/// Extract the track id embedded in a Tidal DASH manifest URL:
/// `…/1/manifests/<base64>.mpd?…` decodes (protobuf) to `\x12\t<trackId>…`.
/// Returns `None` when the URL carries no verifiable id (accept).
pub(crate) fn manifest_track_id(mpd_url: &str) -> Option<String> {
    let marker = "/manifests/";
    let start = mpd_url.find(marker)? + marker.len();
    let rest = &mpd_url[start..];
    let end = rest
        .find(|c| c == '.' || c == '?' || c == '/' || c == '&')
        .unwrap_or(rest.len());
    let mut b64 = rest[..end].replace('-', "+").replace('_', "/");
    while b64.len() % 4 != 0 {
        b64.push('=');
    }
    use base64::Engine as _;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(&b64)
        .ok()?;
    let s = String::from_utf8_lossy(&raw);
    let mut run = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            run.push(ch);
        } else if run.len() >= 5 {
            return Some(run);
        } else {
            run.clear();
        }
    }
    if run.len() >= 5 {
        Some(run)
    } else {
        None
    }
}

fn manifest_matches_track(uri: &str, track_id: &str) -> bool {
    match manifest_track_id(uri) {
        None => true,
        Some(mid) => mid == track_id.trim(),
    }
}

#[derive(Deserialize, Clone)]
#[allow(non_snake_case)]
pub struct TrackManifestsParams {
    #[serde(default = "default_adaptive")]
    pub adaptive: String,
    #[serde(default = "default_manifest_type")]
    pub manifestType: String,
    #[serde(default = "default_uri_scheme")]
    pub uriScheme: String,
    #[serde(default = "default_usage")]
    pub usage: String,
    #[serde(default)]
    pub countryCode: Option<String>,
    /// Atmos preference: true|prefer|only|off. Overrides the server default.
    /// Note: EAC3_JOC needs a Dolby-capable player + Widevine license.
    #[serde(default)]
    pub atmos: Option<String>,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct TrackManifestsQueryParams {
    pub id: String,
    #[serde(default = "default_adaptive")]
    pub adaptive: String,
    #[serde(default = "default_manifest_type")]
    pub manifestType: String,
    #[serde(default = "default_uri_scheme")]
    pub uriScheme: String,
    #[serde(default = "default_usage")]
    pub usage: String,
    #[serde(default)]
    pub countryCode: Option<String>,
    #[serde(default)]
    pub atmos: Option<String>,
}

#[derive(Deserialize)]
pub struct DashParams {
    #[serde(default)]
    pub atmos: Option<String>,
}

/// Resolve the effective format list: explicit `formats=` (comma or
/// repeated) wins, then Atmos preference moves EAC3_JOC first (`prefer`),
/// keeps only it (`only`), or strips it (`off`).
pub(crate) fn resolve_formats(raw_query: Option<&str>, atmos: Option<&str>, default_prefer: bool) -> Vec<String> {
    let mut explicit: Option<Vec<String>> = None;
    if let Some(q) = raw_query {
        let mut out = Vec::new();
        for (k, v) in form_urlencoded::parse(q.as_bytes()) {
            if k == "formats" {
                for part in v.split(',') {
                    let p = part.trim();
                    if !p.is_empty() {
                        out.push(p.to_string());
                    }
                }
            }
        }
        if !out.is_empty() {
            explicit = Some(out);
        }
    }

    let mode = atmos
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    match mode.as_deref() {
        // Explicit Atmos-only request.
        Some("only") => vec!["EAC3_JOC".to_string()],
        Some("true") | Some("1") | Some("prefer") => {
            // Ensure Atmos leads (add it if the explicit list lacks it).
            let mut formats = explicit.unwrap_or_else(default_formats);
            formats.retain(|f| f != "EAC3_JOC");
            formats.insert(0, "EAC3_JOC".to_string());
            formats
        }
        // Explicit lists are always respected verbatim.
        _ if explicit.is_some() => explicit.unwrap_or_else(default_formats),
        Some("false") | Some("0") | Some("off") => {
            let mut formats = default_formats();
            formats.retain(|f| f != "EAC3_JOC");
            formats
        }
        // No preference expressed: server default.
        _ if default_prefer => {
            let mut formats = default_formats();
            formats.retain(|f| f != "EAC3_JOC");
            formats.insert(0, "EAC3_JOC".to_string());
            formats
        }
        _ => default_formats(),
    }
}

pub(crate) fn atmos_default_on(state: &AppState) -> bool {
    state
        .settings
        .atmos_mode
        .read()
        .map(|m| m.as_str() == "prefer")
        .unwrap_or(false)
}

fn high_default_on(state: &AppState) -> bool {
    state
        .settings
        .atmos_mode
        .read()
        .map(|m| m.as_str() == "high")
        .unwrap_or(false)
}

fn use_high_for_dash(atmos: Option<&str>, high_default: bool) -> bool {
    high_default
        && !matches!(
            atmos.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
            Some("true" | "1" | "prefer" | "only")
        )
}

fn high_audio_uri(result: &Value) -> Result<String, AppError> {
    if result.pointer("/data/audioQuality").and_then(Value::as_str) != Some("HIGH") {
        return Err(AppError::ServiceUnavailable(
            "Tidal did not return HIGH audio".into(),
        ));
    }
    let manifest = result
        .pointer("/data/manifest")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Internal("No v1 audio manifest in response".into()))?;
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(manifest)
        .map_err(|_| AppError::Internal("Invalid v1 audio manifest encoding".into()))?;
    let decoded: Value = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::Internal("Invalid v1 audio manifest JSON".into()))?;
    let uri = decoded
        .pointer("/urls/0")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::Internal("No URL in v1 audio manifest".into()))?;
    let parsed = reqwest::Url::parse(uri)
        .map_err(|_| AppError::Internal("Invalid URL in v1 audio manifest".into()))?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() {
        return Err(AppError::Internal("Invalid URL in v1 audio manifest".into()));
    }
    Ok(uri.to_string())
}

/// True when the Tidal manifest actually carries a Dolby Atmos rendition.
fn manifest_has_atmos(result: &Value) -> bool {
    let attrs = result.pointer("/data/data/attributes");
    if let Some(formats) = attrs.and_then(|a| a.get("formats")).and_then(|f| f.as_array()) {
        if formats.iter().any(|f| f.as_str() == Some("EAC3_JOC")) {
            return true;
        }
    }
    if let Some(modes) = attrs.and_then(|a| a.get("audioModes")).and_then(|m| m.as_array()) {
        if modes.iter().any(|m| m.as_str() == Some("DOLBY_ATMOS")) {
            return true;
        }
    }
    false
}

fn default_formats() -> Vec<String> {
    vec![
        "HEAACV1".into(),
        "AACLC".into(),
        "FLAC".into(),
        "FLAC_HIRES".into(),
        "EAC3_JOC".into(),
    ]
}
fn default_adaptive() -> String {
    "true".into()
}
fn default_manifest_type() -> String {
    "MPEG_DASH".into()
}
fn default_uri_scheme() -> String {
    "HTTPS".into()
}
fn default_usage() -> String {
    "PLAYBACK".into()
}

pub(crate) async fn fetch_manifest_inner(
    state: &AppState,
    track_id: &str,
    params: &TrackManifestsParams,
    host: &str,
    raw_query: Option<&str>,
) -> Result<Value, AppError> {    let formats = resolve_formats(raw_query, params.atmos.as_deref(), atmos_default_on(state));
    let url = format!("https://openapi.tidal.com/v2/trackManifests/{}", track_id);

    let country_code = params
        .countryCode
        .as_deref()
        .unwrap_or(&state.config.country_code);
    let mut all_params: Vec<(&str, &str)> = vec![
        ("adaptive", params.adaptive.as_str()),
        ("manifestType", params.manifestType.as_str()),
        ("uriScheme", params.uriScheme.as_str()),
        ("usage", params.usage.as_str()),
        ("countryCode", country_code),
    ];

    for fmt in &formats {
        all_params.push(("formats", fmt.as_str()));
    }

    let result = state
        .tidal_client
        .make_request(&url, Some(all_params))
        .await?;

    if result
        .pointer("/data/data/attributes/trackPresentation")
        .and_then(|v| v.as_str())
        == Some("PREVIEW")
    {
        let reason = result
            .pointer("/data/data/attributes/previewReason")
            .and_then(|v| v.as_str())
            .unwrap_or("PREVIEW");
        return Err(AppError::ServiceUnavailable(format!(
            "Preview only ({}): track {} not available as FULL",
            reason, track_id
        )));
    }

    // Same sibling-substitution guard as /track/: Tidal answers stereo
    // formats for an Atmos-only id with the sibling's manifest (no error).
    // Refuse to proxy track Y's bytes for track X.
    if let Some(uri) = result
        .pointer("/data/data/attributes/uri")
        .and_then(|v| v.as_str())
    {
        if !manifest_matches_track(uri, track_id) {
            let seen =
                manifest_track_id(uri).unwrap_or_else(|| "unknown".to_string());
            return Err(AppError::UpstreamError(
                StatusCode::CONFLICT,
                format!(
                    "Tidal returned manifest for track {} for requested {}: refusing to serve a different track",
                    seen, track_id
                ),
            ));
        }
    }

    let mut result = result;
    let atmos_available = manifest_has_atmos(&result);
    if let Some(obj) = result.as_object_mut() {
        obj.insert("atmos_available".into(), json!(atmos_available));
    }
    if let Some(data) = result.get_mut("data") {
        if let Some(data_obj) = data.as_object_mut() {
            if let Some(data_inner) = data_obj.get_mut("data") {
                if let Some(attributes) = data_inner.get("attributes") {
                    if let Some(drm_data) = attributes.get("drmData") {
                        if let Some(_drm_obj) = drm_data.as_object() {
                            let proxy_url = format!("https://{}/widevine", host);
                            if let Some(drm) = data_inner.as_object_mut() {
                                if let Some(attrs) = drm.get_mut("attributes") {
                                    if let Some(attrs_obj) = attrs.as_object_mut() {
                                        if let Some(drm) = attrs_obj.get_mut("drmData") {
                                            if let Some(drm_obj) = drm.as_object_mut() {
                                                drm_obj.insert("licenseUrl".into(), json!(proxy_url.clone()));
                                                drm_obj.insert("certificateUrl".into(), json!(proxy_url));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(result)
}

// Path-based: GET /trackManifests/{id}?formats=...  (our Rust style, keep for compat)
pub async fn get_track_manifests(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
    Query(params): Query<TrackManifestsParams>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Result<Response, AppError> {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost")
        .to_string();
    let op = crate::playback::PlaybackOp::Manifest {
        track_id,
        params,
        raw_query: query,
        host,
    };
    state.playback.dispatch(&state, op).await
}

// Query-based: GET /trackManifests/?id=...&formats=...  (binimum hifi-api style)
pub async fn get_track_manifests_query(
    State(state): State<AppState>,
    Query(params): Query<TrackManifestsQueryParams>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Result<Response, AppError> {
    let inner = TrackManifestsParams {
        adaptive: params.adaptive.clone(),
        manifestType: params.manifestType.clone(),
        uriScheme: params.uriScheme.clone(),
        usage: params.usage.clone(),
        countryCode: params.countryCode.clone(),
        atmos: params.atmos.clone(),
    };
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost")
        .to_string();
    let op = crate::playback::PlaybackOp::Manifest {
        track_id: params.id,
        params: inner,
        raw_query: query,
        host,
    };
    state.playback.dispatch(&state, op).await
}

#[cfg(test)]
mod tests {
    use super::{
        default_quality, high_audio_uri, manifest_matches_track, manifest_track_id,
        resolve_formats, track_format, unsupported_quality_info,
        unsupported_quality_response, use_high_for_dash, TrackFormat,
    };
    use axum::http::StatusCode;
    use base64::Engine as _;
    use serde_json::json;

    #[test]
    fn track_quality_selects_v1_or_exact_v2_format() {
        assert_eq!(default_quality(), "HIGH");
        assert_eq!(track_format(" high "), Some(TrackFormat::High));
        assert_eq!(track_format("LOW"), Some(TrackFormat::V2("HEAACV1")));
        assert_eq!(track_format("AACLC"), Some(TrackFormat::V2("AACLC")));
        assert_eq!(track_format("LOSSLESS"), Some(TrackFormat::V2("FLAC")));
        assert_eq!(track_format("HI_RES_LOSSLESS"), Some(TrackFormat::V2("FLAC_HIRES")));
        assert_eq!(track_format("DOLBY_ATMOS"), Some(TrackFormat::V2("EAC3_JOC")));
        assert_eq!(track_format("unknown"), None);
    }

    #[test]
    fn unsupported_track_quality_returns_supported_formats() {
        assert_eq!(
            unsupported_quality_response(1781887, "WAV").status(),
            StatusCode::OK
        );
        let info = unsupported_quality_info(1781887, "WAV");
        assert_eq!(info["status"], "unsupported_quality");
        assert_eq!(info["trackId"], 1781887);
        assert_eq!(info["requestedQuality"], "WAV");
        assert_eq!(info["defaultQuality"], "HIGH");
        assert_eq!(info["supportedQualities"][0]["quality"], "HIGH");
        assert_eq!(info["supportedQualities"][4]["format"], "FLAC_HIRES");
    }

    #[test]
    fn requested_track_format_overrides_atmos_default() {
        assert_eq!(
            resolve_formats(Some("formats=FLAC_HIRES"), None, true),
            vec!["FLAC_HIRES".to_string()]
        );
    }

    #[test]
    fn high_dash_uses_v1_unless_atmos_is_requested() {
        assert!(use_high_for_dash(None, true));
        assert!(use_high_for_dash(Some("off"), true));
        assert!(!use_high_for_dash(Some("prefer"), true));
        assert!(!use_high_for_dash(Some("only"), true));
        assert!(!use_high_for_dash(None, false));
    }

    #[test]
    fn high_manifest_requires_high_quality_and_direct_url() {
        let manifest = base64::engine::general_purpose::STANDARD
            .encode(r#"{"mimeType":"audio/mp4","urls":["https://example.com/audio.mp4"]}"#);
        let result = json!({"data": {"audioQuality": "HIGH", "manifest": manifest}});
        assert_eq!(
            high_audio_uri(&result).unwrap(),
            "https://example.com/audio.mp4"
        );
        let lower = json!({"data": {"audioQuality": "LOW", "manifest": manifest}});
        assert!(high_audio_uri(&lower).is_err());
    }

    #[test]
    fn atmos_only_forces_single_format() {
        assert_eq!(
            resolve_formats(Some("formats=FLAC"), Some("only"), false),
            vec!["EAC3_JOC".to_string()]
        );
    }

    #[test]
    fn atmos_prefer_leads_with_eac3() {
        let out = resolve_formats(Some("formats=FLAC,AACLC"), Some("true"), false);
        assert_eq!(out[0], "EAC3_JOC");
        assert!(out.contains(&"FLAC".to_string()));
        assert!(out.contains(&"AACLC".to_string()));
        // No duplicates.
        let mut sorted = out.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), out.len());
    }

    #[test]
    fn explicit_formats_respected_verbatim_by_default() {
        let out = resolve_formats(Some("formats=FLAC&formats=AACLC"), None, false);
        assert_eq!(out, vec!["FLAC".to_string(), "AACLC".to_string()]);
    }

    #[test]
    fn atmos_off_strips_from_defaults_only() {
        let out = resolve_formats(None, Some("off"), false);
        assert!(!out.contains(&"EAC3_JOC".to_string()));
        assert!(out.contains(&"FLAC".to_string()));
        // ...but an explicit request is still honored.
        let out2 = resolve_formats(Some("formats=EAC3_JOC"), Some("off"), false);
        assert_eq!(out2, vec!["EAC3_JOC".to_string()]);
    }

    #[test]
    fn server_default_prefer_mode() {
        let out = resolve_formats(None, None, true);
        assert_eq!(out[0], "EAC3_JOC");
        let out2 = resolve_formats(None, None, false);
        assert!(out2.contains(&"EAC3_JOC".to_string()));
        assert_ne!(out2[0], "EAC3_JOC");
    }

    #[test]
    fn garbage_atmos_falls_back_to_defaults() {
        let out = resolve_formats(None, Some("banana"), false);
        assert!(out.contains(&"FLAC_HIRES".to_string()));
    }

    #[test]
    fn manifest_id_decodes_known_vectors() {
        // im-cf manifests observed live: sibling substitution is detectable
        // only via this embedded id (no trackId field in the JSON).
        assert_eq!(
            manifest_track_id("https://im-cf.manifest.tidal.com/1/manifests/Egk1Mjc3MzkxNTYYAigBWO6T5GNgtGZqCFBMQVlCQUNLcgEFeAOAAQKIAQA.mpd?Expires=1"),
            Some("527739156".to_string())
        );
        assert_eq!(
            manifest_track_id("https://im-cf.manifest.tidal.com/1/manifests/Egk0NzkyMjI3MjAYAigBWL7L52NgtGZqCFBMQVlCQUNLcgQCAwQBeAOAAQKIAQE.mpd?Expires=1"),
            Some("479222720".to_string())
        );
        assert_eq!(
            manifest_track_id("https://im-cf.manifest.tidal.com/1/manifests/EgkyMzA5MDkzOTAYAigBWPWa52NgtGZqCFBMQVlCQUNLcgEEeAOAAQKIAQA.mpd?Expires=1"),
            Some("230909390".to_string())
        );
        assert_eq!(
            manifest_track_id("https://im-cf.manifest.tidal.com/1/manifests/EgkyMzA4ODk4NjgYAigBWNDj4mNgtGZqCFBMQVlCQUNLcgEFeAOAAQKIAQA.mpd?Expires=1"),
            Some("230889868".to_string())
        );
    }

    #[test]
    fn manifest_match_accepts_unverifiable_but_rejects_sibling() {
        assert!(manifest_matches_track("https://cdn.example.com/file.mpd", "123"));
        assert!(manifest_matches_track(
            "https://im-cf.manifest.tidal.com/1/manifests/EgkyMzA5MDkzOTAYAigBWPWa52NgtGZqCFBMQVlCQUNLcgEEeAOAAQKIAQA.mpd",
            "230909390"
        ));
        assert!(!manifest_matches_track(
            "https://im-cf.manifest.tidal.com/1/manifests/EgkyMzA5MDkzOTAYAigBWPWa52NgtGZqCFBMQVlCQUNLcgEEeAOAAQKIAQA.mpd",
            "230889868"
        ));
    }
}

pub async fn get_dash_stream(
    State(state): State<AppState>,
    Path(track_id): Path<String>,
    Query(params): Query<DashParams>,
) -> Result<Response, AppError> {
    let op = crate::playback::PlaybackOp::Dash {
        track_id,
        atmos: params.atmos,
    };
    state.playback.dispatch(&state, op).await
}

/// Core /dash/ fetch returning a DASH manifest or direct AAC URI.
pub(crate) async fn fetch_dash_uri(
    state: &AppState,
    track_id: &str,
    atmos: Option<&str>,
) -> Result<String, AppError> {
    // HIGH is a direct AAC file from v1. No v2 request is needed when the
    // server preference is HIGH and the caller did not request Atmos.
    if use_high_for_dash(atmos, high_default_on(state)) {
        let id = track_id
            .parse::<i64>()
            .map_err(|_| AppError::BadRequest("Invalid track id".into()))?;
        let result = fetch_track_playback(state, id, "HIGH", false).await?;
        return high_audio_uri(&result);
    }
    let url = format!("https://openapi.tidal.com/v2/trackManifests/{}", track_id);

    // Same Atmos semantics as /trackManifests, over the fixed /dash chain.
    let mode = atmos
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let formats = match mode.as_deref() {
        Some("only") => "EAC3_JOC",
        Some("true") | Some("1") | Some("prefer") => "EAC3_JOC,FLAC_HIRES,FLAC,AACLC",
        Some("false") | Some("0") | Some("off") => "FLAC_HIRES,FLAC,AACLC",
        _ if atmos_default_on(state) => "EAC3_JOC,FLAC_HIRES,FLAC,AACLC",
        _ => "FLAC_HIRES,FLAC,EAC3_JOC,AACLC",
    };

    let all_params: Vec<(&str, &str)> = vec![
        ("adaptive", "true"),
        ("manifestType", "MPEG_DASH"),
        ("uriScheme", "HTTPS"),
        ("usage", "PLAYBACK"),
        ("countryCode", &state.config.country_code),
        ("formats", formats),
    ];

    let result = state
        .tidal_client
        .make_request(&url, Some(all_params))
        .await?;

    if result
        .pointer("/data/data/attributes/trackPresentation")
        .and_then(|v| v.as_str())
        == Some("PREVIEW")
    {
        let reason = result
            .pointer("/data/data/attributes/previewReason")
            .and_then(|v| v.as_str())
            .unwrap_or("PREVIEW");
        return Err(AppError::ServiceUnavailable(format!(
            "Preview only ({}): track {} not available as FULL",
            reason, track_id
        )));
    }

    let uri = result
        .pointer("/data/data/attributes/uri")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::Internal("No manifest URI in response".into()))?;

    if !manifest_matches_track(uri, track_id) {
        let seen = manifest_track_id(uri).unwrap_or_else(|| "unknown".to_string());
        return Err(AppError::UpstreamError(
            StatusCode::CONFLICT,
            format!(
                "Tidal returned manifest for track {} for requested {}: refusing to serve a different track",
                seen, track_id
            ),
        ));
    }

    Ok(uri.to_string())
}
