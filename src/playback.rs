//! Async playback queue (upstream parity with binimum/hifi-api).
//!
//! Upstream serializes playback traffic: each playback credential serves at
//! most one request at a time. When every slot is busy, the request becomes
//! a pollable job (`202 + Location: /playback/requests/{id}`) instead of
//! failing. Jobs expire 300s after finishing and can be cancelled.
//!
//! Slots here are concurrency permits sized to the playback pool
//! (non-catalog accounts); the wrapped operation still uses the normal
//! multi-account failover inside its slot.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

use crate::error::AppError;
use crate::routes::track::TrackManifestsParams;
use crate::AppState;

const JOB_TTL_SECS: i64 = 300;

/// A queued playback operation. All data owned so jobs can outlive the request.
#[derive(Clone)]
pub enum PlaybackOp {
    Track {
        id: i64,
        quality: String,
        immersive: bool,
    },
    Manifest {
        track_id: String,
        params: TrackManifestsParams,
        raw_query: Option<String>,
        host: String,
    },
    Dash {
        track_id: String,
        atmos: Option<String>,
    },
    Video {
        id: i64,
        quality: String,
        mode: String,
        presentation: String,
    },
    Widevine {
        method: String,
        content_type: Option<String>,
        body: Vec<u8>,
    },
}

