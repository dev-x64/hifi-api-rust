# USAGE — Getting Tidal playback URLs

These examples use a running `hifi-api` instance at `http://localhost:8000`.
The format you receive depends on the selected playback mode, the track, and
the configured Tidal account.

---

## 1. Quality tiers

Tidal serves audio in several tiers. Which one you get depends on **both** the
track's availability and your account/client entitlement:

| Tier | Format | Typical specs | Notes |
|---|---|---|---|
| `HIGH` | AAC (`mp4a.40.2`) | Up to 320 kbps | Available through the v1 playback endpoint. |
| `LOSSLESS` / `FLAC` | FLAC-in-fMP4 | 16-bit / 44.1 kHz | CD quality. |
| `FLAC_HIRES` | FLAC-in-fMP4 | 24-bit / 96–192 kHz (e.g. 176.4 kHz) | Only on tracks tagged `HIRES_LOSSLESS`; client must be entitled. |
| `EAC3_JOC` | E-AC-3 (Dolby Atmos) | 48 kHz, 5.1/7.1 bed + JOC objects | Only on tracks tagged `DOLBY_ATMOS`; client must be entitled. |

Tidal decides which requested formats are available to a given account and track.

---

## 2. Find a track ID

```bash
curl -s "http://localhost:8000/search/?s=Billie%20Jean" | \
  python3 -c "import json,sys; [print(i['id'], i['title'], '-', i['artist']['name']) for i in json.load(sys.stdin)['data']['items'][:5]]"
```

Use an ID returned by your own search in the playback examples below. Search
results may also include `mediaMetadata.tags` such as `LOSSLESS`,
`HIRES_LOSSLESS`, or `DOLBY_ATMOS`.

---

## 3. `/dash/{id}` — redirect to the selected format

`/dash/{id}` returns a **307 redirect**. Its target depends on the playback
setting in the admin panel:

| Setting | Upstream request | Redirect target |
|---|---|---|
| `HIGH · AAC 320 kbps` | v1 `playbackinfo` with `audioquality=HIGH` | Direct AAC/MP4 URL. v2 is not called. |
| FLAC priority | v2 `trackManifests` | DASH `.mpd` URL, with FLAC formats first. |
| Atmos priority | v2 `trackManifests` | DASH `.mpd` URL, with Atmos first. |

Inspect the redirect without downloading the target:

```bash
curl -sS -D - -o /dev/null "http://localhost:8000/dash/1781887"
```

In FLAC or Atmos mode, the target has an `.mpd` extension and the content type
`application/dash+xml`. It is a text manifest, so opening `/dash/{id}` in a
browser may download a text file. Play this URL with a DASH-capable player:

```bash
mpv "http://localhost:8000/dash/1781887"
```

Select `HIGH` in the admin panel to get a direct AAC/MP4 URL instead. With
`HIGH` selected, `?atmos=prefer` or `?atmos=only` explicitly switches this
request to v2; `?atmos=off` keeps the `HIGH` default. `/trackManifests/{id}`
is always the raw v2 endpoint.

---

## 4. `/trackManifests/{id}` — raw v2 JSON

The underlying v2 endpoint, exposed directly. Useful when you need the full JSON
(URI, hash, DRM data, normalization) rather than a redirect:

```bash
curl -s "http://localhost:8000/trackManifests/1781887?formats=FLAC_HIRES,FLAC,AACLC&atmos=off" | \
  python3 -m json.tool
```

Query parameters (all optional):

| Param | Default | Notes |
|---|---|---|
| `formats` | `HEAACV1,AACLC,FLAC,FLAC_HIRES,EAC3_JOC` before Atmos preference | Comma-separated or repeated. |
| `atmos` | Admin preference | `prefer` puts `EAC3_JOC` first; `only` requests only Atmos; `off` excludes Atmos from the default list. |
| `adaptive` | `true` | Multi-format response. |
| `manifestType` | `MPEG_DASH` | `MPEG_DASH` or `HLS`. |
| `uriScheme` | `HTTPS` | `HTTPS` = manifest link, `DATA` = inline base64. |
| `usage` | `PLAYBACK` | `PLAYBACK` or `DOWNLOAD`. |
| `countryCode` | Server's configured country code | Overrides the server default for this request. |

Explicit `formats` are kept as given unless `atmos=prefer` adds Atmos first or
`atmos=only` replaces the list. `atmos=off` does not remove Atmos from an
explicit format list.

Response shape:

