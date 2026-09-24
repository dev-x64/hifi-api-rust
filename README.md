# hifi-api-rust

Rust (Axum) API for Tidal catalog data and playback, with multiple accounts, a playback queue, a separate catalog account, and an admin panel.

This repository is a fork of [itsmeadarsh2008/hifi-api](https://github.com/itsmeadarsh2008/hifi-api). That project is a fork of [binimum/hifi-api](https://github.com/binimum/hifi-api), which is a fork of [sachinsenal0x64/hifi](https://github.com/sachinsenal0x64/hifi).

Use your own valid Tidal account. Catalog availability and playback formats depend on the account, track, and client credentials.

## Quick start

### Docker Compose

```bash
cp .env.example .env
# Set a strong ADMIN_KEY in .env.
docker compose up -d --build
docker compose ps
```

Open [http://127.0.0.1:8000/admin](http://127.0.0.1:8000/admin), sign in with `ADMIN_KEY`, and use **Add via OAuth** to add a Tidal account. You can also import an existing `credentials.json` in the admin panel. API liveness is available at `/health`.

The base Compose file binds to `127.0.0.1:8000` by default and stores SQLite data in the `hifi_data` volume. Set `HIFI_BIND` and `HIFI_PORT` in `.env` to change the host binding. `token.json` is not mounted into the container automatically.

To connect the container to a reverse proxy, create the external Docker network `proxy` once, then start with the override:

```bash
docker network create proxy
docker compose -f docker-compose.yml -f docker-compose.proxy.yml up -d --build
```

The proxy override removes the host port mapping. Set your reverse proxy's upstream to `hifi-api-rust:8000` on the `proxy` network and serve it over HTTPS. Use the base Compose command when you want the local port binding.

### Run from source

Requires the Rust toolchain. From the repository root:

```bash
cp .env.example .env
# Set ADMIN_KEY in .env.
cargo run --release
```

The server listens on `0.0.0.0:8000` by default; use `HOST` and `PORT` to change this. With no accounts configured, add one at `/admin`. To start the device authorization flow automatically on first boot, set `AUTO_SETUP=true` before starting the server. Its verification URL is printed in the logs.

You may instead provide `CLIENT_ID`, `REFRESH_TOKEN`, and optionally `CLIENT_SECRET` and `USER_ID` in `.env`. When the database has no playback accounts, these values create a default account. Keep refresh tokens and `ADMIN_KEY` out of version control.

### Changing the version

Set the version in `Cargo.toml` under `[package]`. Cargo updates the generated `Cargo.lock` when you build or run `cargo check`:

```bash
cargo check
```

Commit `Cargo.toml` and the updated `Cargo.lock` together. The API response, startup log, and admin panel read the package version from `Cargo.toml` at compile time. Docker also updates the lockfile during its build, but that copy stays inside the build container.

## Configuration

Copy [`.env.example`](.env.example) for the full list. Values saved in the admin panel can override initial environment defaults where noted.

| Variable | Default | Purpose |
| --- | --- | --- |
| `DATABASE_URL` | `hifi.db` locally; `/data/hifi.db` in Compose | SQLite file. Empty or `ephemeral` disables the database and persistence. |
| `ADMIN_KEY` | Empty | Protects the admin panel and admin API. Set a strong value before exposing the service. |
| `HOST`, `PORT` | `0.0.0.0`, `8000` | Server listener. |
| `HIFI_BIND`, `HIFI_PORT` | `127.0.0.1`, `8000` | Host binding for the base Compose file. |
| `AUTO_SETUP` | `false` | Start device authorization in the background when no account exists. |
| `CLIENT_ID`, `CLIENT_SECRET`, `REFRESH_TOKEN`, `USER_ID` | Empty | Initial playback account credentials. |
| `TOKEN_FILE` | `token.json` | Import an existing upstream credential file on startup when present; entries marked `role: "catalog"` stay out of playback. |
| `CATALOG_CLIENT_ID`, `CATALOG_CLIENT_SECRET`, `CATALOG_REFRESH_TOKEN`, `CATALOG_USER_ID` | Empty | Initial dedicated metadata account. |
| `CATALOG_TOKEN` | Empty | Static metadata bearer token; cannot refresh itself. `CATALOG_ACCESS_TOKEN` is also accepted. |
| `COUNTRY_CODE` | `US` | Default Tidal catalog region. |
| `ATMOS_MODE` | `prefer` | Default playback mode: `prefer`, `off`, or `high`. The admin setting and `?atmos=` can override it. |
| `AUTO_HEAL` | `true` | Retry accounts disabled by system errors; does not turn manually disabled accounts on. Editable in admin. |
| `USE_PROXIES`, `PROXIES_FILE` | `false`, `proxies.txt` | Initial outbound proxy setting and proxy list. Editable in admin and persisted with SQLite. |
| `FALLBACK_TO_DIRECT_CONNECTION` | `false` | Use the host connection when no proxy works. This exposes the host IP to Tidal. |
| `MAX_RETRIES`, `ROTATE_PROXIES_ON_REFRESH` | `2`, `false` | Proxy retry count and whether to rotate on token refresh. |
| `TRUST_PROXY_HEADERS` | `true` | Use `X-Forwarded-For` and `X-Real-IP` to identify clients. Enable only behind a trusted reverse proxy that replaces client-supplied forwarding headers; set `false` for direct access. |
| `DISCORD_WEBHOOK_URL` | Empty | Initial Discord webhook for account and outage alerts. Admin → System → Notifications can replace or disable it without a restart; the saved value persists in SQLite and shared Redis when enabled. The panel never returns the saved URL. |
| `UPSTASH_REDIS_REST_URL`, `UPSTASH_REDIS_REST_TOKEN` | Empty | Optional shared state across instances using Upstash REST. |
| `REDIS_POOL` | Empty | Alternative native Redis/Valkey URL. Takes precedence over the Upstash pair. |
| `RUST_LOG` | `info` | Log filter. |

`USER_AGENT` and `DEV_MODE` configure upstream HTTP requests and diagnostics. See `.env.example` for their defaults. `PUBLIC_POOL_REDIS_URL` is a deprecated fallback for `REDIS_POOL`.

### Proxies

`proxies.txt` accepts one `http://` or `https://` proxy URL per line, optionally with credentials. When proxy mode is enabled and no proxy is usable, Tidal requests fail unless `FALLBACK_TO_DIRECT_CONNECTION=true`. The admin panel can change the list and switch proxy mode without restarting the server.

### Multiple instances

Configure either the Upstash pair or `REDIS_POOL` on every instance in a fleet. Shared state includes app settings, Tidal access tokens, account credentials, and API key definitions and usage. SQLite still belongs to each instance. Playback jobs, request logs, metadata cache, and proxy settings remain local. Protect Redis credentials: the shared state contains Tidal credentials and the Discord webhook token. Database backups also contain the saved webhook.

Without Redis, each instance operates independently. If Redis becomes unavailable, the service continues with local state until sync recovers.

## Admin and API authentication

The admin panel is at `/admin`. With `ADMIN_KEY` set, sign-in creates an HttpOnly session cookie; admin API clients can use `X-Admin-Key`. Five incorrect admin keys from one IP within five minutes lock admin authentication for 15 minutes (`429` with `Retry-After`). The same limit applies when `X-Admin-Key` is used on public API routes. Missing credentials and expired session cookies are not counted. Successful authentication clears earlier failures. Lockouts are local to each instance and reset on restart. Leaving `ADMIN_KEY` empty leaves the admin panel open.

The panel manages OAuth accounts, catalog flags, credentials import/export, API keys, proxy settings, Atmos and auto-heal settings, alerts, cache, backups, and request statistics. Its interface defaults to English; choose English or Russian in System → Panel settings. The choice is saved in the current browser. Overview shows completed API requests per second averaged over the last 60 seconds and p95 response time from up to 5000 recent logged requests. These are observed per-instance metrics, not the server's maximum capacity. The existing total request and error cards count account selections and account errors. The live log records recent API requests with status, latency, client IP, endpoint counts, and top tracks; admin, health, and favicon requests are excluded. Three requests to scanner paths such as `wp-includes`, `.env`, or `.git/config` within one minute temporarily block the client IP from all routes for 15 minutes (`403` with `Retry-After`). The ban is local to each instance and resets on restart.

Public API routes are open until you create the first API key. After that, send `X-API-Key` with requests, or use `X-Admin-Key` as the owner. `/`, `/health`, and `/admin` are exempt from API key checks. API keys can have usage quotas.

## API

All routes below are registered by the Rust server. The catalog routes return JSON derived from Tidal; available fields vary with the upstream response. Paths shown with a trailing slash require that slash.

| Method and path | Query parameters | Result |
| --- | --- | --- |
| `GET /` | — | API version and repository field. |
| `GET /health` | — | Liveness and Redis connection status. |
| `GET /info/` | `id` | Track metadata. |
| `GET /search/` | One of `s` (track), `a` (artist), `al` (album), `v` (video), `p` (playlist), `i` (ISRC); optional `limit`, `offset` | Search results. Default `limit=25`, `offset=0`. |
| `GET /album/` | `id`; optional `limit`, `offset` | Album and tracks. Default `limit=100`, `offset=0`. |
| `GET /artist/` | `id` or `f`; optional `skip_tracks` | Artist details or a related artist view. |
| `GET /album/similar/`, `GET /artist/similar/` | `id`; optional `cursor` | Similar items. |
| `GET /mix/` | `id` | Mix and its tracks. |
| `GET /playlist/` | `id`; optional `offset` | Playlist and a page of items (100 per request). |
| `GET /recommendations/` | `id` | Up to 20 recommended tracks. |
| `GET /cover/` | `id` or `q` | Cover URLs. |
| `GET /lyrics/` | `id` | Track lyrics. |
| `GET /topvideos/` | Optional `countryCode`, `locale`, `deviceType`, `limit`, `offset` | Recommended videos. |
| `GET /track/{id}/{quality}`, `GET /track/{id}`, or `GET /track/?id=...` | Optional `quality` in query for older paths; `immersiveaudio` | Playback info as JSON. Default `HIGH` uses v1; other supported qualities use v2. |
| `GET /trackManifests/{id}` or `GET /trackManifests/?id=...` | Optional `formats`, `adaptive`, `manifestType`, `uriScheme`, `usage`, `countryCode`, `atmos` | Track manifest data. `formats` accepts commas or repeated parameters. |
| `GET /dash/{id}` | Optional `atmos` | Redirects to a fresh Tidal DASH manifest, or a direct AAC file when `HIGH` is selected. |
| `GET /video/` | `id`; optional `quality`, `mode`, `presentation` | Video playback info. Defaults: `HIGH`, `STREAM`, `FULL`. |
| `GET` or `POST /widevine` | Challenge in request body when required | Proxies the Widevine license request. |
| `GET`, `DELETE /playback/requests/{request_id}` | — | Poll or cancel a queued playback job. |

Examples:

```bash
curl 'http://127.0.0.1:8000/health'
curl 'http://127.0.0.1:8000/search/?s=Billie%20Jean'
curl 'http://127.0.0.1:8000/trackManifests/1781887?formats=FLAC_HIRES,FLAC&atmos=off'
curl -s -D - -o /dev/null 'http://127.0.0.1:8000/dash/1781887?atmos=off'
```

Add `-H 'X-API-Key: YOUR_KEY'` to the `curl` commands after enabling API keys.

### Playback queue and formats

`/track/`, `/trackManifests`, `/dash`, `/widevine`, and `/video/` use playback accounts. Each account handles at most one playback request at a time. If all slots are busy, the API returns `202 Accepted` with a `Location` header pointing to `/playback/requests/{request_id}` and a `Retry-After` header. Poll that URL with `GET` until it returns the original result; `DELETE` cancels the job. Finished jobs expire after five minutes. Catalog accounts do not serve playback.

`/trackManifests` accepts `formats=FLAC_HIRES,FLAC,AACLC` or repeated `formats` parameters. It also accepts `atmos=prefer`, `atmos=only`, or `atmos=off`. Explicit formats take precedence except when `atmos=prefer` adds Atmos first or `atmos=only` selects only Atmos. `/dash/{id}` supports the same Atmos preference, but uses its own fixed format list. In the admin panel, `HIGH · AAC 320 kbps` makes `/dash/{id}` redirect to a direct AAC file from v1 without calling v2; `atmos=prefer` or `atmos=only` still requests v2. `/trackManifests` always exposes the raw v2 endpoint. `atmos=off` requests non-Atmos formats for a conventional DASH player when the FLAC or Atmos preference is selected. Atmos playback requires a compatible player and may require Widevine.

`/track/{id}/{quality}` puts quality in the path, for example `/track/1781887/HIGH`. `/track/{id}` defaults to `HIGH`, and the older `?quality=` and `?id=` query forms still work. `HIGH` uses v1; `LOW`/`HEAACV1`, `AACLC`, `LOSSLESS`/`FLAC`, `HI_RES_LOSSLESS`/`FLAC_HIRES`, and `DOLBY_ATMOS`/`EAC3_JOC` each request one format through v2. Unsupported values return `200 OK` with `status=unsupported_quality` and a `supportedQualities` list, without calling Tidal. The v1 and v2 JSON responses have different shapes; see [USAGE.md](USAGE.md).

The FLAC and Atmos `/dash/{id}` response points to a DASH `.mpd` manifest (`application/dash+xml`). A browser may download that text manifest; play the URL in a DASH-capable player. The `HIGH` setting redirects to an AAC/MP4 file instead.

If every available playback account receives only a preview for a track, the API returns `503` instead of presenting the preview as a full track. Quality and format availability ultimately depend on Tidal.

For examples of extracting and playing media URLs, see [USAGE.md](USAGE.md).

## License

See [LICENSE](LICENSE).