/// What a finished job holds. Widevine bodies are small (licenses), so
/// keeping them in memory until TTL expiry is fine.
#[derive(Clone)]
pub enum JobResult {
    Json(Value),
    Redirect(String),
    Binary {
        status: u16,
        content_type: String,
        body: Vec<u8>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JobState {
    Pending,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

impl JobState {
    fn as_str(self) -> &'static str {
        match self {
            JobState::Pending => "pending",
            JobState::Processing => "processing",
            JobState::Completed => "completed",
            JobState::Failed => "failed",
            JobState::Cancelled => "cancelled",
        }
    }
}

struct Job {
    id: String,
    state: JobState,
    result: Option<JobResult>,
    error: Option<String>,
    error_status: u16,
    created_at: i64,
    finished_at: Option<i64>,
}

pub struct PlaybackQueue {
    inflight: AtomicUsize,
    jobs: RwLock<HashMap<String, Job>>,
    handles: Mutex<HashMap<String, JoinHandle<()>>>,
}

impl PlaybackQueue {
    pub fn new() -> Self {
        Self {
            inflight: AtomicUsize::new(0),
            jobs: RwLock::new(HashMap::new()),
            handles: Mutex::new(HashMap::new()),
        }
    }

    fn try_acquire(&self, pool: usize) -> bool {
        let mut current = self.inflight.load(Ordering::Relaxed);
        loop {
            if current >= pool.max(1) {
                return false;
            }
            match self.inflight.compare_exchange_weak(
                current,
                current + 1,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }

    fn release(&self) {
        self.inflight.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
            Some(v.saturating_sub(1))
        }).ok();
    }

    fn prune_sync(jobs: &mut HashMap<String, Job>) {
        let cutoff = chrono::Utc::now().timestamp() - JOB_TTL_SECS;
        jobs.retain(|_, j| j.finished_at.map(|f| f >= cutoff).unwrap_or(true));
    }

    async fn prune(&self) {
        let mut jobs = self.jobs.write().await;
        Self::prune_sync(&mut jobs);
    }

    /// Pool size for payloads (mirrors upstream playbackAccounts).
    async fn pool_size(state: &AppState) -> usize {
        state.account_manager.playback_slots().await
    }

    fn payload(
        id: &str,
        state_str: &str,
        position: usize,
        pool: usize,
        active: usize,
    ) -> Value {
        let url = format!("/playback/requests/{}", id);
        json!({
            "status": state_str,
            "requestId": id,
            "queuePosition": position,
            "statusUrl": url,
            "cancelUrl": url,
            "playbackAccounts": pool,
            "activePlaybackRequests": active,
        })
    }

    async fn pending_position(&self, id: &str) -> usize {
        let jobs = self.jobs.read().await;
        let Some(me) = jobs.get(id) else {
            return 0;
        };
        if me.state != JobState::Pending {
            return 0;
        }
        let mut pending: Vec<&Job> = jobs
            .values()
            .filter(|j| j.state == JobState::Pending)
            .collect();
        pending.sort_by_key(|j| j.created_at);
        pending
            .iter()
            .position(|j| j.id == id)
            .map(|i| i + 1)
            .unwrap_or(0)
    }

    fn pending_response(id: &str, position: usize, pool: usize, active: usize) -> Response {
        let body = Self::payload(id, "pending", position, pool, active);
        let mut headers = HeaderMap::new();
        headers.insert("Location", format!("/playback/requests/{}", id).parse().unwrap());
        headers.insert("Retry-After", HeaderValue::from_static("1"));
        headers.insert(
            "X-Playback-Queue-Position",
            position.to_string().parse().unwrap_or(HeaderValue::from_static("0")),
        );
        headers.insert(
            "X-Playback-Request-Id",
            id.parse().unwrap_or(HeaderValue::from_static("")),
        );
        (StatusCode::ACCEPTED, headers, Json(body)).into_response()
    }

    /// Run immediately when a slot is free, else enqueue as a pollable job.
    /// Immediate errors propagate as normal HTTP errors (upstream parity:
    /// only saturated-pool requests become 202 jobs).
    pub async fn dispatch(
        &self,
        state: &AppState,
        op: PlaybackOp,
    ) -> Result<Response, AppError> {
        self.prune().await;
        let pool = Self::pool_size(state).await;
        if self.try_acquire(pool) {
            let out = run_op(state, &op).await;
            self.release();
            return match out {
                Ok(result) => Ok(job_result_response(&result)),
                Err(e) => Err(e),
            };
        }

        let id = uuid::Uuid::new_v4().simple().to_string();
        let now = chrono::Utc::now().timestamp();
        {
            let mut jobs = self.jobs.write().await;
            jobs.insert(
                id.clone(),
                Job {
                    id: id.clone(),
                    state: JobState::Pending,
                    result: None,
                    error: None,
                    error_status: 500,
                    created_at: now,
                    finished_at: None,
                },
            );
        }
        let active = self.inflight.load(Ordering::Relaxed);
        let position = self.pending_position(&id).await;

        // Background worker: wait for a slot, then run. Cancellation is
        // cooperative at slot-wait plus JoinHandle abort on DELETE.
        let queue = state.playback.clone();
        let op_owned = op.clone();
        let app = state.clone();
        let job_id = id.clone();
        let handle = tokio::spawn(async move {
            loop {
                // Stop waiting if cancelled while queued.
                {
                    let jobs = queue.jobs.read().await;
                    if matches!(
                        jobs.get(&job_id).map(|j| j.state),
                        Some(JobState::Cancelled) | None
                    ) {
                        return;
                    }
                }
                let pool_now = app.account_manager.playback_slots().await;
                if queue.try_acquire(pool_now) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            {
                let mut jobs = queue.jobs.write().await;
                if let Some(j) = jobs.get_mut(&job_id) {
                    if j.state == JobState::Cancelled {
                        queue.release();
                        return;
                    }
                    j.state = JobState::Processing;
                }
            }
            let out = run_op(&app, &op_owned).await;
            {
                let mut jobs = queue.jobs.write().await;
                if let Some(j) = jobs.get_mut(&job_id) {
                    match out {
                        Ok(result) => {
                            j.state = JobState::Completed;
                            j.result = Some(result);
                        }
                        Err(e) => {
                            let (status, detail) = app_error_parts(&e);
                            j.state = JobState::Failed;
                            j.error = Some(detail);
                            j.error_status = status;
                        }
                    }
                    j.finished_at = Some(chrono::Utc::now().timestamp());
                }
            }
            queue.release();
            let mut handles = queue.handles.lock().await;
            handles.remove(&job_id);
        });
        {
            let mut handles = self.handles.lock().await;
            handles.insert(id.clone(), handle);
        }

        Ok(Self::pending_response(&id, position, pool, active))
    }

    async fn snapshot(&self, pool: usize) -> (usize, usize, usize) {
        let jobs = self.jobs.read().await;
        let pending = jobs.values().filter(|j| j.state == JobState::Pending).count();
        let total = jobs.len();
        let active = self.inflight.load(Ordering::Relaxed);
        let _ = pool;
        (active, pending, total)
    }

    /// Queue counters for /admin/stats.
    pub async fn stats(&self, pool: usize) -> Value {
        let (active, pending, total) = self.snapshot(pool).await;
        json!({
            "pool_size": pool,
            "active": active,
            "pending": pending,
            "jobs": total,
        })
    }
}

impl Default for PlaybackQueue {
    fn default() -> Self {
        Self::new()
    }
}

fn app_error_parts(e: &AppError) -> (u16, String) {
    match e {
        AppError::NotFound(d) => (404, d.clone()),
        AppError::BadRequest(d) => (400, d.clone()),
        AppError::Unauthorized(d) => (401, d.clone()),
        AppError::UpstreamError(s, d) => (s.as_u16(), d.clone()),
        AppError::Timeout => (429, "Upstream timeout".into()),
        AppError::ServiceUnavailable(d) => (503, d.clone()),
        AppError::ServiceUnavailableRetry(d, _) => (503, d.clone()),
        AppError::Internal(d) => (500, d.clone()),
    }
}

fn job_result_response(result: &JobResult) -> Response {
    match result {
        JobResult::Json(v) => Json(v.clone()).into_response(),
        JobResult::Redirect(uri) => axum::response::Redirect::temporary(uri).into_response(),
        JobResult::Binary {
            status,
            content_type,
            body,
        } => {
            let code = StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY);
            (
                code,
                [("Content-Type", content_type.as_str())],
                axum::body::Bytes::from(body.clone()),
            )
                .into_response()
        }
    }
}

async fn run_op(state: &AppState, op: &PlaybackOp) -> Result<JobResult, AppError> {
    match op {
        PlaybackOp::Track {
            id,
            quality,
            immersive,
        } => {
            let v = crate::routes::track::fetch_track_playback(state, *id, quality, *immersive)
                .await?;
            Ok(JobResult::Json(v))
        }
        PlaybackOp::Manifest {
            track_id,
            params,
            raw_query,
            host,
        } => {
            let v = crate::routes::track::fetch_manifest_inner(
                state,
                track_id,
                params,
                host,
                raw_query.as_deref(),
            )
            .await?;
            Ok(JobResult::Json(v))
        }
        PlaybackOp::Dash { track_id, atmos } => {
            let uri = crate::routes::track::fetch_dash_uri(state, track_id, atmos.as_deref())
                .await?;
            Ok(JobResult::Redirect(uri))
        }
        PlaybackOp::Video {
            id,
            quality,
            mode,
            presentation,
        } => {
            let v = crate::routes::video::fetch_video_playback(
                state,
                *id,
                quality,
                mode,
                presentation,
            )
            .await?;
            Ok(JobResult::Json(v))
        }
        PlaybackOp::Widevine {
            method,
            content_type,
            body,
        } => {
            let (status, ct, out) = crate::routes::widevine::fetch_widevine_license(
                state,
                method,
                content_type.as_deref(),
                body,
            )
            .await?;
            Ok(JobResult::Binary {
                status,
                content_type: ct,
                body: out,
            })
        }
    }
}

/// Poll a playback job (upstream: GET /playback/requests/{request_id}).
pub async fn get_playback_request(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> Response {
    state.playback.prune().await;
    let pool = PlaybackQueue::pool_size(&state).await;
    let active = state.playback.inflight.load(Ordering::Relaxed);
    let jobs = state.playback.jobs.read().await;
    let Some(job) = jobs.get(&request_id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"detail": "Playback request not found or expired"})),
        )
            .into_response();
    };
    match job.state {
        JobState::Pending | JobState::Processing => {
            let position = if job.state == JobState::Pending {
                drop(jobs);
                state.playback.pending_position(&request_id).await
            } else {
                0
            };
            PlaybackQueue::pending_response(&request_id, position, pool, active)
        }
        JobState::Completed => {
            let result = job.result.clone().unwrap_or(JobResult::Json(json!({})));
            job_result_response(&result)
        }
        JobState::Cancelled => {
            let mut body = PlaybackQueue::payload(&request_id, "cancelled", 0, pool, active);
            body["detail"] = json!("Playback request was cancelled");
            (StatusCode::GONE, Json(body)).into_response()
        }
        JobState::Failed => {
            let mut body = PlaybackQueue::payload(
                &request_id,
                "failed",
                0,
                pool,
                active,
            );
            body["detail"] =
                json!(job.error.clone().unwrap_or_else(|| "Playback request failed".into()));
            (
                StatusCode::from_u16(job.error_status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                Json(body),
            )
                .into_response()
        }
    }
}