```json
{
  "version": "2.10",
  "data": {
    "data": {
      "id": "1781887",
      "type": "trackManifests",
      "attributes": {
        "trackPresentation": "FULL",
        "uri": "https://im-fa.manifest.tidal.com/1/manifests/...mpd?token=...",
        "hash": "...",
        "formats": ["FLAC_HIRES", "FLAC", "AACLC"]
      }
    }
  }
}
```

---

## 5. `/track/{id}/{quality}` — quality selection

Put the quality in the path, for example `/track/1781887/HIGH` or
`/track/1781887/LOSSLESS`. `/track/{id}` defaults to `HIGH`. The older
`/track/{id}?quality=...` and `/track/?id=...&quality=...` forms still work.
`HIGH` calls v1 `playbackinfo`; other supported qualities call v2
`trackManifests` with only the corresponding format. These routes return JSON,
with different v1 and v2 shapes, and do not redirect to audio.

| `quality` | Upstream | Requested format |
|---|---|---|
| `HIGH` (default) | v1 | AAC, up to 320 kbps |
| `LOW`, `HEAACV1` | v2 | `HEAACV1` |
| `AACLC` | v2 | `AACLC` |
| `LOSSLESS`, `FLAC` | v2 | `FLAC` |
| `HI_RES_LOSSLESS`, `FLAC_HIRES` | v2 | `FLAC_HIRES` |
| `DOLBY_ATMOS`, `ATMOS`, `EAC3_JOC` | v2 | `EAC3_JOC` |

Unsupported quality values return `200 OK` with `status=unsupported_quality`,
the requested value, and a `supportedQualities` list (names, aliases, formats,
and v1/v2 source). This response does not call Tidal, so it does not claim
which formats are available for that track. Quality names are case-insensitive.
`immersiveaudio` applies only to the v1 `HIGH` request.
If both path and query contain a quality, the path value is used.

```bash
curl -s "http://localhost:8000/track/1781887/WAV" | python3 -m json.tool
```

For `HIGH`, decode the v1 base64 manifest to get its direct audio URL:

```bash
curl -s "http://localhost:8000/track/1781887/HIGH" | \
  python3 -c "
import json, sys, base64
d = json.load(sys.stdin)['data']
m = json.loads(base64.b64decode(d['manifest']))
print(d['audioQuality'])   # quality actually returned by Tidal
print(m['urls'][0])        # direct audio URL
"

# HIGH is also the default; legacy query form gives the same result
curl -s "http://localhost:8000/track/1781887" | python3 -m json.tool
curl -s "http://localhost:8000/track/?id=1781887" | python3 -m json.tool
```

For lossless, the v2 response instead contains a DASH manifest URI:

```bash
curl -s "http://localhost:8000/track/1781887/LOSSLESS" | \
  python3 -c "import json,sys; print(json.load(sys.stdin)['data']['data']['attributes']['uri'])"
```

You can use `/track/1781887/HI_RES_LOSSLESS` or
`/track/1781887/DOLBY_ATMOS` for their respective v2 formats. Tidal may reject
a format that is unavailable for the track or account.

If all playback slots are busy, these endpoints can return `202 Accepted` with
a `Location: /playback/requests/{request_id}` header. Poll that URL until it
returns the result before parsing the JSON or following a redirect.

---

## 6. Known limitations

- **Availability:** The track and account determine which formats Tidal returns.
  A requested format may be unavailable or rejected.
- **Expiring URLs:** Request a fresh `/dash/{id}`, `/trackManifests/{id}`, or
  `/track/{id}` response before playback. Do not cache Tidal redirect targets
  or direct audio URLs indefinitely.
- **Atmos and DRM:** Atmos requires a compatible player. A manifest with
  `drmData` requires a DRM-capable player and a license request through
  `/widevine`.

---

## 7. Quick reference

```bash
# Play using the format selected in the admin panel
mpv "http://localhost:8000/dash/1781887"

# HIGH via v1 (explicit path, default path, and legacy query forms)
curl -s "http://localhost:8000/track/1781887/HIGH"
curl -s "http://localhost:8000/track/1781887"
curl -s "http://localhost:8000/track/?id=1781887"

# FLAC via v2
curl -s "http://localhost:8000/track/1781887/LOSSLESS"

# Get the raw v2 manifest JSON
curl -s "http://localhost:8000/trackManifests/1781887"
```

If API keys are enabled, add `-H 'X-API-Key: YOUR_KEY'` to the `curl` commands.