/// Cancel a playback job (upstream: DELETE /playback/requests/{request_id}).
pub async fn cancel_playback_request(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> Response {
    state.playback.prune().await;
    let pool = PlaybackQueue::pool_size(&state).await;
    let active = state.playback.inflight.load(Ordering::Relaxed);
    // Abort the worker first so a queued job never starts after cancel.
    let aborted = {
        let mut handles = state.playback.handles.lock().await;
        handles.remove(&request_id).map(|h| h.abort()).is_some()
    };
    {
        let mut jobs = state.playback.jobs.write().await;
        if let Some(job) = jobs.get_mut(&request_id) {
            if matches!(job.state, JobState::Pending | JobState::Processing) {
                job.state = JobState::Cancelled;
                job.finished_at = Some(chrono::Utc::now().timestamp());
            }
            let body = PlaybackQueue::payload(&request_id, job.state.as_str(), 0, pool, active);
            let _ = aborted;
            return Json(body).into_response();
        }
    }
    (
        StatusCode::NOT_FOUND,
        Json(json!({"detail": "Playback request not found or expired"})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::PlaybackQueue;

    #[test]
    fn payload_shape_matches_upstream() {
        let v = PlaybackQueue::payload("abc123", "pending", 2, 7, 7);
        assert_eq!(v["status"], "pending");
        assert_eq!(v["requestId"], "abc123");
        assert_eq!(v["queuePosition"], 2);
        assert_eq!(v["statusUrl"], "/playback/requests/abc123");
        assert_eq!(v["cancelUrl"], "/playback/requests/abc123");
        assert_eq!(v["playbackAccounts"], 7);
        assert_eq!(v["activePlaybackRequests"], 7);
    }

    #[test]
    fn slot_gate_allows_pool_size() {
        let q = PlaybackQueue::new();
        assert!(q.try_acquire(2));
        assert!(q.try_acquire(2));
        assert!(!q.try_acquire(2));
        q.release();
        assert!(q.try_acquire(2));
        // Pool floor of 1 always admits one.
        let q2 = PlaybackQueue::new();
        assert!(q2.try_acquire(0));
        assert!(!q2.try_acquire(0));
    }
}
