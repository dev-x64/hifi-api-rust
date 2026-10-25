use axum::response::Html;
use std::sync::OnceLock;

pub async fn admin_index() -> Html<&'static str> {
    static HTML: OnceLock<String> = OnceLock::new();
    Html(HTML.get_or_init(|| {
        ADMIN_HTML
            .replace("__HIFI_VERSION__", env!("CARGO_PKG_VERSION"))
            .replace("__HIFI_I18N__", include_str!("i18n.js"))
    }))
}

const ADMIN_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>HiFi API — управление</title>
<style>
* { margin:0; padding:0; box-sizing:border-box; }
body { font-family:'SF Mono','Fira Code','Cascadia Code','JetBrains Mono',Menlo,Monaco,Consolas,monospace; background:#0d1117; color:#c9d1d9; padding:20px; }
.container { max-width:1000px; margin:0 auto; padding:0 8px; }

.header { display:flex; align-items:center; justify-content:space-between; margin-bottom:24px; flex-wrap:wrap; gap:12px; }
.header h1 { font-size:22px; color:#f0f6fc; letter-spacing:-0.3px; }
.header .badge { font-size:11px; background:#1f6feb; color:#fff; padding:3px 10px; border-radius:10px; font-weight:500; }

.stat-card { background:#161b22; border:1px solid #30363d; border-radius:10px; padding:18px 20px; transition:border-color 0.2s; }
.stat-card:hover { border-color:#484f58; }
.stat-card .label { font-size:11px; color:#8b949e; text-transform:uppercase; letter-spacing:0.5px; }
.stat-card .value { font-size:26px; font-weight:700; margin-top:4px; color:#f0f6fc; letter-spacing:-0.5px; }

.accounts-grid { display:flex; flex-direction:column; gap:20px; margin-bottom:24px; }
.account-card { background:#161b22; border:1px solid #30363d; border-radius:10px; overflow:hidden; transition:border-color 0.2s, box-shadow 0.2s; }
.account-card:hover { border-color:#484f58; box-shadow:0 4px 24px rgba(0,0,0,0.3); }

.card-header { display:flex; align-items:center; justify-content:space-between; padding:14px 20px; background:#1c2128; border-bottom:1px solid #30363d; flex-wrap:wrap; gap:10px; }
.card-header .left { display:flex; align-items:center; gap:10px; min-width:0; flex-wrap:wrap; }
.acc-num { display:inline-flex; align-items:center; justify-content:center; min-width:22px; height:22px; padding:0 6px; border-radius:11px; background:#21262d; border:1px solid #30363d; color:#8b949e; font-size:11px; font-weight:700; flex-shrink:0; }
.card-header .label { font-weight:600; font-size:14px; color:#f0f6fc; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }

.card-body { padding:18px 20px; }
.cred-row { display:flex; align-items:baseline; gap:8px; padding:8px 0; font-size:12px; }
.cred-row:last-child { padding-bottom:0; }
.cred-key { color:#8b949e; min-width:120px; user-select:none; flex-shrink:0; }
.cred-key::after { content:'='; margin-left:4px; color:#30363d; }
.cred-value { color:#c9d1d9; word-break:break-all; min-width:0; }
.cred-value.masked { color:#58a6ff; }
.cred-value.token { color:#d2a8ff; font-size:11px; }

.card-footer { display:flex; align-items:center; justify-content:space-between; padding:12px 20px; border-top:1px solid #30363d; background:#12161c; flex-wrap:wrap; gap:10px; }
.card-stats { display:flex; gap:14px; flex-wrap:wrap; }
.card-stat { font-size:11px; color:#8b949e; white-space:nowrap; }
.card-stat strong { color:#c9d1d9; }
.account-uptime { padding:16px 20px; border-top:1px solid #30363d; }
.uptime-heading { display:flex; align-items:center; flex-wrap:wrap; gap:8px 12px; margin-bottom:12px; font-size:12px; color:var(--muted); }
.uptime-heading strong { color:var(--text); font-weight:600; }
.uptime-tabs { display:inline-flex; gap:2px; padding:2px; border:1px solid var(--line); border-radius:7px; background:#0c1117; }
.uptime-tabs button { padding:3px 9px; border:0; border-radius:5px; background:transparent; color:var(--muted); font:inherit; font-size:11px; cursor:pointer; }
.uptime-tabs button:hover { color:var(--text); }
.uptime-tabs button[aria-pressed="true"] { background:#222a35; color:var(--text); }
.uptime-current { padding:2px 8px; border-radius:999px; background:rgba(244,184,96,.12); color:var(--amber); font-size:11px; }
.uptime-score { margin-left:auto; }
.uptime-score strong { margin-left:6px; font-size:15px; font-variant-numeric:tabular-nums; }
.uptime-score.good strong { color:var(--green); }
.uptime-score.warn strong { color:var(--amber); }
.uptime-score.bad strong { color:var(--red); }
.uptime-track { position:relative; height:28px; cursor:crosshair; }
.uptime-bars { position:absolute; inset:0; overflow:hidden; border-radius:6px; background:var(--panel-2); }
.uptime-seg { position:absolute; top:0; bottom:0; }
.uptime-bars .uptime-waiting, .uptime-bars .uptime-down { min-width:2px; }
.uptime-grid { position:absolute; top:0; bottom:0; width:1px; background:rgba(10,13,18,.75); pointer-events:none; }
.uptime-up { background:#2fa36b; }
.uptime-waiting { background:#d9a03f; }
.uptime-down { background:var(--red); }
.uptime-unknown { background:repeating-linear-gradient(135deg,transparent,transparent 4px,rgba(139,150,165,.2) 4px,rgba(139,150,165,.2) 8px); }
.uptime-cursor { position:absolute; top:-3px; bottom:-3px; width:2px; margin-left:-1px; border-radius:1px; background:var(--text); opacity:0; pointer-events:none; }
.uptime-tip { position:absolute; bottom:calc(100% + 10px); z-index:20; display:grid; gap:2px; min-width:170px; padding:8px 10px; transform:translateX(-50%); border:1px solid var(--line); border-radius:8px; background:var(--bg); box-shadow:0 8px 24px rgba(0,0,0,.45); color:var(--muted); font-size:11px; line-height:1.5; white-space:nowrap; pointer-events:none; opacity:0; visibility:hidden; }
.uptime-tip b { color:var(--text); font-weight:600; }
.uptime-tip b::before { content:''; display:inline-block; width:8px; height:8px; margin-right:6px; border-radius:2px; background:var(--muted); }
.uptime-tip .uptime-tip-up::before { background:#2fa36b; }
.uptime-tip .uptime-tip-waiting::before { background:#d9a03f; }
.uptime-tip .uptime-tip-down::before { background:var(--red); }
.uptime-tip[data-edge="start"] { transform:translateX(-12px); }
.uptime-tip[data-edge="end"] { transform:translateX(calc(-100% + 12px)); }
.uptime-track.hover .uptime-cursor, .uptime-track.hover .uptime-tip { opacity:1; visibility:visible; }
.uptime-labels { position:relative; height:16px; margin-top:6px; font-size:10px; color:var(--muted); }
.uptime-labels span { position:absolute; top:0; transform:translateX(-50%); white-space:nowrap; }
.uptime-totals { display:flex; flex-wrap:wrap; gap:6px 18px; margin-top:8px; color:var(--muted); font-size:11px; }
.uptime-totals span { display:inline-flex; align-items:center; gap:6px; }
.uptime-totals strong { color:var(--text); font-weight:600; }
.uptime-key { display:inline-block; width:8px; height:8px; border-radius:2px; }
.uptime-key.uptime-unknown { box-shadow:inset 0 0 0 1px var(--muted); }
.rate-bar::after { content:attr(data-tip); position:absolute; bottom:calc(100% + 8px); left:50%; z-index:20; transform:translateX(-50%); padding:8px 10px; border:1px solid var(--line); border-radius:8px; background:var(--bg); box-shadow:0 8px 24px rgba(0,0,0,.45); color:var(--text); font-size:11px; line-height:1.6; white-space:pre; pointer-events:none; opacity:0; visibility:hidden; transition:opacity .12s; }
.rate-bar:hover::after { opacity:1; visibility:visible; }
.rate-bar[data-edge="start"]::after { left:0; transform:none; }
.rate-bar[data-edge="end"]::after { left:auto; right:0; transform:none; }
.uptime-axis { display:flex; justify-content:space-between; margin-top:6px; font-size:10px; color:var(--muted); }
.sub-ok { color:var(--green); }
.sub-bad { color:var(--amber); }
.sub-checked { color:var(--muted); font-size:11px; }
.token-refresh-error { margin-top:12px; font-size:12px; line-height:1.5; color:#d29922; overflow-wrap:anywhere; }
.uptime-note { margin-top:8px; font-size:10px; line-height:1.5; color:var(--muted); }
.ready-history:empty { display:none; }
.ready-history { margin-bottom:28px; }
.ready-history-head h2 { font-size:17px; letter-spacing:-.25px; }
.ready-history-head p { color:var(--muted); font-size:12px; margin:5px 0 14px; line-height:1.45; }
.ready-history-head strong { color:var(--text); }
.ready-days { display:grid; grid-template-columns:repeat(3,minmax(0,1fr)); gap:12px; }
.ready-day { padding:16px 18px; border:1px solid var(--line); border-radius:12px; background:linear-gradient(145deg,#151a22,#11161d); }
.ready-day-head { display:flex; justify-content:space-between; gap:8px; font-size:12px; color:var(--muted); }
.ready-day-head span:first-child { color:var(--text); font-weight:650; }
.ready-nums { display:grid; grid-template-columns:repeat(3,1fr); gap:8px; margin-top:12px; }
.ready-nums small { display:block; color:var(--muted); font-size:11px; }
.ready-nums strong { display:block; margin-top:2px; color:var(--text); font-size:24px; font-weight:700; letter-spacing:-.5px; font-variant-numeric:tabular-nums; }
.ready-nums .ready-min { color:var(--amber); }
.ready-nums .ready-max { color:var(--green); }
.ready-spark { display:block; width:100%; height:40px; margin-top:12px; }
.ready-grid line { stroke:var(--line); stroke-width:1; vector-effect:non-scaling-stroke; }
.ready-area { fill:rgba(72,199,142,.14); }
.ready-line { fill:none; stroke:var(--green); stroke-width:1.5; vector-effect:non-scaling-stroke; }
.ready-foot { min-height:16px; margin-top:8px; color:var(--muted); font-size:11px; }
.ready-empty { margin-top:14px; color:var(--muted); font-size:12px; }
@media (max-width:760px) { .ready-days { grid-template-columns:1fr; } .uptime-score { margin-left:0; width:100%; } }
.test-badge { cursor:pointer; text-decoration:underline; text-decoration-style:dotted; text-underline-offset:2px; }
.test-badge:hover { color:#f0f6fc; }

.card-actions { display:flex; gap:6px; flex-wrap:wrap; }
.status-dot { display:inline-block; width:10px; height:10px; border-radius:50%; flex-shrink:0; }
.status-ok { background:#3fb950; box-shadow:0 0 6px rgba(63,185,80,0.3); }
.status-warn { background:#d29922; box-shadow:0 0 6px rgba(210,153,34,0.3); }
.status-err { background:#f85149; box-shadow:0 0 6px rgba(248,81,73,0.3); }

.btn { background:#21262d; border:1px solid #30363d; color:#c9d1d9; padding:7px 14px; border-radius:6px; cursor:pointer; font-size:12px; font-weight:500; transition:all 0.15s; }
.btn:hover { background:#30363d; border-color:#484f58; transform:translateY(-1px); }
.btn:active { transform:translateY(0); }
.btn-primary { background:#238636; border-color:rgba(35,134,54,0.5); color:#fff; }
.btn-primary:hover { background:#2ea043; border-color:#2ea043; }
.btn-danger { border-color:rgba(248,81,73,0.4); color:#f85149; }
.btn-danger:hover { background:#f85149; border-color:#f85149; color:#fff; }
.btn-active { background:#1f6feb; border-color:rgba(31,111,235,0.5); color:#fff; }

.form-section { background:#161b22; border:1px solid #30363d; border-radius:10px; padding:28px; margin-bottom:24px; transition:border-color 0.2s; }
.form-section:hover { border-color:#484f58; }
.form-section h3 { font-size:16px; margin-bottom:20px; color:#f0f6fc; }
.form-row { display:grid; grid-template-columns:1fr 1fr; gap:14px; margin-bottom:16px; }
.form-row.full { grid-template-columns:1fr; }
.form-group label { display:block; font-size:11px; color:#8b949e; margin-bottom:4px; font-weight:500; text-transform:uppercase; letter-spacing:0.3px; }
.form-group input { width:100%; background:#0d1117; border:1px solid #30363d; color:#c9d1d9; padding:10px 12px; border-radius:6px; font-size:13px; transition:border-color 0.15s; }
.form-group input:focus { outline:none; border-color:#1f6feb; box-shadow:0 0 0 3px rgba(31,111,235,0.15); }
.pw-wrap { position:relative; }
.pw-wrap input { padding-right:38px; }
.pw-toggle { position:absolute; right:6px; top:50%; transform:translateY(-50%); background:none; border:none; color:#8b949e; cursor:pointer; font-size:15px; padding:4px 6px; line-height:1; }
.pw-toggle:hover { color:#f0f6fc; }

.error { color:#f85149; font-size:13px; margin-bottom:10px; padding:8px 12px; background:rgba(248,81,73,0.08); border:1px solid rgba(248,81,73,0.2); border-radius:6px; }
.success { color:#3fb950; font-size:13px; margin-bottom:10px; padding:8px 12px; background:rgba(63,185,80,0.08); border:1px solid rgba(63,185,80,0.2); border-radius:6px; }
.error:empty, .success:empty { display:none; padding:0; margin:0; border:none; }

.overlay { display:none; position:fixed; inset:0; background:rgba(0,0,0,0.65); z-index:100; backdrop-filter:blur(6px); -webkit-backdrop-filter:blur(6px); }
.overlay.open { display:flex; align-items:center; justify-content:center; }
.modal { background:#161b22; border:1px solid #30363d; border-radius:12px; padding:24px; width:520px; max-width:92vw; max-height:90vh; overflow-y:auto; scrollbar-width:none; animation:modalIn 0.2s ease; }
.modal::-webkit-scrollbar { display:none; }
@keyframes modalIn { from { opacity:0; transform:scale(0.95) translateY(8px); } to { opacity:1; transform:scale(1) translateY(0); } }
.modal h3 { font-size:17px; margin-bottom:20px; color:#f0f6fc; }
.modal .form-group { margin-bottom:16px; }
.modal .modal-actions { display:flex; gap:10px; margin-top:18px; }
.mode-tabs { display:inline-flex; gap:2px; padding:3px; margin:2px 0 14px; border:1px solid var(--line); border-radius:9px; background:#0c1117; }
.mode-tabs button { min-height:30px; padding:5px 14px; border:0; border-radius:6px; background:transparent; color:var(--muted); font-size:12px; font-weight:650; cursor:pointer; }
.mode-tabs button:hover { color:var(--text); }
.mode-tabs button[aria-selected="true"] { background:#222a35; color:var(--text); }
[data-mode="oauth"] .manual-only, [data-mode="manual"] .oauth-only { display:none !important; }
[data-mode="oauth"] .add-account .form-row { grid-template-columns:1fr; margin-bottom:0; }
.modal .modal-actions .btn { padding:8px 20px; font-size:13px; }

.empty-state { text-align:center; padding:48px 20px; color:#8b949e; }
.empty-state p { font-size:15px; margin-bottom:6px; }
.empty-state .hint { font-size:13px; }

.test-pass { color:#3fb950; }
.test-fail { color:#f85149; }
.test-pending { color:#d29922; }

.status-label { font-size:11px; margin-left:6px; padding:2px 8px; border-radius:4px; font-weight:500; }
.status-label.status-ok { color:#3fb950; background:rgba(63,185,80,0.1); }
.status-label.status-err { color:#f85149; background:rgba(248,81,73,0.1); }
.status-label.status-warn { color:#d29922; background:rgba(210,153,34,0.1); }
.status-unknown { background:#8b949e; }
.card-stat.test-fail { white-space:normal; overflow-wrap:anywhere; }
.playback-access { display:inline-flex; align-items:center; gap:7px; min-height:28px; margin-left:4px; padding:4px 9px; border:1px solid; border-radius:7px; font-family:Inter,sans-serif; line-height:1; white-space:nowrap; }
.playback-access .access-caption { font-size:9px; font-weight:700; letter-spacing:.55px; text-transform:uppercase; opacity:.72; }
.playback-access strong { font-size:12px; letter-spacing:.25px; }
.playback-access.access-full { color:#64df9b; background:rgba(46,160,103,.13); border-color:rgba(65,204,130,.38); box-shadow:inset 0 0 18px rgba(46,160,103,.04); }
.playback-access.access-preview { color:#ffc766; background:rgba(210,153,34,.13); border-color:rgba(240,173,78,.4); box-shadow:inset 0 0 18px rgba(210,153,34,.04); }
.playback-access.access-unknown { color:#9ba8b7; background:rgba(139,148,158,.08); border-color:rgba(139,148,158,.24); }
.playback-access.access-catalog { color:#d2a8ff; background:rgba(210,168,255,.08); border-color:rgba(210,168,255,.25); }
.access-check-time strong { font-weight:650; }

.test-results-section { background:#161b22; border:1px solid #30363d; border-radius:10px; margin:24px 0; overflow:hidden; transition:border-color 0.2s, box-shadow 0.2s; }
.test-results-section:hover { border-color:#484f58; box-shadow:0 4px 24px rgba(0,0,0,0.3); }
.test-results-section .test-results-header { display:flex; align-items:center; justify-content:space-between; padding:14px 20px; background:#1c2128; border-bottom:1px solid #30363d; }
.test-results-section .test-results-header h3 { font-size:14px; color:#f0f6fc; font-weight:600; }
.test-results-section .test-results-header .test-summary { font-size:11px; color:#8b949e; }
.test-results-section .test-results-body { padding:4px; }
.test-result-row { display:flex; align-items:center; gap:10px; padding:10px 16px; border-bottom:1px solid #21262d; cursor:pointer; transition:background 0.12s; font-size:12px; }
.test-result-row:hover { background:#1c2128; }
.test-result-row:last-child { border-bottom:none; }
.test-result-row .result-label { flex:1; color:#c9d1d9; font-weight:500; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.test-result-row .result-status { min-width:44px; font-weight:600; }
.test-result-row .result-http { min-width:34px; color:#8b949e; }
.test-result-row .result-ms { min-width:60px; color:#8b949e; text-align:right; }
.test-result-row .result-token { min-width:90px; color:#8b949e; font-size:11px; }

.json-key { color:#79c0ff; }
.json-string { color:#a5d6ff; }
.json-number { color:#79c0ff; }
.json-boolean { color:#ff7b72; }
.json-null { color:#ff7b72; }
.json-bracket { color:#c9d1d9; }

@media (max-width:768px) {
  body { padding:12px; }
  .card-header { flex-direction:column; align-items:stretch; }
  .card-footer { flex-direction:column; align-items:stretch; gap:12px; }
  .card-stats { gap:10px; }
  .cred-row { flex-direction:column; gap:2px; padding:6px 0; }
  .cred-key { min-width:0; }
  .cred-key::after { content:':'; }
  .form-row { grid-template-columns:1fr; gap:14px; }
  .form-section { padding:20px; }
  .header h1 { font-size:18px; }
  .test-results-section { padding:16px; overflow-x:auto; }
  .test-result-row { min-width:500px; }
}

@media (max-width:480px) {
  .modal { padding:20px; max-width:96vw; }
}

@keyframes highlightPulse { 0%,100% { border-color:#30363d; } 50% { border-color:#58a6ff; box-shadow:0 0 20px rgba(88,166,255,0.15); } }
.form-highlight { animation:highlightPulse 1.5s ease; }

.terminal { background:#050805; border:1px solid #1d3a24; border-radius:10px; overflow:hidden; margin-bottom:24px; box-shadow:0 0 24px rgba(63,185,80,0.07); }
.term-bar { display:flex; align-items:center; gap:10px; padding:9px 14px; background:#0b120c; border-bottom:1px solid #1d3a24; }
.term-dots { display:flex; gap:6px; }
.term-dots i { width:10px; height:10px; border-radius:50%; background:#2a3b2d; }
.term-dots i:nth-child(1) { background:#f85149; }
.term-dots i:nth-child(2) { background:#d29922; }
.term-dots i:nth-child(3) { background:#3fb950; }
.term-title { font-size:11px; color:#7d8a7e; font-family:monospace; flex:1; }
.term-live { font-size:10px; color:#3fb950; font-family:monospace; letter-spacing:1px; animation:termBlink 2s infinite; }
@keyframes termBlink { 0%,100% { opacity:1; } 50% { opacity:0.35; } }
.term-meta { display:flex; gap:16px; flex-wrap:wrap; padding:9px 14px; border-bottom:1px solid #142114; font-family:monospace; font-size:11px; color:#5f6f60; }
.term-meta strong { color:#9fe8b4; font-weight:600; }
.term-meta .card-stat { font-size:11px; }
.term-body { font-family:'SF Mono','Fira Code',Menlo,Consolas,monospace; font-size:12px; line-height:1.75; padding:12px 14px; height:400px; overflow-y:auto; color:#c9e8d2; scrollbar-width:thin; scrollbar-color:#1d3a24 transparent; }
.term-body::-webkit-scrollbar { width:8px; }
.term-body::-webkit-scrollbar-thumb { background:#1d3a24; border-radius:4px; }
.term-line { white-space:nowrap; }
.term-time { color:#4a5a4c; }
.term-method { font-weight:700; }
.m-GET { color:#3fb950; }
.m-POST { color:#58a6ff; }
.m-PUT { color:#d29922; }
.m-PATCH { color:#d2a8ff; }
.m-DELETE { color:#f85149; }
.term-path { color:#e6f5ea; }
.term-id { color:#d2a8ff; }
.term-dim { color:#5f6f60; }
.term-extra { display:none; }
.terminal.show-extra .term-extra { display:inline; }
.term-account { color:#79c0ff; }
.errors-panel > summary { cursor:pointer; list-style:none; user-select:none; }
.errors-panel > summary::-webkit-details-marker { display:none; }
.errors-panel > summary::before { content:'▸'; color:#7d8a7e; font-size:11px; }
.errors-panel[open] > summary::before { content:'▾'; }
.errors-panel:not([open]) > summary { border-bottom:0; }
.errors-panel .term-title { font-size:12px; color:#c9e8d2; }
.errors-panel .term-body { height:auto; max-height:520px; padding-top:4px; }
.err-count { font-family:monospace; font-size:11px; color:#f85149; }
.err-item { padding:9px 0; border-bottom:1px solid #142114; overflow-wrap:anywhere; }
.err-item:last-child { border-bottom:0; }
.err-item .term-line { white-space:normal; }
.err-cause strong { color:#e3b341; font-weight:600; }
.err-cause span { color:#9aa79b; }
.term-cursor { display:inline-block; width:8px; height:14px; background:#3fb950; vertical-align:-2px; animation:termBlink 1.1s infinite; }
@media (max-width:768px) { .term-body { height:300px; font-size:11px; } }

/* Application shell */
:root {
  color-scheme:dark;
  --bg:#0a0d12; --panel:#11161d; --panel-2:#171d26; --line:#252d38;
  --text:#f3f5f7; --muted:#8b96a5; --accent:#7c6cff; --accent-2:#9a8fff;
  --green:#48c78e; --red:#ff6b6b; --amber:#f4b860; --sidebar:248px;
}
body { padding:0; min-height:100vh; background:var(--bg); color:var(--text); font-family:Inter,ui-sans-serif,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif; }
button,input,select { font:inherit; }
button:focus-visible,input:focus-visible,select:focus-visible { outline:2px solid var(--accent-2); outline-offset:2px; }
.hidden { display:none !important; }
.app-shell { min-height:100vh; }
.sidebar { position:fixed; inset:0 auto 0 0; width:var(--sidebar); z-index:40; padding:22px 14px 16px; display:flex; flex-direction:column; background:#0e1218; border-right:1px solid var(--line); }
.brand { display:flex; align-items:center; gap:12px; padding:0 10px 24px; }
.brand-mark { width:36px; height:36px; display:grid; place-items:center; border-radius:11px; color:white; font-weight:800; font-size:13px; background:linear-gradient(145deg,var(--accent),#5546db); box-shadow:0 10px 28px rgba(124,108,255,.25); }
.brand strong { display:block; font-size:14px; letter-spacing:-.1px; }
.brand small { display:block; color:var(--muted); margin-top:2px; font-size:11px; }
.nav-label { color:#596473; font-size:10px; font-weight:700; text-transform:uppercase; letter-spacing:1px; padding:8px 12px; }
.nav { display:flex; flex-direction:column; gap:4px; }
.nav-btn { width:100%; display:flex; align-items:center; gap:11px; border:0; border-radius:9px; padding:10px 12px; color:#9ba5b2; background:transparent; cursor:pointer; text-align:left; font-size:13px; transition:.16s ease; }
.nav-btn svg { width:17px; height:17px; flex:none; }
.nav-btn:hover { color:#fff; background:#171d25; }
.nav-btn.active { color:#fff; background:#211e39; box-shadow:inset 3px 0 0 var(--accent); }
.sidebar-bottom { margin-top:auto; border-top:1px solid var(--line); padding:14px 8px 0; }
.admin-user { display:flex; align-items:center; gap:9px; margin-bottom:10px; color:#c9d0d8; font-size:12px; }
.admin-avatar { width:28px; height:28px; display:grid; place-items:center; border-radius:50%; background:#242c37; color:#b9c2ce; font-size:11px; }
.logout-btn { width:100%; border:1px solid var(--line); background:transparent; color:var(--muted); border-radius:8px; padding:8px 10px; cursor:pointer; text-align:left; font-size:12px; }
.logout-btn:hover { color:#fff; border-color:#3a4553; }
.workspace { margin-left:var(--sidebar); min-height:100vh; padding:34px 40px 64px; }
.container { max-width:1180px; margin:0 auto; padding:0; }
.header { margin-bottom:28px; align-items:flex-start; }
.header h1 { font-size:28px; letter-spacing:-.8px; line-height:1.15; }
.page-kicker { color:var(--accent-2); font-size:11px; font-weight:700; text-transform:uppercase; letter-spacing:1.1px; margin-bottom:7px; }
.page-subtitle { color:var(--muted); font-size:13px; margin-top:8px; max-width:620px; line-height:1.5; }
.header-actions { display:flex; gap:9px; align-items:center; }
.view { display:none; animation:viewIn .22s ease; }
.view.active { display:block; }
@keyframes viewIn { from { opacity:0; transform:translateY(5px); } to { opacity:1; transform:none; } }
.section-head { display:flex; align-items:flex-end; justify-content:space-between; gap:16px; margin:0 0 16px; }
.section-head h2 { font-size:17px; letter-spacing:-.25px; }
.section-head p { color:var(--muted); font-size:12px; margin-top:5px; line-height:1.45; }
.section-actions { display:flex; gap:8px; flex-wrap:wrap; }
.overview { display:grid; gap:24px; margin-bottom:30px; }
.ov-title { margin-bottom:10px; color:#c5ccd6; font-size:13px; font-weight:650; }
.ov-grid { display:grid; gap:12px; }
.ov-4 { grid-template-columns:repeat(4,minmax(0,1fr)); }
.ov-3 { grid-template-columns:repeat(3,minmax(0,1fr)); }
.ov-2 { grid-template-columns:repeat(2,minmax(0,1fr)); }
.stat-card { border-color:var(--line); background:linear-gradient(145deg,#151a22,#11161d); border-radius:12px; padding:18px; min-height:100px; }
.stat-card:hover { border-color:#394452; }
.stat-card .label { color:var(--muted); font-family:Inter,sans-serif; font-size:12px; font-weight:600; text-transform:none; letter-spacing:0; }
.stat-card .value { font-family:Inter,sans-serif; color:var(--text); font-size:26px; margin-top:8px; font-variant-numeric:tabular-nums; }
.metric-unit { font-size:14px; color:var(--muted); font-weight:500; letter-spacing:0; }
.metric-text { display:inline-flex; align-items:center; gap:9px; font-size:17px; letter-spacing:-.2px; }
.stat-details { display:grid; gap:5px; margin-top:10px; font-size:12px; line-height:1.5; color:var(--muted); overflow-wrap:anywhere; }
.stat-details:empty { display:none; }
.stat-details strong { color:var(--text); font-weight:600; }
.stat-bar { display:flex; gap:2px; height:8px; margin-top:14px; border-radius:4px; overflow:hidden; }
.stat-bar i { display:block; flex:1 1 0; min-width:4px; }
.stat-meter { background:#222a35; }
.stat-meter i { flex:none; min-width:0; border-radius:4px; background:var(--accent-2); }
.stat-rows { display:grid; gap:7px; margin-top:12px; font-size:12px; line-height:1.4; color:#aab4c0; }
.stat-row { display:flex; align-items:center; gap:8px; }
.stat-row strong { margin-left:auto; padding-left:12px; color:var(--text); font-weight:600; font-variant-numeric:tabular-nums; white-space:nowrap; }
.rate-rows { grid-template-columns:minmax(0,1fr) auto auto auto; column-gap:16px; align-items:baseline; }
.rate-rows > span { min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.rate-rows strong,.rate-rows .rate-head { text-align:right; white-space:nowrap; font-variant-numeric:tabular-nums; }
.rate-rows strong { color:var(--text); font-weight:600; }
.rate-rows .rate-head { color:var(--muted); font-size:11px; }
.stat-wide { grid-column:span 2; }
.stat-tall { grid-row:span 2; }
.rate-chart { display:flex; align-items:flex-end; gap:2px; height:48px; margin-top:14px; border-bottom:1px solid var(--line); }
.rate-bar { position:relative; display:flex; flex:1; flex-direction:column-reverse; min-width:0; height:100%; }
.rate-bar i { display:block; flex:none; min-height:2px; }
.rate-bar i + i { box-sizing:border-box; border-bottom:1px solid var(--panel); }
.rate-bar i:last-child { border-radius:2px 2px 0 0; }
.rate-bar:hover i { filter:brightness(1.3); }
.rate-playback { background:#7c6cff; }
.rate-search { background:#2aa3b3; }
.rate-catalog { background:#c9822f; }
.rate-cached { box-shadow:inset 0 0 0 1px var(--muted); }
.rate-key { display:inline-block; width:8px; height:8px; margin-right:8px; border-radius:2px; }
.rate-unit { color:var(--muted); font-weight:500; }
.rate-cols { display:flex; flex-wrap:wrap; gap:0 32px; align-items:flex-start; }
.rate-cols > * { flex:1 1 200px; min-width:0; }
.rate-cols > .rate-rows { flex-basis:270px; }
.stat-card .status-dot { width:8px; height:8px; box-shadow:none; }
.stat-card .status-ok { background:#2fa36b; }
.stat-card .status-warn { background:#f4b860; }
.stat-card .status-err { background:#ff6b6b; }
.stat-note { margin-top:12px; padding:1px 0 1px 10px; border-left:2px solid #f4b860; color:var(--muted); font-size:12px; line-height:1.5; }
.stat-note strong { color:var(--text); font-weight:600; }
.account-card,.form-section,.test-results-section,.terminal { background:var(--panel); border-color:var(--line); border-radius:12px; box-shadow:none; }
.account-card:hover,.form-section:hover,.test-results-section:hover { border-color:#35404d; box-shadow:0 12px 36px rgba(0,0,0,.18); }
.card-header,.test-results-section .test-results-header { background:var(--panel-2); border-color:var(--line); }
.card-footer { background:#0e1319; border-color:var(--line); }
.card-header .label,.form-section h3,.modal h3 { color:var(--text); }
.form-section { padding:22px; }
.form-section h3 { margin-bottom:7px; }
.form-copy { font-size:12px; color:var(--muted); margin-bottom:18px; line-height:1.5; }
.form-group label { color:var(--muted); font-family:Inter,sans-serif; text-transform:none; font-size:12px; letter-spacing:0; margin-bottom:7px; }
.form-group input,.form-group select,.select { background:#0c1117 !important; border:1px solid var(--line) !important; color:var(--text) !important; border-radius:8px !important; min-height:40px; }
.form-group textarea { width:100%; min-height:120px; resize:vertical; background:#0c1117; border:1px solid var(--line); color:var(--text); border-radius:8px; padding:11px 12px; font:12px/1.6 'SF Mono',Menlo,Consolas,monospace; }
.form-group textarea:focus { border-color:var(--accent); outline:2px solid rgba(124,108,255,.3); }
.form-group input:focus { border-color:var(--accent) !important; box-shadow:0 0 0 3px rgba(124,108,255,.15); }
.btn { min-height:36px; padding:7px 13px; border-radius:8px; border-color:var(--line); background:#181e27; color:#d6dce3; font-family:Inter,sans-serif; font-size:12px; font-weight:650; }
.btn:hover { background:#222a35; border-color:#3a4553; }
.btn-primary { background:var(--accent); border-color:var(--accent); color:white; }
.btn-primary:hover { background:#8a7cff; border-color:#8a7cff; }
.btn-danger { color:#ff8585; border-color:rgba(255,107,107,.28); background:rgba(255,107,107,.04); }
.btn-active { color:white; background:#257c59; border-color:#257c59; }
.btn-quiet { background:transparent; }
.icon-btn { width:38px; padding:0; display:grid; place-items:center; }
.error,.success { position:fixed; top:18px; right:18px; z-index:200; width:min(380px,calc(100vw - 36px)); padding:13px 15px; border-radius:10px; box-shadow:0 18px 50px rgba(0,0,0,.4); font-family:Inter,sans-serif; }
.error { color:#ffc0c0; background:#31181c; border-color:#673037; }
.success { color:#b7f5d5; background:#123024; border-color:#285f49; }
.terminal { border-color:#1e382a; }
.system-grid { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:14px; }
.system-grid .form-section { margin-bottom:0; }
.span-2 { grid-column:span 2; }
.toolbar { display:flex; align-items:center; justify-content:space-between; gap:12px; margin-bottom:16px; }
.search-wrap { position:relative; width:min(360px,100%); }
.search-wrap input { width:100%; height:38px; background:#0e1319; border:1px solid var(--line); border-radius:9px; color:var(--text); padding:0 12px 0 36px; }
.search-wrap svg { position:absolute; left:12px; top:10px; width:17px; color:#667281; }
.helper { color:var(--muted); font-size:11px; line-height:1.5; }
.account-summary { display:inline-flex; align-items:center; gap:7px; min-height:38px; padding:0 12px; border:1px solid var(--line); border-radius:9px; background:#0e1319; color:var(--muted); font-size:12px; white-space:nowrap; }
.account-summary strong { color:var(--green); font-size:13px; }
.account-summary.warn strong { color:var(--amber); }
.account-summary.calm strong { color:var(--muted); }
.toolbar-stats { display:flex; flex-wrap:wrap; align-items:center; gap:8px; }
.account-group { display:flex; flex-direction:column; gap:12px; }
.account-group + .account-group { margin-top:12px; }
.account-group-head { display:flex; flex-wrap:wrap; align-items:center; gap:4px 10px; padding-bottom:10px; border-bottom:1px solid var(--line); }
.account-group-head h3 { font-size:14px; letter-spacing:-.1px; }
.account-group-head p { flex-basis:100%; color:var(--muted); font-size:12px; }
.account-group-head .group-mark { width:10px; height:10px; border-radius:3px; }
.account-group-head .group-count { padding:1px 8px; border-radius:999px; font-size:12px; font-weight:600; font-variant-numeric:tabular-nums; }
.account-group[data-group="playback"] .group-mark { background:#2fa36b; }
.account-group[data-group="playback"] .group-count { color:#64df9b; background:rgba(46,160,103,.13); }
.account-group[data-group="catalog"] .group-mark { background:#d2a8ff; }
.account-group[data-group="catalog"] .group-count { color:#d2a8ff; background:rgba(210,168,255,.1); }
.account-group-list { display:flex; flex-direction:column; gap:20px; }
.account-card.role-playback { border-left:3px solid #2fa36b; }
.account-card.role-catalog { border-left:3px solid #b78cf0; }
.auth-error { min-height:20px; color:#ff8e8e; font-size:12px; margin-top:10px; }
.mobile-bar,.sidebar-scrim { display:none; }
.overlay { background:rgba(3,5,8,.75); }
.modal { background:#121820; border-color:var(--line); border-radius:14px; box-shadow:0 28px 90px rgba(0,0,0,.55); }
.modal .section-head { flex-direction:row; align-items:flex-start; }
.auth-screen { min-height:100vh; display:grid; grid-template-columns:1fr; place-items:center; padding:24px; background:var(--bg); }
.minimal-login { position:relative; width:min(420px,100%); }
.minimal-login input { width:100%; height:54px; padding:0 60px 0 17px; background:#151b23; border:1px solid #343d4a; border-radius:12px; color:var(--text); font-size:15px; box-shadow:0 16px 60px rgba(0,0,0,.16); }
.minimal-login input::placeholder { color:#778393; }
.minimal-login button { position:absolute; right:6px; top:6px; width:42px; height:42px; border:0; border-radius:9px; background:var(--accent); color:white; font-size:23px; cursor:pointer; }
.minimal-login button:hover { background:#8a7cff; }
.minimal-login button:disabled { opacity:.55; cursor:wait; }
.minimal-login .auth-error { margin:11px 2px 0; min-height:18px; }
.sr-only { position:absolute; width:1px; height:1px; padding:0; margin:-1px; overflow:hidden; clip:rect(0,0,0,0); white-space:nowrap; border:0; }
@media (max-width:1150px) {
  .ov-4 { grid-template-columns:repeat(2,minmax(0,1fr)); }
}
@media (max-width:980px) {
  .system-grid { grid-template-columns:1fr; }
  .span-2 { grid-column:auto; }
}
@media (max-width:760px) {
  body { padding:0; }
  .sidebar { transform:translateX(-100%); transition:transform .22s ease; }
  .sidebar.open { transform:none; }
  .sidebar-scrim.open { display:block; position:fixed; inset:0; z-index:35; background:rgba(0,0,0,.58); }
  .mobile-bar { display:flex; position:sticky; top:0; z-index:30; height:58px; align-items:center; justify-content:space-between; padding:0 16px; background:rgba(10,13,18,.9); backdrop-filter:blur(16px); border-bottom:1px solid var(--line); }
  .workspace { margin-left:0; padding:20px 16px 50px; }
  .header { margin-top:4px; }
  .header h1 { font-size:24px; }
  .header-actions .btn:not(.mobile-keep) { display:none; }
  .ov-2,.ov-3 { grid-template-columns:1fr; }
  .toolbar,.section-head { align-items:stretch; flex-direction:column; }
  .search-wrap { width:100%; }
}
@media (max-width:480px) {
  .ov-4 { grid-template-columns:1fr; }
  .stat-wide { grid-column:auto; }
  .stat-tall { grid-row:auto; }
  .card-actions { width:100%; display:grid; grid-template-columns:1fr 1fr; }
  .card-actions .btn { width:100%; }
  .section-actions { display:grid; grid-template-columns:1fr 1fr; }
  .modal { max-width:calc(100vw - 20px); }
}
</style>
</head>
<body>
<div id="loginScreen" class="auth-screen">
  <form class="minimal-login" onsubmit="login(event)">
    <label for="admin-key-input" class="sr-only">Ключ администратора</label>
    <input id="admin-key-input" type="password" autocomplete="current-password" placeholder="Ключ администратора">
    <button id="loginBtn" type="submit" aria-label="Войти">→</button>
    <div id="loginError" class="auth-error" role="alert"></div>
  </form>
</div>

<div id="appShell" class="app-shell hidden">
<div id="sidebarScrim" class="sidebar-scrim" onclick="toggleSidebar(false)"></div>
<aside id="sidebar" class="sidebar">
  <div class="brand"><span class="brand-mark">HF</span><div><strong>HiFi API</strong><small>Панель управления</small></div></div>
  <div class="nav-label">Рабочее пространство</div>
  <nav class="nav" aria-label="Основная навигация">
    <button class="nav-btn active" data-view="overview" onclick="showView('overview')"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="3" y="3" width="7" height="7" rx="2"/><rect x="14" y="3" width="7" height="7" rx="2"/><rect x="3" y="14" width="7" height="7" rx="2"/><rect x="14" y="14" width="7" height="7" rx="2"/></svg>Обзор</button>
    <button class="nav-btn" data-view="accounts" onclick="showView('accounts')"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><circle cx="12" cy="8" r="4"/><path d="M4 21a8 8 0 0 1 16 0"/></svg>Аккаунты</button>
    <button class="nav-btn" data-view="access" onclick="showView('access')"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><circle cx="8" cy="15" r="4"/><path d="m11 12 9-9m-3 3 3 3m-6 0 3 3"/></svg>Доступ к API</button>
    <button class="nav-btn" data-view="system" onclick="showView('system')"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H2.8v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1a1.7 1.7 0 0 0 1.9.3A1.7 1.7 0 0 0 10 3V2.8h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z"/></svg>Система</button>
  </nav>
  <div class="sidebar-bottom"><div class="admin-user"><span class="admin-avatar">AD</span><span><strong style="display:block;font-size:12px">Администратор</strong><span style="color:var(--green);font-size:10px">● Сессия активна</span></span></div><button class="logout-btn" onclick="logout()">Выйти из панели</button></div>
</aside>

<div class="mobile-bar"><button class="btn icon-btn btn-quiet" onclick="toggleSidebar(true)" aria-label="Открыть меню">☰</button><strong>HiFi API</strong><span id="mobileVersion" style="font-size:11px;color:var(--muted)">v__HIFI_VERSION__</span></div>
<main class="workspace"><div class="container">
  <header class="header">
    <div><div class="page-kicker" id="pageKicker">Состояние сервиса</div><h1 id="pageTitle">Обзор</h1><p class="page-subtitle" id="pageSubtitle">Главные показатели и последние запросы в одном месте.</p></div>
    <div class="header-actions"><button class="btn" onclick="refreshCurrentView()">Обновить</button><button class="btn btn-primary mobile-keep" onclick="showView('accounts');openAddAccount()">+ Аккаунт</button><span class="badge" id="version">v__HIFI_VERSION__</span></div>
  </header>
  <div id="error" class="error"></div><div id="success" class="success"></div>

  <section id="view-overview" class="view active">
    <div id="stats" class="overview"></div>
    <div class="section-head"><div><h2>Журнал запросов</h2><p>Последняя активность обновляется автоматически каждые 5 секунд.</p></div><div class="section-actions"><button class="btn" id="requestLogDetails" aria-pressed="false" onclick="toggleRequestLogDetails()">Доп. поля</button><button class="btn" id="requestLogMore" onclick="showMoreRequests()">Показать ещё</button><button class="btn" onclick="loadRequestLog()">Обновить журнал</button></div></div>
    <div class="terminal" id="requestLogTerminal">
      <div class="term-bar"><span class="term-dots"><i></i><i></i><i></i></span><span class="term-title">hifi-api — live request log</span><span class="term-live" id="term-live">● LIVE</span></div>
      <div class="term-meta"><span>Всего <strong id="rq-total">—</strong></span><span>Показано <strong id="rq-shown">—</strong></span><span>Ошибок <strong id="rq-errors">—</strong></span><span>4xx без 429 <strong id="rq-user-errors">—</strong></span><span>429/5xx <strong id="rq-upstream-errors">—</strong></span><span>p50 <strong id="rq-p50">—</strong></span><span>p95 <strong id="rq-p95">—</strong></span><span id="rq-endpoints"></span><span id="rq-tracks" style="color:#d2a8ff"></span></div>
      <div class="term-meta" id="rq-slowest"></div>
      <div id="rq-recent" class="term-body"></div>
    </div>
    <details class="terminal errors-panel" id="errorsPanel">
      <summary class="term-bar"><span class="term-title">Последние ошибки API</span><span class="err-count" id="er-count">—</span></summary>
      <div class="term-meta" id="er-causes"></div>
      <div class="term-meta" id="er-accounts"></div>
      <div id="er-list" class="term-body"></div>
    </details>
  </section>

  <section id="view-accounts" class="view">
    <div id="readyHistory" class="ready-history" data-no-i18n></div>
    <div class="section-head"><div><h2>Tidal-аккаунты</h2><p>Управляйте пулом воспроизведения, токенами и каталогом.</p></div><div class="section-actions"><button class="btn" onclick="testAll()" id="testAllBtn">Проверить все</button><button class="btn btn-primary" onclick="openAddAccount()">+ Добавить</button></div></div>
    <div class="toolbar"><div class="search-wrap"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/></svg><input id="accountSearch" type="search" placeholder="Найти аккаунт..." oninput="filterAccounts(this.value)"></div><div class="toolbar-stats"><div class="account-summary" id="accountCount"><span class="status-dot status-ok"></span><span>Готовы сейчас</span><strong>— / —</strong></div><div class="toolbar-stats" id="subscriptionSummary" data-no-i18n></div></div></div>
    <div id="accounts-container" class="accounts-grid"></div>
    <div class="test-results-section" id="testResultsSection" style="display:none"><div class="test-results-header"><h3>Результаты проверки</h3><div class="test-summary" id="testSummary"></div></div><div class="test-results-body" id="testResultsList"></div></div>
  </section>

  <section id="view-access" class="view">
    <div class="section-head"><div><h2>Ключи доступа</h2><p>Контролируйте клиентов API и их квоты.</p></div></div>
    <div class="form-section">
      <h3>Новый API-ключ</h3><p class="form-copy">Квота — общее число запросов с этим ключом ко всем маршрутам API (включая поиск и получение трека), без ежедневного сброса. 0 — без лимита. После создания ключ показывается только один раз.</p>
      <div class="form-row"><div class="form-group"><label>Название</label><input type="text" id="new-key-label" placeholder="Например, мобильное приложение"></div><div class="form-group"><label>Квота запросов</label><input type="number" id="new-key-quota" min="0" placeholder="0"></div></div>
      <button class="btn btn-primary" onclick="addApiKey()">Создать ключ</button><div id="keyResult" style="font-size:12px;margin-top:12px;color:var(--green);word-break:break-all"></div>
    </div>
    <div class="form-section"><h3>API-ключи</h3><p class="form-copy">Если ключей нет, публичные маршруты API остаются открытыми.</p><div id="keys-container"></div></div>
  </section>

  <section id="view-system" class="view">
    <div class="section-head"><div><h2>Системные настройки</h2><p>Интеграции, резервные копии и обслуживание.</p></div></div>
    <div class="system-grid">
      <div class="form-section"><h3>Настройки панели</h3><p class="form-copy">Язык интерфейса сохраняется в cookie этого браузера.</p><div class="form-group"><label for="panel-language">Язык интерфейса</label><select id="panel-language" class="select" style="width:100%;padding:10px 12px" onchange="setLanguage(this.value)"><option value="en">English</option><option value="ru">Русский</option></select></div></div>
      <div class="form-section"><h3>Воспроизведение</h3><p class="form-copy">Настройки выбора формата и исходящих запросов к Tidal.</p><div class="form-group" style="margin-bottom:16px"><label>Формат по умолчанию</label><select id="rl-atmos" class="select" style="width:100%;padding:10px 12px"><option value="high">HIGH · AAC 320 kbps (v1)</option><option value="off">FLAC в приоритете</option><option value="prefer">Atmos в приоритете</option></select></div><div class="form-group" style="margin-bottom:16px"><label for="rl-v1-endpoint">Метод воспроизведения v1</label><select id="rl-v1-endpoint" class="select" style="width:100%;padding:10px 12px"><option value="playbackinfopostpaywall">postpaywall (по умолчанию)</option><option value="playbackinfo">playbackinfo (старый)</option></select></div><p class="form-copy">Применяется к v1-запросам треков и проверкам FULL. Изменения действуют сразу.</p><div class="form-row"><div class="form-group"><label for="rl-track-concurrency">Одновременных запросов на аккаунт воспроизведения</label><input type="number" id="rl-track-concurrency" min="0" max="1000" value="1"></div><div class="form-group"><label for="rl-track-delay">Пауза аккаунта между запросами, мс</label><input type="number" id="rl-track-delay" min="0" max="10000" step="50" value="0"></div></div><p class="form-copy">Сколько запросов один аккаунт воспроизведения одновременно отправляет в Tidal — треки, видео, лицензии и метаданные, если они попали в пул. Занятый или отдыхающий аккаунт пропускается, запрос берёт свободный; если свободных нет, запрос ждёт в очереди. 0 — без ограничения. Пауза — сколько аккаунт отдыхает после каждого запроса, прежде чем взять следующий (до 10 000 мс). Аккаунтов «только каталог» это не касается.</p><div class="form-row"><div class="form-group"><label for="rl-track-requests">Попыток на одном аккаунте · треки и видео</label><input type="number" id="rl-track-requests" min="1" max="10" value="1"></div><div class="form-group"><label for="rl-catalog-requests">Попыток на одном аккаунте · метаданные</label><input type="number" id="rl-catalog-requests" min="1" max="10" value="1"></div></div><p class="form-copy">Сколько раз один запрос может обратиться к одному и тому же аккаунту, считая первую попытку. 1 — без повторов; 2 — один повтор, например после обновления токена при 401. Метаданные — это поиск, альбомы, артисты, плейлисты и т. п., с любого аккаунта. Если аккаунт не справился, запрос уходит на следующий независимо от этой настройки; цепочка аккаунтов видна в журнале.</p><div class="form-group" style="margin-bottom:8px"><label for="rl-catalog-concurrency">Одновременных запросов на аккаунт «только каталог»</label><input type="number" id="rl-catalog-concurrency" min="0" max="1000" value="0"></div><p class="form-copy">Касается только аккаунтов «только каталог». 0 — без ограничения. Когда все они заняты, запросы метаданных уходят в пул воспроизведения.</p><div class="form-group" style="margin-bottom:8px"><label for="rl-per-hour">Запросов в час на аккаунт воспроизведения</label><input type="number" id="rl-per-hour" min="0" max="10000" value="0"></div><p class="form-copy">Аккаунт, который уже начал столько запросов за последний час, пропускается до освобождения квоты. Помогает не перегружать отдельные аккаунты. 0 — без ограничения.</p><div class="form-group" style="margin-bottom:8px"><label for="rl-auth-disable">Отключать аккаунт после 403 при обновлении токена, ч</label><input type="number" id="rl-auth-disable" min="0" max="720" value="24"></div><p class="form-copy">Если Tidal отвечает 403 на каждое обновление токена дольше этого времени, аккаунт выключается и больше не пробует обновиться, пока вы не включите его сами. Отсчёт идёт с первого 403 и сбрасывается при успешном обновлении. 0 — никогда не выключать.</p><p class="form-copy">Автовосстановление всегда включено. Отключённые вручную аккаунты остаются выключенными.</p><div class="section-actions"><button class="btn btn-primary" onclick="saveSettings()">Сохранить</button><button class="btn" id="clearPlaybackBtn" onclick="clearPlaybackQueue()">Очистить очередь</button></div></div>
      <div class="form-section"><h3>Прокси</h3><p class="form-copy">Маршрутизация исходящих запросов. Изменения применяются сразу.</p><div class="card-stats" style="margin-bottom:16px"><span class="card-stat">Статус <strong id="px-status">—</strong></span><span class="card-stat">Назначено <strong id="px-current">—</strong></span><span class="card-stat">В пуле <strong id="px-pool">—</strong></span><span class="card-stat">Проверено назначений <strong id="px-verified">—</strong></span><span class="card-stat" title="Сумма последовательных ошибок по текущим назначениям; сбрасываются после успешного запроса">Ошибок подряд <strong id="px-fails">—</strong></span></div><div id="px-assignments" class="helper" style="white-space:pre-line;margin-bottom:16px"></div><div class="form-group"><label for="px-list">Адреса прокси · по одному в строке</label><textarea id="px-list" spellcheck="false" placeholder="http://user:password@host:port" oninput="proxyListDirty=true"></textarea></div><div class="section-actions" style="margin-top:12px"><button class="btn btn-primary" id="pxToggleBtn" onclick="toggleProxies()">Включить прокси</button><button class="btn" id="pxSaveBtn" onclick="saveProxyList()">Сохранить список</button></div><p class="helper" id="px-persist" style="margin-top:11px"></p></div>
      <div class="form-section"><h3>Уведомления</h3><p class="form-copy">Discord-оповещения о риске блокировки и недоступности аккаунтов.</p><div class="card-stats" style="margin-bottom:15px"><span class="card-stat">Discord <strong id="al-discord">—</strong></span></div><div class="form-group"><label for="al-webhook">URL вебхука Discord</label><input type="password" id="al-webhook" placeholder="https://discord.com/api/webhooks/…" autocomplete="new-password" spellcheck="false"></div><p class="helper" style="margin:8px 0 14px">Сохранённый URL скрыт. Введите новый, чтобы заменить его.</p><div class="section-actions" style="margin-bottom:15px"><button class="btn btn-primary" onclick="saveDiscordWebhook()" id="saveWebhookBtn">Сохранить вебхук</button><button class="btn btn-danger" onclick="disableDiscordWebhook()" id="disableWebhookBtn">Отключить вебхук</button></div><div class="section-actions"><button class="btn" onclick="testAlert()" id="alertTestBtn">Тест</button><button class="btn" onclick="sendReport('status')" id="reportStatusBtn">Статус</button><button class="btn" onclick="sendReport('accounts')" id="reportAccountsBtn">Аккаунты</button></div></div>
      <div class="form-section"><h3>Кэш</h3><p class="form-copy">Метаданные: плейлисты и миксы свежие 15 мин, поиск — 4 ч, остальное (альбомы, артисты, обложки, тексты, info, рекомендации) — 12 ч. После этого запись ещё столько же отдаётся мгновенно и обновляется в фоне. Треки, манифесты и DASH: одинаковый запрос в течение 120 с отдаётся из кэша без обращения к Tidal; видео и лицензии не кэшируются. Кэш хранится в памяти и сбрасывается при перезапуске.</p><p class="form-copy">Счётчики запросов с запуска сервера. Устаревшие и объединённые ответы входят в попадания. Очистка кэша не обнуляет счётчики.</p><div class="card-stats" style="margin-bottom:15px"><span class="card-stat">Попадания <strong id="cc-hits">—</strong></span><span class="card-stat">Промахи <strong id="cc-misses">—</strong></span><span class="card-stat" title="Попадания от суммы попаданий и промахов">Доля попаданий <strong id="cc-rate">—</strong></span><span class="card-stat">Из устаревшего кэша <strong id="cc-stale">—</strong></span><span class="card-stat">Ошибок из кэша <strong id="cc-negative">—</strong></span><span class="card-stat">Объединённых запросов <strong id="cc-coalesced">—</strong></span><span class="card-stat" title="Запросы треков, манифестов и DASH, отданные из кэша за 120 с">Треки из кэша <strong id="cc-tracks">—</strong></span></div><button class="btn" onclick="clearCache()" id="clearCacheBtn">Очистить кэш</button></div>
      <div class="form-section"><h3>Аптайм аккаунтов</h3><p class="form-copy">Сброс удаляет историю доступности всех аккаунтов и начинает отсчёт заново — например, чтобы проверить стабильность после обновления.</p><button class="btn btn-danger" onclick="resetUptime()" id="resetUptimeBtn">Сбросить аптайм</button></div>
      <div class="form-section"><h3>Учётные данные</h3><p class="form-copy">Экспортируйте или импортируйте Tidal-аккаунты в JSON. Дубликаты токенов будут пропущены.</p><div class="section-actions"><button class="btn" onclick="exportCredentials()">Экспорт JSON</button><button class="btn" onclick="document.getElementById('importFile').click()">Импорт JSON</button><input type="file" id="importFile" accept=".json,application/json" style="display:none" onchange="importCredentials(event)"></div><div id="importResult" class="helper" style="margin-top:10px"></div></div>
      <div class="form-section"><h3>База данных</h3><p class="form-copy">Скачайте полный снимок или восстановите состояние без перезапуска.</p><div class="section-actions"><button class="btn" onclick="downloadBackup()">Скачать копию</button><button class="btn btn-danger" onclick="document.getElementById('restoreFile').click()">Восстановить</button><input type="file" id="restoreFile" accept=".db,.sqlite,.sqlite3,application/x-sqlite3" style="display:none" onchange="restoreBackup(event)"></div><div id="restoreResult" class="helper" style="margin-top:10px"></div></div>
    </div>
  </section>
</div></main>
</div>

<div id="addAccountOverlay" class="overlay" data-mode="oauth" onclick="if(event.target===this)closeAddAccount()"><form class="modal add-account" style="width:560px" onsubmit="event.preventDefault();submitAddAccount()"><div class="section-head"><div><div class="page-kicker">Новый аккаунт</div><h3 style="margin:0">Подключить Tidal</h3></div><button type="button" class="btn icon-btn btn-quiet" onclick="closeAddAccount()" aria-label="Закрыть">×</button></div><div class="mode-tabs" role="tablist"><button type="button" role="tab" id="addModeOauth" aria-selected="true" onclick="setAddAccountMode('oauth')">OAuth</button><button type="button" role="tab" id="addModeManual" aria-selected="false" onclick="setAddAccountMode('manual')">Вручную</button></div><p class="form-copy oauth-only">Откроется ссылка Tidal: войдите и подтвердите доступ — аккаунт добавится сам.</p><p class="form-copy manual-only">Для уже готовых credentials: Client ID, Client Secret и Refresh Token.</p><div class="form-row"><div class="form-group"><label for="new-label">Название · необязательно</label><input type="text" id="new-label" placeholder="Основной аккаунт"></div><div class="form-group manual-only"><label for="new-user-id">User ID · необязательно</label><input type="text" id="new-user-id" placeholder="208921067"></div></div><div class="manual-only"><div class="form-group" style="margin:0 0 14px"><label for="new-client-id">Client ID</label><input type="text" id="new-client-id" placeholder="client_id"></div><div class="form-row"><div class="form-group"><label for="new-client-secret">Client Secret</label><div class="pw-wrap"><input type="password" id="new-client-secret" placeholder="client_secret"><button type="button" class="pw-toggle" onclick="togglePw('new-client-secret', this)">&#128065;</button></div></div><div class="form-group"><label for="new-refresh-token">Refresh Token</label><div class="pw-wrap"><input type="password" id="new-refresh-token" placeholder="refresh_token"><button type="button" class="pw-toggle" onclick="togglePw('new-refresh-token', this)">&#128065;</button></div></div></div></div><label style="display:flex;align-items:center;gap:8px;font-size:12px;color:var(--muted);margin:16px 0"><input type="checkbox" id="new-catalog"> Только каталог — без воспроизведения</label><div class="modal-actions"><button type="submit" class="btn btn-primary oauth-only" id="oauthBtn">Подключить через OAuth</button><button type="submit" class="btn btn-primary manual-only">Добавить аккаунт</button><button type="button" class="btn" onclick="closeAddAccount()">Отмена</button></div></form></div>
<div id="oauthOverlay" class="overlay" onclick="if(event.target===this)closeOAuth()">
<div class="modal">
<h3>Авторизация через Tidal</h3>
<p style="margin-bottom:16px;color:#8b949e;font-size:14px">Откройте ссылку, войдите в Tidal и подтвердите доступ. Панель сама заметит завершение.</p>
<div style="background:#0d1117;border:1px solid #30363d;border-radius:6px;padding:16px;word-break:break-all;font-size:13px;font-family:monospace;color:#58a6ff;margin-bottom:16px" id="oauthUrl">—</div>
<button class="btn" onclick="copyOAuthUrl()" id="copyOAuthBtn" style="margin-right:8px">Копировать</button>
<button class="btn btn-primary" onclick="openOAuthUrl()" id="openOAuthBtn">Открыть Tidal</button>
<div class="form-group" style="margin-top:16px"><label>Название аккаунта</label><input type="text" id="oauth-modal-label" placeholder="Мой Tidal" oninput="updateOAuthLabel()"></div>
<p style="margin-top:16px;color:#8b949e;font-size:13px" id="oauthStatus">Ожидаем авторизацию…</p>
<div class="modal-actions">
<button class="btn" onclick="closeOAuth()">Отмена</button>
</div>
</div>
</div>

<div id="editOverlay" class="overlay" onclick="if(event.target===this)closeEdit()">
<div class="modal">
<h3 id="editTitle">Редактировать аккаунт</h3>
<div class="form-group"><label>Название</label><input type="text" id="ed-label"></div>
<div class="form-group"><label>User ID</label><input type="text" id="ed-user-id"></div>
<div class="form-group"><label>Client ID</label><input type="text" id="ed-client-id"></div>
<div class="form-group"><label>Client Secret</label><div class="pw-wrap"><input type="password" id="ed-client-secret"><button type="button" class="pw-toggle" onclick="togglePw('ed-client-secret', this)" title="Show/hide">&#128065;</button></div></div>
<div class="form-group"><label>Refresh Token</label><div class="pw-wrap"><input type="password" id="ed-refresh-token"><button type="button" class="pw-toggle" onclick="togglePw('ed-refresh-token', this)" title="Show/hide">&#128065;</button></div></div>
<div class="modal-actions">
<button class="btn btn-primary" onclick="saveEdit()">Сохранить</button>
<button class="btn" onclick="closeEdit()">Отмена</button>
</div>
</div>
</div>

<div id="testOverlay" class="overlay" onclick="if(event.target===this)closeTestDetails()">
<div class="modal" style="width:680px">
<h3 id="testTitle">Результат проверки</h3>
<div id="testDetailsContent" style="font-size:12px;line-height:1.6;max-height:60vh;overflow-y:auto;scrollbar-width:none"></div>
<div class="modal-actions">
<button class="btn" onclick="closeTestDetails()">Закрыть</button>
</div>
</div>
</div>

<script>
__HIFI_I18N__
var API = window.location.origin;
var legacyAdminKey = sessionStorage.getItem('admin_key') || localStorage.getItem('admin_key') || '';
sessionStorage.removeItem('admin_key');
localStorage.removeItem('admin_key');
window._accounts = [];
var editId = null;
var authenticated = false;
var proxyEnabled = false;
var proxyListDirty = false;
var currentView = 'overview';
var viewMeta = {
    overview: ['Состояние сервиса', 'Обзор', 'Главные показатели и последние запросы в одном месте.'],
    accounts: ['Управление пулом', 'Аккаунты', 'Tidal-аккаунты, токены и роли каталога.'],
    access: ['Безопасность', 'Доступ к API', 'Ключи клиентов, квоты и состояние доступа.'],
    system: ['Конфигурация', 'Система', 'Интеграции, резервные копии и обслуживание.']
};

function headers() {
    return { 'Content-Type': 'application/json' };
}

function showLogin(message) {
    authenticated = false;
    document.getElementById('appShell').classList.add('hidden');
    document.getElementById('loginScreen').classList.remove('hidden');
    document.getElementById('loginError').textContent = message || '';
    var input = document.getElementById('admin-key-input');
    input.value = '';
    setTimeout(function() { input.focus(); }, 50);
}

function showApp() {
    authenticated = true;
    document.getElementById('loginScreen').classList.add('hidden');
    document.getElementById('appShell').classList.remove('hidden');
}

async function login(event) {
    if (event) event.preventDefault();
    var btn = document.getElementById('loginBtn');
    var candidate = document.getElementById('admin-key-input').value.trim();
    btn.disabled = true;
    btn.textContent = '…';
    document.getElementById('loginError').textContent = '';
    try {
        var res = await fetch('/admin/login', { method:'POST', headers:headers(), body:JSON.stringify({ key:candidate }) });
        if (res.status === 401) throw new Error('Ключ не подходит. Проверьте значение и попробуйте ещё раз.');
        if (res.status === 429) throw new Error('Слишком много неверных попыток. Повторите через ' + Math.ceil(Number(res.headers.get('Retry-After') || 900) / 60) + ' мин.');
        if (!res.ok) throw new Error('Сервис временно недоступен: HTTP ' + res.status);
        document.getElementById('admin-key-input').value = '';
        showApp();
        await refreshAll();
    } catch(e) {
        document.getElementById('loginError').textContent = e.message;
    } finally {
        btn.disabled = false;
        btn.textContent = '→';
    }
}

async function logout() {
    try {
        var res = await fetch('/admin/logout', { method:'POST', headers:headers() });
        if (!res.ok) throw new Error('HTTP ' + res.status);
    } catch(e) {
        document.getElementById('error').textContent = 'Не удалось завершить сессию: ' + e.message;
        return;
    }
    window._accounts = [];
    document.getElementById('accounts-container').innerHTML = '';
    document.getElementById('keys-container').innerHTML = '';
    document.getElementById('keyResult').textContent = '';
    document.querySelectorAll('.overlay').forEach(function(el) { el.classList.remove('open'); });
    if (oauthPollInterval) { clearInterval(oauthPollInterval); oauthPollInterval = null; }
    showLogin('');
}

function setKey() { showLogin('Сессия истекла. Введите ключ ещё раз.'); }

function showView(name) {
    currentView = name;
    document.querySelectorAll('.view').forEach(function(el) { el.classList.toggle('active', el.id === 'view-' + name); });
    document.querySelectorAll('.nav-btn').forEach(function(el) { el.classList.toggle('active', el.dataset.view === name); });
    var meta = viewMeta[name] || viewMeta.overview;
    document.getElementById('pageKicker').textContent = meta[0];
    document.getElementById('pageTitle').textContent = meta[1];
    document.getElementById('pageSubtitle').textContent = meta[2];
    toggleSidebar(false);
    if (name === 'overview') { refreshStats(); loadRequestLog(); loadProxyStatus(); }
    window.scrollTo({ top:0, behavior:'smooth' });
}

function toggleSidebar(open) {
    document.getElementById('sidebar').classList.toggle('open', !!open);
    document.getElementById('sidebarScrim').classList.toggle('open', !!open);
}

// OAuth is the default; manual entry is only for credentials that already exist.
function setAddAccountMode(mode) {
    var manual = mode === 'manual';
    document.getElementById('addAccountOverlay').dataset.mode = manual ? 'manual' : 'oauth';
    document.getElementById('addModeOauth').setAttribute('aria-selected', String(!manual));
    document.getElementById('addModeManual').setAttribute('aria-selected', String(manual));
}
function submitAddAccount() {
    if (document.getElementById('addAccountOverlay').dataset.mode === 'manual') addAccount();
    else startOAuth();
}
function openAddAccount(mode) {
    setAddAccountMode(mode);
    document.getElementById('addAccountOverlay').classList.add('open');
    setTimeout(function(){ document.getElementById(mode === 'manual' ? 'new-client-id' : 'new-label').focus(); }, 100);
}
function closeAddAccount() { document.getElementById('addAccountOverlay').classList.remove('open'); }

function filterAccounts(query) {
    var q = (query || '').trim().toLowerCase();
    document.querySelectorAll('#accounts-container .account-card').forEach(function(card) {
        card.style.display = !q || (card.dataset.search || '').indexOf(q) !== -1 ? '' : 'none';
    });
    document.querySelectorAll('#accounts-container .account-group').forEach(function(group) {
        var visible = Array.prototype.some.call(group.querySelectorAll('.account-card'), function(card) { return card.style.display !== 'none'; });
        group.style.display = visible ? '' : 'none';
    });
}

function plural(n, one, few, many) {
    var mod10 = n % 10, mod100 = n % 100;
    return mod10 === 1 && mod100 !== 11 ? one : (mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14) ? few : many);
}

async function bootstrap() {
    try {
        if (legacyAdminKey) {
            await fetch('/admin/login', { method:'POST', headers:headers(), body:JSON.stringify({ key:legacyAdminKey }) });
            legacyAdminKey = '';
        }
        var res = await fetch('/admin/stats', { headers: headers() });
        if (res.status === 401) { showLogin(''); return; }
        if (res.status === 429) { showLogin('Слишком много неверных попыток. Повторите позже.'); return; }
        if (!res.ok) { showLogin('Не удалось подключиться к сервису: HTTP ' + res.status); return; }
        showApp();
        refreshAll();
    } catch(e) { showLogin('Не удалось подключиться к сервису: ' + e.message); }
}

async function refreshAll() {
    await Promise.all([fetchData(), loadRequestLog(), loadSettings(), loadProxyStatus(), loadAlertStatus(), loadCacheStats(), loadApiKeys()]);
}

function refreshCurrentView() {
    if (currentView === 'overview') { fetchData(); loadRequestLog(); loadProxyStatus(); }
    else if (currentView === 'accounts') fetchData();
    else if (currentView === 'access') loadApiKeys();
    else Promise.all([loadSettings(), loadProxyStatus(), loadAlertStatus(), loadCacheStats()]);
}

document.addEventListener('keydown', function(event) {
    if (event.key !== 'Escape') return;
    closeAddAccount(); closeOAuth(); closeEdit(); closeTestDetails(); toggleSidebar(false);
});

['error', 'success'].forEach(function(id) {
    var el = document.getElementById(id);
    var timeout;
    new MutationObserver(function() {
        clearTimeout(timeout);
        if (el.textContent.trim()) timeout = setTimeout(function() { el.textContent = ''; }, 6000);
    }).observe(el, { childList:true, characterData:true, subtree:true });
});

function timeStr(ts) {
    if (!ts || ts === 0) return 'Нет данных';
    var d = new Date(ts * 1000);
    var diff = d - new Date();
    if (diff < 0) {
        var ago = Math.floor(-diff / 60000);
        if (ago < 60) return 'истёк ' + ago + ' мин назад';
        return 'истёк ' + Math.floor(ago / 60) + ' ч назад';
    }
    var mins = Math.floor(diff / 60000);
    if (mins < 60) return mins + ' мин';
    return Math.floor(mins / 60) + ' ч ' + (mins % 60) + ' мин';
}

function sleepDuration(ts) {
    var minutes = Math.floor(Math.max(0, Date.now() / 1000 - ts) / 60);
    if (minutes < 1) return 'меньше минуты';
    var days = Math.floor(minutes / 1440);
    var hours = Math.floor((minutes % 1440) / 60);
    var remainingMinutes = minutes % 60;
    if (days) return days + ' д ' + hours + ' ч';
    if (hours) return hours + ' ч ' + remainingMinutes + ' мин';
    return minutes + ' мин';
}

function uptimeDuration(seconds) {
    var s = Math.max(0, Math.floor(Number(seconds) || 0));
    var units = adminLanguage === 'ru' ? ['д', 'ч', 'мин', 'с'] : ['d', 'hr', 'min', 's'];
    if (s < 60) return s + ' ' + units[3];
    var minutes = Math.floor(s / 60);
    var days = Math.floor(minutes / 1440);
    var hours = Math.floor((minutes % 1440) / 60);
    var parts = [];
    if (days) parts.push(days + ' ' + units[0]);
    if (hours) parts.push(hours + ' ' + units[1]);
    if (minutes % 60) parts.push((minutes % 60) + ' ' + units[2]);
    return parts.join(' ');
}

var uptimeRanges = [[86400, '24 ч'], [3 * 86400, '3 дня'], [7 * 86400, '7 дней']];
var uptimeRange = 7 * 86400;
try { uptimeRange = Number(localStorage.getItem('hifi_uptime_range')) || uptimeRange; } catch (_) {}
var uptimeLabels = {up: 'Токен действует', waiting: 'Без токена', down: 'Аккаунт отключён', unknown: 'Нет данных'};

function setUptimeRange(seconds) {
    uptimeRange = seconds;
    try { localStorage.setItem('hifi_uptime_range', String(seconds)); } catch (_) {}
    refreshUptimeCharts();
}

function uptimeLocale() { return adminLanguage === 'ru' ? 'ru-RU' : 'en-GB'; }
function uptimeDay(ts) { return new Date(ts * 1000).toLocaleDateString(uptimeLocale(), {day:'numeric', month:'short'}); }
function uptimeClock(ts) { return new Date(ts * 1000).toLocaleTimeString(uptimeLocale(), {hour:'2-digit', minute:'2-digit'}); }
function uptimePeriod(from, to) {
    return uptimeDay(from) + ', ' + uptimeClock(from) + ' — ' + (uptimeDay(to) === uptimeDay(from) ? '' : uptimeDay(to) + ', ') + uptimeClock(to);
}

// Local midnights (or every 6 hours for a single day) between start and end.
function uptimeTicks(start, end) {
    var ticks = [], d = new Date(start * 1000);
    var daily = end - start > 36 * 3600;
    d.setMinutes(0, 0, 0);
    if (daily) d.setHours(0); else d.setHours(Math.floor(d.getHours() / 6) * 6);
    while (d.getTime() / 1000 < end) {
        ticks.push(d.getTime() / 1000);
        if (daily) d.setDate(d.getDate() + 1); else d.setHours(d.getHours() + 6);
    }
    return {daily: daily, ticks: ticks};
}

// A real timeline: every segment sits at its own time, so short outages and
// their timing read directly. Hovering shows the exact interval.
function uptimeCard(uptime, accountId) {
    if (!uptime) return '';
    var end = uptime.window_end, start = Math.max(uptime.window_start, end - uptimeRange), span = end - start;
    if (!(span > 0)) return '';
    function pos(ts) { return ((ts - start) * 100 / span).toFixed(3); }
    var totals = {up: 0, waiting: 0, down: 0, unknown: 0}, base = '', marks = '', outages = 0, previous = null;
    (uptime.segments || []).forEach(function(segment) {
        var from = Math.max(start, segment.start), to = Math.min(end, segment.end);
        if (to <= from) return;
        var kind = segment.status || 'unknown';
        totals[kind] += to - from;
        var html = '<i class="uptime-seg uptime-' + kind + '" style="left:' + pos(from) + '%;width:' + pos(start + to - from) + '%"></i>';
        // Problems are drawn last with a minimum width, so even a minute stays visible.
        if (kind === 'waiting' || kind === 'down') {
            marks += html;
            if (previous !== 'waiting' && previous !== 'down') outages++;
        } else base += html;
        previous = kind;
    });
    var axis = uptimeTicks(start, end), grid = '', labels = '';
    axis.ticks.forEach(function(tick, i) {
        if (tick > start) grid += '<i class="uptime-grid" style="left:' + pos(tick) + '%"></i>';
        if (axis.daily) {
            var from = Math.max(tick, start), to = Math.min(axis.ticks[i + 1] || end, end);
            if (to - from > span * 0.07) labels += '<span style="left:' + pos((from + to) / 2) + '%">' + esc(uptimeDay(tick)) + '</span>';
        } else if (tick > start + span * 0.04 && tick < end - span * 0.04) {
            labels += '<span style="left:' + pos(tick) + '%">' + esc(uptimeClock(tick)) + '</span>';
        }
    });
    var known = totals.up + totals.waiting + totals.down;
    var percentage = known ? Number(totals.up * 100 / known).toLocaleString(uptimeLocale(), {minimumFractionDigits:1, maximumFractionDigits:1}) + '%' : '—';
    var tone = !known ? '' : (totals.up / known >= 0.95 ? ' good' : (totals.up / known >= 0.7 ? ' warn' : ' bad'));
    var tabs = uptimeRanges.map(function(range) {
        return '<button type="button" aria-pressed="' + (range[0] === uptimeRange) + '" onclick="setUptimeRange(' + range[0] + ')">' + tr(range[1]) + '</button>';
    }).join('');
    var legend = ['up', 'waiting', 'down', 'unknown'].map(function(kind) {
        if (kind !== 'up' && !totals[kind]) return '';
        return '<span><i class="uptime-key uptime-' + kind + '" aria-hidden="true"></i>' + tr(uptimeLabels[kind]) + ' <strong>' + esc(uptimeDuration(totals[kind])) + '</strong></span>';
    }).join('') + (outages ? '<span title="' + esc(tr('Сколько раз аккаунт переставал быть готовым')) + '">' + tr('Перерывов') + ' <strong>' + outages + '</strong></span>' : '');
    var current = uptime.current_status === 'waiting'
        ? '<span class="uptime-current">' + tr('Нет действующего токена') + '</span>' : '';
    var summary = tr('Доступность токена') + ' ' + percentage;
    return '<div class="uptime-heading"><strong>' + tr('Аптайм') + '</strong><span class="uptime-tabs">' + tabs + '</span>' + current +
        '<span class="uptime-score' + tone + '" title="' + esc(tr('Процент за период с известным статусом')) + '">' + tr('Доступность токена') + '<strong>' + percentage + '</strong></span></div>' +
        '<div class="uptime-track" role="img" aria-label="' + esc(summary) + '" data-start="' + start + '" data-end="' + end + '" data-account="' + esc(accountId || '') + '" onmousemove="uptimeHover(event, this)" onmouseleave="uptimeHoverEnd(this)">' +
            '<div class="uptime-bars">' + base + marks + grid + '</div><i class="uptime-cursor"></i><div class="uptime-tip"></div></div>' +
        '<div class="uptime-labels">' + labels + '</div>' +
        '<div class="uptime-totals">' + legend + '</div>' +
        (totals.unknown > 0 ? '<p class="uptime-note">' + tr('До начала наблюдения история недоступна.') + '</p>' : '');
}

function uptimeHover(event, track) {
    var account = (window._accounts || []).find(function(a) { return a.id === track.dataset.account; });
    if (!account || !account.uptime) return;
    var rect = track.getBoundingClientRect(), x = Math.min(Math.max(event.clientX - rect.left, 0), rect.width);
    var start = Number(track.dataset.start), end = Number(track.dataset.end), perPixel = (end - start) / rect.width;
    var t = start + x * perPixel, hit = null;
    // Prefer a problem segment under the cursor: they are drawn wider than their real length.
    (account.uptime.segments || []).forEach(function(segment) {
        var problem = segment.status === 'waiting' || segment.status === 'down';
        var slack = problem ? 3 * perPixel : 0;
        if (segment.start - slack <= t && t < segment.end + slack && (!hit || problem)) hit = segment;
    });
    if (!hit) return;
    var kind = hit.status || 'unknown', from = Math.max(hit.start, start), to = Math.min(hit.end, end);
    var tip = track.querySelector('.uptime-tip'), cursor = track.querySelector('.uptime-cursor');
    tip.innerHTML = '<b class="uptime-tip-' + kind + '">' + esc(tr(uptimeLabels[kind])) + '</b><span>' + esc(uptimePeriod(from, to)) + '</span><span>' +
        esc(uptimeDuration(to - from)) + (to >= end ? ' · ' + esc(tr('до сих пор')) : '') + '</span>';
    tip.style.left = x + 'px';
    tip.dataset.edge = x < 120 ? 'start' : (x > rect.width - 120 ? 'end' : '');
    cursor.style.left = x + 'px';
    track.classList.add('hover');
}

function uptimeHoverEnd(track) { track.classList.remove('hover'); }

// Ready = enabled with a valid token, from the uptime history of playback
// accounts (catalog-only ones are left out). Per local calendar day: the
// lowest and highest number of ready accounts and the time-weighted mean.
function readyDayStats(accounts, dayStart, dayEnd) {
    var events = [];
    accounts.forEach(function(account) {
        ((account.uptime || {}).segments || []).forEach(function(segment) {
            var from = Math.max(dayStart, segment.start), to = Math.min(dayEnd, segment.end);
            if (to <= from || !segment.status) return;
            events.push([from, segment.status === 'up' ? 1 : 0, 1], [to, segment.status === 'up' ? -1 : 0, -1]);
        });
    });
    events.sort(function(a, b) { return a[0] - b[0]; });
    var ready = 0, known = 0, stats = {min: null, max: null, minAt: null, maxAt: null, sum: 0, observed: 0, series: []};
    for (var i = 0; i < events.length; i++) {
        ready += events[i][1]; known += events[i][2];
        var at = events[i][0];
        if (i + 1 < events.length && events[i + 1][0] === at) continue;
        var next = i + 1 < events.length ? events[i + 1][0] : at;
        stats.series.push([at, known > 0 ? ready : null]);
        if (known <= 0 || next <= at) continue;
        if (stats.min == null || ready < stats.min) { stats.min = ready; stats.minAt = at; }
        if (stats.max == null || ready > stats.max) { stats.max = ready; stats.maxAt = at; }
        stats.sum += ready * (next - at);
        stats.observed += next - at;
    }
    stats.avg = stats.observed ? stats.sum / stats.observed : null;
    return stats;
}

function readySparkline(series, dayStart, top) {
    var width = 288, height = 40, path = '', area = '', open = false, last = null;
    function x(ts) { return ((ts - dayStart) * width / 86400).toFixed(2); }
    function y(n) { return (height - 2 - n * (height - 4) / Math.max(top, 1)).toFixed(2); }
    series.forEach(function(point) {
        if (point[1] == null) {
            if (open) { path += 'H' + x(point[0]); area += 'H' + x(point[0]) + 'V' + height + 'Z'; }
            open = false; return;
        }
        if (!open) { path += 'M' + x(point[0]) + ' ' + y(point[1]); area += 'M' + x(point[0]) + ' ' + height + 'V' + y(point[1]); open = true; }
        else { path += 'H' + x(point[0]) + 'V' + y(point[1]); area += 'H' + x(point[0]) + 'V' + y(point[1]); }
        last = point;
    });
    var grid = [6, 12, 18].map(function(h) { return '<line x1="' + (h * width / 24) + '" x2="' + (h * width / 24) + '" y1="0" y2="' + height + '"></line>'; }).join('');
    return '<svg class="ready-spark" viewBox="0 0 ' + width + ' ' + height + '" preserveAspectRatio="none" aria-hidden="true"><g class="ready-grid">' + grid + '</g>' +
        '<path class="ready-area" d="' + area + '"></path><path class="ready-line" d="' + path + '"></path></svg>';
}

function readyHistoryCard(accounts) {
    var playback = (accounts || []).filter(function(a) { return !a.is_catalog && a.uptime; });
    if (!playback.length) return '';
    var now = Math.max.apply(null, playback.map(function(a) { return a.uptime.window_end; }));
    var today = new Date(now * 1000); today.setHours(0, 0, 0, 0);
    var locale = uptimeLocale(), cards = '';
    for (var back = 0; back < 3; back++) {
        var from = new Date(today); from.setDate(from.getDate() - back);
        var to = new Date(from); to.setDate(to.getDate() + 1);
        var dayStart = from.getTime() / 1000, dayEnd = Math.min(to.getTime() / 1000, now);
        var stats = readyDayStats(playback, dayStart, dayEnd);
        var name = back === 0 ? tr('Сегодня') : (back === 1 ? tr('Вчера') : from.toLocaleDateString(locale, {weekday:'short'}));
        var avg = stats.avg == null ? '—' : stats.avg.toLocaleString(locale, {minimumFractionDigits:1, maximumFractionDigits:1});
        var notes = [];
        if (stats.min != null) notes.push(tr('мин. в') + ' ' + uptimeClock(stats.minAt), tr('макс. в') + ' ' + uptimeClock(stats.maxAt));
        var gap = dayEnd - dayStart - stats.observed;
        if (stats.observed && gap >= 600) notes.push(tr('нет данных') + ' ' + uptimeDuration(gap));
        cards += '<div class="ready-day">' +
            '<div class="ready-day-head"><span>' + esc(name) + '</span><span>' + esc(uptimeDay(dayStart)) + (back === 0 ? ' · ' + tr('до') + ' ' + esc(uptimeClock(now)) : '') + '</span></div>' +
            (stats.observed ? '<div class="ready-nums">' +
                '<div><small>' + tr('Минимум') + '</small><strong class="ready-min">' + stats.min + '</strong></div>' +
                '<div><small>' + tr('Среднее') + '</small><strong>' + avg + '</strong></div>' +
                '<div><small>' + tr('Максимум') + '</small><strong class="ready-max">' + stats.max + '</strong></div></div>' +
                readySparkline(stats.series, dayStart, playback.length) +
                '<div class="ready-foot">' + esc(notes.join(' · ')) + '</div>'
              : '<div class="ready-empty">' + tr('Нет данных за этот день') + '</div>') +
        '</div>';
    }
    return '<div class="ready-history-head"><div><h2>' + tr('Готовые аккаунты по дням') + '</h2><p>' +
        tr('Аккаунты воспроизведения (без «только каталог»), включённые и с действующим токеном. Всего сейчас:') + ' <strong>' + playback.length + '</strong></p></div></div>' +
        '<div class="ready-days">' + cards + '</div>';
}

function refreshUptimeCharts() {
    (window._accounts || []).forEach(function(account) {
        var element = document.getElementById('uptime-' + account.id);
        if (element) element.innerHTML = uptimeCard(account.uptime, account.id);
    });
    var history = document.getElementById('readyHistory');
    if (history) history.innerHTML = readyHistoryCard(window._accounts);
}

function metricNumber(value, digits) {
    if (value == null || !Number.isFinite(Number(value))) return '—';
    return Number(value).toLocaleString(adminLanguage === 'ru' ? 'ru-RU' : 'en-GB', {
        minimumFractionDigits: digits || 0, maximumFractionDigits: digits || 0
    });
}

function accountStatus(account) {
    var state = (account.availability || {}).state;
    if (state === 'ready') return {tone:'status-ok', text:account.is_catalog ? 'Готов к каталогу' : 'Готов к FULL'};
    if (state === 'disabled') return {tone:'status-err', text:account.auto_disabled ? 'Автоотключён' :
        (account.auth_failing_since > 0 ? (String(account.last_refresh_error || '').indexOf('abuse_detected') !== -1 ? 'Отключён: Tidal abuse_detected' : 'Отключён: токен не обновлялся') : 'Отключён вручную')};
    if (state === 'no_token') {
        var token = (account.availability || {}).token;
        return {tone:'status-warn', text:token === 'rejected' ? 'Токен отклонён' : (token === 'expired' ? 'Токен истёк' : 'Нет токена')};
    }
    if (state === 'rate_limited') return {tone:'status-warn', text:'Пауза 429'};
    if (state === 'preview_only') return {tone:'status-warn', text:'Только PREVIEW'};
    if (state === 'needs_check') return {tone:'status-warn', text:'Нужна FULL-проверка'};
    return {tone:'status-unknown', text:'Статус неизвестен'};
}

function tokenDescription(account) {
    var status = (account.availability || {}).token;
    if (status === 'missing') return 'Не получен';
    if (status === 'rejected') return 'Отклонён Tidal';
    return timeStr(account.token_expires_at);
}

// Two-letter country with its flag; Tidal reports it on sign-in and token refresh.
function countryRow(account) {
    var code = String(account.country_code || '').toUpperCase();
    var value = /^[A-Z]{2}$/.test(code)
        ? String.fromCodePoint.apply(null, code.split('').map(function(c) { return 0x1F1E6 + c.charCodeAt(0) - 65; })) + ' ' + esc(code)
        : '<span class="sub-checked">' + esc(tr('появится после обновления токена или проверки подписки')) + '</span>';
    return '<div class="cred-row" data-no-i18n><span class="cred-key">' + esc(tr('Страна')) + '</span><span class="cred-value">' + value + '</span></div>';
}

// Failed manual lookups, kept until the page reloads; answers are stored on the server.
var _subscriptions = {};

// Plans ending within this many days are highlighted and counted above the list.
var SUBSCRIPTION_SOON_DAYS = 3;

function subscriptionPlan(body) {
    return String(((body || {}).subscription || {}).type || (body || {}).type || '');
}

function subscriptionDaysLeft(body) {
    var until = body && body.validUntil ? Date.parse(String(body.validUntil).replace(/([+-]\d\d)(\d\d)$/, '$1:$2')) : NaN;
    return isNaN(until) ? NaN : (until - Date.now()) / 86400000;
}

function subscriptionTime(seconds) {
    var at = new Date(seconds * 1000);
    var sameDay = at.toDateString() === new Date().toDateString();
    return at.toLocaleString(adminLanguage === 'ru' ? 'ru-RU' : 'en-GB', sameDay ? {hour:'2-digit', minute:'2-digit'} : {day:'numeric', month:'short', hour:'2-digit', minute:'2-digit'});
}

// The last good subscription answer: checked daily, after a token refresh, or
// on demand. A failed check never hides it, it is only noted next to it.
function subscriptionRow(account) {
    var body = account.subscription;
    var checkedAt = Number(account.subscription_checked_at) || 0;
    var failure = account.subscription_error ? { error: account.subscription_error, at: Number(account.subscription_error_at) || 0 } : null;
    var manual = _subscriptions[account.id];
    if (manual && (!failure || manual.checked_at > failure.at)) failure = { error: manual.error, at: manual.checked_at };
    if (failure && body && checkedAt >= failure.at) failure = null;
    if (!body && !failure) return '';
    var html = '';
    if (body) {
        var plan = subscriptionPlan(body);
        var parts = [];
        if (plan) parts.push('<strong class="' + (plan.toUpperCase() === 'FREE' ? 'sub-bad' : '') + '">' + esc(plan) + '</strong>');
        if (body.status) parts.push('<span class="' + (body.status === 'ACTIVE' ? 'sub-ok' : 'sub-bad') + '">' + esc(body.status) + '</span>');
        var daysLeft = subscriptionDaysLeft(body);
        if (!isNaN(daysLeft)) {
            var date = new Date(Date.now() + daysLeft * 86400000).toLocaleDateString(adminLanguage === 'ru' ? 'ru-RU' : 'en-GB', {day:'numeric', month:'short', year:'numeric'});
            parts.push('<span class="' + (daysLeft < SUBSCRIPTION_SOON_DAYS ? 'sub-bad' : '') + '">' + esc(tr('до')) + ' ' + esc(date) + '</span>');
            if (daysLeft > 0 && daysLeft < SUBSCRIPTION_SOON_DAYS) parts.push('<span class="sub-bad">' + esc(tr('заканчивается через')) + ' ' + esc(uptimeDuration(daysLeft * 86400)) + '</span>');
        }
        if (body.highestSoundQuality) parts.push(esc(body.highestSoundQuality));
        if (body.paymentOverdue) parts.push('<span class="sub-bad">' + esc(tr('оплата просрочена')) + '</span>');
        if (!parts.length) parts.push('<code>' + esc(JSON.stringify(body).slice(0, 200)) + '</code>');
        html = parts.join(' · ') + ' <span class="sub-checked">· ' + esc(tr('проверено в')) + ' ' + esc(subscriptionTime(checkedAt || Date.now() / 1000)) + '</span>';
    }
    if (failure) {
        html += body
            ? ' <span class="sub-bad" title="' + esc(failure.error) + '">· ' + esc(tr('последняя проверка не удалась')) + ' ' + esc(subscriptionTime(failure.at)) + '</span>'
            : '<span class="sub-bad">' + esc(tr('Не удалось проверить:')) + ' ' + esc(failure.error) + '</span> <span class="sub-checked">· ' + esc(subscriptionTime(failure.at)) + '</span>';
    }
    return '<div class="cred-row" data-no-i18n><span class="cred-key">' + esc(tr('Подписка')) + '</span><span class="cred-value">' + html + '</span></div>';
}

// Counters above the account list: FREE plans, plans ending soon, plans that
// ended or are overdue, and playback accounts without any answer yet.
function subscriptionSummary(accounts) {
    var groups = { free: [], soon: [], ended: [], unknown: [] };
    accounts.forEach(function(a) {
        var label = a.label || a.id.slice(0, 8);
        var body = a.subscription;
        if (!body) {
            if (!a.is_catalog && a.is_active) groups.unknown.push(label);
            return;
        }
        var daysLeft = subscriptionDaysLeft(body);
        if (subscriptionPlan(body).toUpperCase() === 'FREE') groups.free.push(label);
        else if (body.paymentOverdue || (body.status && body.status !== 'ACTIVE') || daysLeft <= 0) groups.ended.push(label);
        else if (daysLeft < SUBSCRIPTION_SOON_DAYS) groups.soon.push(label);
    });
    function chip(names, text, always) {
        if (!names.length && !always) return '';
        return '<div class="account-summary' + (names.length ? ' warn' : ' calm') + '"' + (names.length ? ' title="' + esc(names.join(', ')) + '"' : '') + '><span>' + esc(tr(text)) + '</span><strong>' + metricNumber(names.length) + '</strong></div>';
    }
    return chip(groups.free, 'FREE-подписка', true) +
        chip(groups.soon, 'Подписка кончается за ' + SUBSCRIPTION_SOON_DAYS + ' дн.', true) +
        chip(groups.ended, 'Подписка закончилась', false) +
        chip(groups.unknown, 'Подписка не проверена', false);
}

// How long Tidal has answered 403 to token refreshes, and when (or that)
// the account is switched off for it.
function authFailureDetails(account) {
    var since = Number(account.auth_failing_since) || 0;
    if (since <= 0) return '';
    var now = Date.now() / 1000;
    var text;
    if (!account.is_active && !account.auto_disabled) {
        text = tr('Аккаунт выключен: Tidal отвечал 403 на обновление токена') + ' <strong>' + esc(uptimeDuration((Number(account.disabled_at) || now) - since)) + '</strong>. ' +
            tr('Попыток больше нет; включите аккаунт, когда он снова заработает.');
    } else {
        text = tr('Tidal отвечает 403 на обновление токена уже') + ' <strong>' + esc(uptimeDuration(now - since)) + '</strong>' +
            (account.auth_disable_at ? '; ' + tr('аккаунт выключится через') + ' <strong>' + esc(uptimeDuration(Math.max(0, account.auth_disable_at - now))) + '</strong>' : '') + '.';
    }
    return '<div class="token-refresh-error" data-no-i18n>' + text + '</div>';
}

function tokenRefreshDetails(account) {
    var token = (account.availability || {}).token;
    var coolingDown = account.heal_next_retry > Date.now() / 1000;
    var streak = authFailureDetails(account);
    if (token === 'valid' && !coolingDown && !account.auto_disabled) return streak;
    var reason = account.last_refresh_error;
    if (reason) {
        return '<div class="token-refresh-error" data-no-i18n>' + tr('Обновление токена не удалось:') + ' <strong>' + esc(reason) + '</strong></div>' + streak;
    }
    var description = token === 'expired' ? 'Срок действия access token закончился.' :
        (token === 'rejected' ? 'Tidal отклонил access token.' : '');
    return (description ? '<div class="token-refresh-error" data-no-i18n>' + tr(description) + '</div>' : '') + streak;
}

function metricPair(label, value) {
    return '<span>' + esc(label) + '</span> <strong>' + esc(metricNumber(value)) + '</strong>';
}

function metricCard(label, value, details, title, body, layout) {
    return '<div class="stat-card' + (layout ? ' ' + layout : '') + '"' + (title ? ' title="' + esc(title) + '"' : '') + '><div class="label">' + esc(label) + '</div>' +
        '<div class="value">' + value + '</div>' + (body || '') + '<div class="stat-details">' + (details || []).map(function(detail) {
            return '<div>' + detail + '</div>';
        }).join('') + '</div></div>';
}

function metricRow(label, value, tone) {
    return '<div class="stat-row">' + (tone ? '<span class="status-dot ' + tone + '"></span>' : '') +
        '<span>' + esc(label) + '</span><strong>' + value + '</strong></div>';
}

function metricRows(rows) {
    return rows.length ? '<div class="stat-rows">' + rows.join('') + '</div>' : '';
}

function metricStatus(label, tone) {
    return '<span class="metric-text"><span class="status-dot ' + tone + '"></span><span>' + esc(label) + '</span></span>';
}

function overviewGroup(title, columns, cards) {
    return '<section><h2 class="ov-title">' + esc(title) + '</h2><div class="ov-grid ov-' + columns + '">' + cards + '</div></section>';
}

function accountsCard(stats) {
    var counts = stats.account_readiness || {};
    var playback = stats.playback_readiness || {}, catalog = stats.catalog_readiness || {};
    var total = counts.total || 0, ready = counts.ready || 0, disabled = counts.disabled || 0;
    var rows = [metricRow('Готовы к FULL', esc(metricNumber(playback.ready)), 'status-ok')];
    if (catalog.total > 0) rows.push(metricRow('Готовы к каталогу', esc(metricNumber(catalog.ready)), 'status-ok'));
    [['no_token', 'Без токена'], ['needs_check', 'Нужна FULL-проверка'], ['preview_only', 'Только PREVIEW'], ['rate_limited', 'Пауза 429']].forEach(function(item) {
        if (counts[item[0]]) rows.push(metricRow(item[1], esc(metricNumber(counts[item[0]])), 'status-warn'));
    });
    if (disabled) rows.push(metricRow('Отключены', esc(metricNumber(disabled)), 'status-err'));
    return metricCard('Готовые аккаунты', esc(metricNumber(ready)) + ' <span class="metric-unit"><span>из</span> ' + esc(metricNumber(total)) + '</span>',
        total === 0 ? ['Добавьте аккаунт для доступа к Tidal.'] : [],
        'Готовность: действующий токен и отсутствие паузы 429. Для воспроизведения нужна подтверждённая FULL-проверка не старше 8 часов.',
        total > 0 ? metricRows(rows) : '');
}

function playbackCard(pb, readiness) {
    pb = pb || {}; readiness = readiness || {};
    // `limit` is null when the per-account limit is off; older servers only send pool_size.
    var unlimited = pb.limit === null, limit = unlimited ? 0 : (pb.limit != null ? pb.limit : (pb.pool_size || 0)), active = pb.active || 0;
    var meter = '<div class="stat-bar stat-meter" aria-hidden="true"><i style="width:' +
        (limit > 0 ? Math.min(100, active * 100 / limit) : 0).toFixed(1) + '%"></i></div>';
    var rows = [metricRow('Лимит одновременно', unlimited ? 'без лимита' : esc(metricNumber(limit))), metricRow('В очереди', esc(metricNumber(pb.pending)))];
    if (pb.pending > 0) rows.push(metricRow('Самое долгое ожидание', esc(uptimeDuration(pb.oldest_pending_secs))));
    var note = '';
    if (readiness.enabled === 0) note = '<span>Нет включённых аккаунтов воспроизведения.</span>';
    else if ((pb.pool_size || 0) < readiness.enabled) note = '<span>Обслуживать запросы могут</span> <strong>' + esc(metricNumber(readiness.metadata_ready)) +
        '</strong> <span>из</span> <strong>' + esc(metricNumber(readiness.enabled)) + '</strong> <span>включённых аккаунтов; остальные без токена или на паузе 429.</span>';
    return metricCard('Запросы воспроизведения', esc(metricNumber(pb.active)) + ' <span class="metric-unit">выполняются</span>', [],
        'Запросы треков, видео и лицензий, выполняемые в момент обновления. Лимит — число аккаунтов воспроизведения с действующим токеном и без паузы 429, умноженное на число одновременных запросов на аккаунт из настроек.',
        meter + metricRows(rows) + (note ? '<div class="stat-note">' + note + '</div>' : ''));
}

function catalogCard(cat, playback, catalog) {
    cat = cat || {}; playback = playback || {}; catalog = catalog || {};
    var mode = cat.mode || 'pool';
    var label = mode === 'static_token' ? 'Статичный токен' : (mode === 'account' ? 'Аккаунты каталога' : 'Пул воспроизведения');
    var details, tone = 'status-unknown';
    if (mode === 'static_token') {
        details = ['Токен настроен; доступ проверяется при запросе.', metricPair('Резервных аккаунтов каталога:', catalog.enabled)];
    } else {
        var source = mode === 'account' ? catalog : playback;
        tone = !source.metadata_ready ? 'status-err' : (source.metadata_ready < source.enabled ? 'status-warn' : 'status-ok');
        details = [metricPair('С действующим токеном:', source.metadata_ready) + ' <span>из</span> ' + esc(metricNumber(source.enabled)) + ' <span>включённых</span>',
            'Для метаданных FULL-проверка не требуется.'];
        if (!source.enabled) details.push('Нет включённых аккаунтов для метаданных.');
        else if (!source.metadata_ready) details.push('Нет готовых токенов; требуется обновление или завершение паузы.');
        if (mode === 'pool' && catalog.total > 0) details.push('Аккаунты каталога отключены или на паузе 429.');
    }
    return metricCard('Источник метаданных', metricStatus(label, tone), details,
        'Приоритет источников: статичный токен → аккаунты каталога → пул воспроизведения.');
}

function redisCard(redis) {
    redis = redis || {};
    var label = !redis.configured ? 'Не настроена' : (redis.status === 'ok' ? 'Redis доступен' : 'Нет связи с Redis');
    var tone = !redis.configured ? 'status-unknown' : (redis.status === 'ok' ? 'status-ok' : 'status-err');
    var detail = !redis.configured ? 'Общие данные между серверами не синхронизируются.' :
        (redis.status === 'ok' ? 'Redis отвечает; общие данные синхронизируются.' : 'Сервис использует локальное состояние.');
    var details = [detail];
    if (redis.endpoint) details.push('<span data-no-i18n>' + esc(redis.endpoint) + '</span>');
    return metricCard('Синхронизация данных', metricStatus(label, tone), details);
}

// Filled by loadProxyStatus(); the overview re-renders on every stats refresh.
var overviewProxy = null;

function proxyCard(proxy) {
    if (!proxy) return metricCard('Прокси', '<span class="metric-text">—</span>', []);
    if (!proxy.enabled) return metricCard('Прокси', metricStatus(proxy.status, proxy.tone), ['Запросы к Tidal идут без прокси.']);
    return metricCard('Прокси', metricStatus(proxy.status, proxy.tone), [], '',
        metricRows([metricRow('Проверено назначений', esc(metricNumber(proxy.verified)) + ' / ' + esc(metricNumber(proxy.assigned))),
            metricRow('В пуле', esc(metricNumber(proxy.pool_size)))]));
}

// Stacking order of the per-second chart; the rate table doubles as its legend.
var rateSeries = [['playback', 'Воспроизведение'], ['search', 'Поиск'], ['catalog', 'Альбомы, артисты…']];

function rateChart(history) {
    if (!history || !history.length) return '';
    var peak = Math.max.apply(null, history.map(function(second) { return second[0] + second[1] + second[2]; }));
    var bars = history.map(function(second, index) {
        var tip = (history.length - index) + ' ' + tr('с назад');
        var spans = '';
        rateSeries.forEach(function(series, kind) {
            if (second[kind] > 0) spans += '<i class="rate-' + series[0] + '" style="height:' + (second[kind] * 100 / peak).toFixed(1) + '%"></i>';
            tip += '\n' + tr(series[1]) + ': ' + second[kind];
        });
        var edge = index < 8 ? ' data-edge="start"' : (index >= history.length - 8 ? ' data-edge="end"' : '');
        return '<div class="rate-bar" data-tip="' + esc(tip) + '"' + edge + '>' + spans + '</div>';
    }).join('');
    return '<div class="rate-chart" data-no-i18n role="img" aria-label="' + esc(tr('Запросы к Tidal по секундам за последние 60 секунд')) + '">' + bars + '</div>' +
        '<div class="uptime-axis"><span>60 с назад</span><span>Сейчас</span></div>';
}

function rateCard(rates, stats) {
    rates = rates || {}; stats = stats || {};
    var unit = ' <span class="rate-unit">/с</span>';
    var cells = '<span></span><span class="rate-head">Сейчас</span><span class="rate-head">60 с</span><span class="rate-head">Пик</span>' +
        [['Всего', rates.all, ''], ['Воспроизведение', rates.playback, 'rate-playback'], ['Поиск', rates.search, 'rate-search'],
            ['Альбомы, артисты…', rates.catalog, 'rate-catalog'], ['Из кэша', rates.cached, 'rate-cached']].map(function(row) {
            var rate = row[1] || {};
            return '<span>' + (row[2] ? '<i class="rate-key ' + row[2] + '" aria-hidden="true"></i>' : '') + esc(row[0]) + '</span><strong>' +
                esc(metricNumber(rate.now, 2)) + '</strong><strong>' + esc(metricNumber(rate.avg_60s, 2)) + '</strong><strong>' +
                esc(metricNumber(rate.peak_60s)) + '</strong>';
        }).join('');
    // Load is spread over the accounts able to serve right now, which is what Tidal limits.
    var mode = (stats.catalog || {}).mode;
    var metadataAccounts = mode === 'static_token' ? 0 : ((mode === 'account' ? stats.catalog_readiness : stats.playback_readiness) || {}).metadata_ready;
    function perAccount(label, rate, accounts) {
        return accounts > 0 && rate != null ? [metricRow(label, esc(metricNumber(rate / accounts, 2)) + unit)] : [];
    }
    var limits = perAccount('Воспроизведение на один аккаунт', (rates.playback || {}).avg_60s, (stats.playback || {}).pool_size).concat(
        perAccount('Метаданные на один аккаунт', rates.search && rates.catalog ? rates.search.avg_60s + rates.catalog.avg_60s : null, metadataAccounts));
    if (rates.throttled_60s != null) limits.push(metricRow('Ответов 429 за 60 с', esc(metricNumber(rates.throttled_60s)), rates.throttled_60s > 0 ? 'status-warn' : ''));
    return metricCard('Запросы к Tidal в секунду', esc(metricNumber((rates.all || {}).now, 2)) + ' <span class="metric-unit">сейчас</span>',
        ['Считаются только запросы, отправленные в Tidal: без ответов из кэша, опроса очереди и служебных маршрутов. «Сейчас» — среднее за последние 5 секунд, пик — самая загруженная секунда из последних 60, у каждой строки своя.',
            'Нагрузка на один аккаунт — среднее за 60 секунд, делённое на число аккаунтов, которые сейчас могут обслуживать такие запросы.'], '',
        rateChart(rates.history) + '<div class="rate-cols"><div class="stat-rows rate-rows">' + cells + '</div>' + metricRows(limits) + '</div>', 'stat-wide stat-tall');
}

// Tracks are timed from arrival to a ready answer, queue wait included;
// search and albums come from the request log, without cache hits.
function latencyCard(stats, noRequests) {
    var track = (stats.playback || {}).track_latency || {}, latency = stats.latency || {};
    var ms = function(value) { return value == null ? '—' : metricNumber(value); };
    var rows = [['Треки', track], ['Поиск', latency.search || {}], ['Альбомы, артисты…', latency.catalog || {}]];
    var hasTracks = track.p95_ms != null;
    var headline = hasTracks ? track.p95_ms : stats.recent_p95_ms;
    var value = esc(ms(headline)) + (headline != null ? ' <span class="metric-unit">' + (hasTracks ? 'мс · p95 треков' : 'мс · p95 всех запросов') + '</span>' : '');
    var table = '<div class="stat-rows rate-rows"><span></span><span class="rate-head">p50</span><span class="rate-head">p95</span><span class="rate-head">p99</span>' +
        rows.map(function(row) {
            return '<span>' + esc(row[0]) + '</span><strong>' + esc(ms(row[1].p50_ms)) + '</strong><strong>' + esc(ms(row[1].p95_ms)) + '</strong><strong>' + esc(ms(row[1].p99_ms)) + '</strong>';
        }).join('') + '</div>';
    var details = [];
    if (hasTracks) {
        details.push('<span>Самый долгий трек</span> <strong>' + esc(ms(track.max_ms)) + '</strong> <span>мс</span>');
        if (track.queued > 0) details.push('<span>Ждали в очереди</span> <strong>' + esc(metricNumber(track.queued)) + '</strong> <span>из</span> <strong>' +
            esc(metricNumber(track.count)) + '</strong>, <span>p95 ожидания</span> <strong>' + esc(ms(track.wait_p95_ms)) + '</strong> <span>мс</span>');
    } else if (!(stats.recent_requests > 0)) details.push(noRequests);
    return metricCard('Время ответа', value, details,
        'Треки — от получения запроса до готового ответа, включая ожидание в очереди; последние 1000 успешных запросов треков, манифестов и DASH. Поиск и альбомы — успешные ответы Tidal без кэша среди последних 5000 запросов. Значения в миллисекундах.',
        table);
}

function overviewCards(stats) {
    var sample = stats.recent_requests;
    var noRequests = 'Пока нет завершённых запросов.';
    var errorValue = sample > 0 ? metricNumber(stats.recent_error_rate_percent, 2) + '%' : '—';
    return overviewGroup('Трафик', 4,
            rateCard(stats.request_rates, stats) +
            latencyCard(stats, noRequests) +
            metricCard('Ответы с ошибками', esc(errorValue),
                [sample > 0 ? '<strong>' + esc(metricNumber(stats.total_errors)) + '</strong> <span>из</span> <strong>' + esc(metricNumber(sample)) + '</strong> <span>последних запросов</span>' : noRequests],
                'Доля ответов HTTP 4xx/5xx, включая 404 и 429, среди последних завершённых запросов. Журнал хранит до 5000 записей.') +
            metricCard('Завершённые API-запросы', esc(metricNumber(stats.total_requests)), ['С момента запуска сервера.'],
                'Один входящий API-запрос считается один раз, включая внутренние повторы через другие аккаунты.', '', 'stat-wide')) +
        overviewGroup('Аккаунты и воспроизведение', 2,
            accountsCard(stats) + playbackCard(stats.playback, stats.playback_readiness)) +
        overviewGroup('Инфраструктура', 3,
            catalogCard(stats.catalog, stats.playback_readiness, stats.catalog_readiness) + redisCard(stats.redis) + proxyCard(overviewProxy));
}

// Cards whose markup did not change keep their DOM, so frequent refreshes do not
// reset hover tooltips on cards that are standing still.
var overviewCardHtml = [];

function renderOverview(stats) {
    var root = document.getElementById('stats');
    var fresh = document.createElement('div');
    fresh.innerHTML = overviewCards(stats);
    var next = Array.prototype.slice.call(fresh.querySelectorAll('.stat-card'));
    var current = root.querySelectorAll('.stat-card');
    var html = next.map(function(card) { return card.outerHTML; });
    if (current.length !== next.length || overviewCardHtml.length !== next.length) {
        root.innerHTML = fresh.innerHTML;
    } else {
        next.forEach(function(card, index) {
            if (overviewCardHtml[index] === html[index]) return;
            // A chart under the pointer waits for it to leave, so the tooltip being read stays put.
            if (current[index].querySelector('.rate-bar:hover')) html[index] = overviewCardHtml[index];
            else current[index].replaceWith(card);
        });
    }
    overviewCardHtml = html;
}

var statsRefreshing = false;

async function refreshStats() {
    if (statsRefreshing) return;
    statsRefreshing = true;
    try {
        var res = await fetch('/admin/stats', { headers: headers() });
        if (res.status === 401) { setKey(); return; }
        if (!res.ok) return;
        window._stats = await res.json();
        renderOverview(window._stats);
    } catch(e) {
    } finally {
        statsRefreshing = false;
    }
}

var _testResults = {};
var _testCacheTs = 0;

async function testAll() {
    var now = Math.floor(Date.now() / 1000);
    if (now - _testCacheTs < 30 && Object.keys(_testResults).length > 0) {
        renderTestResults(_testResults);
        return;
    }
    var btn = document.getElementById('testAllBtn');
    btn.textContent = 'Проверяем…';
    btn.disabled = true;
    try {
        var res = await fetch('/admin/accounts/test-all', { method: 'POST', headers: headers() });
        if (!res.ok) { document.getElementById('error').textContent = 'Проверка не удалась: HTTP ' + res.status; return; }
        var data = await res.json();
        _testResults = {};
        for (var r of data.results) { _testResults[r.id] = r; }
        _testCacheTs = now;
        renderTestResults(_testResults);
    } catch(e) {
        document.getElementById('error').textContent = 'Ошибка проверки: ' + e.message;
    } finally {
        btn.textContent = 'Проверить все';
        btn.disabled = false;
    }
}

function renderTestResults(results) {
    var ids = Object.keys(results);
    var section = document.getElementById('testResultsSection');
    if (ids.length === 0) { section.style.display = 'none'; return; }
    section.style.display = 'block';
    var pass = 0, fail = 0;
    for (var id in results) { if (results[id].ok) pass++; else fail++; }
    document.getElementById('testSummary').textContent = 'Успешно: ' + pass + ' · Ошибок: ' + fail + ' · Нажмите на строку для деталей';
    var html = '';
    for (var i = 0; i < ids.length; i++) {
        var id = ids[i];
        var r = results[id];
        var acc = window._accounts.find(function(x) { return x.id === id; });
        var label = acc ? esc(acc.label || id.slice(0, 8)) : id.slice(0, 8);
        var statusClass = r.ok ? 'test-pass' : 'test-fail';
        var statusText = r.ok ? 'PASS' : 'FAIL';
        var msText = r.ms ? r.ms + 'ms' : '-';
        var httpText = r.status_code || '-';
        var tokenStr = r.token_expires_at ? timeStr(r.token_expires_at) : '-';
        html += '<div class="test-result-row" onclick="showTestDetails(\'' + id + '\')">';
        html += '<span class="result-label">' + label + '</span>';
        html += '<span class="result-status ' + statusClass + '">' + statusText + '</span>';
        html += '<span class="result-http">' + httpText + '</span>';
        html += '<span class="result-ms">' + msText + '</span>';
        html += '<span class="result-token">' + tokenStr + '</span>';
        html += '</div>';
        var badge = document.getElementById('test-' + id);
        if (badge) {
            badge.className = r.ok ? 'card-stat test-badge test-pass' : 'card-stat test-badge test-fail';
            badge.innerHTML = '<span>Последний тест</span> <strong>' + (r.ok ? 'OK ' + r.ms + ' мс' : esc(testErrorSummary(r.error))) + '</strong>';
            badge.title = tr('Результат ручной проверки. Подробности по нажатию; текущее состояние — в заголовке аккаунта.');
        }
    }
    document.getElementById('testResultsList').innerHTML = html;
}

function testErrorSummary(error) {
    var text = String(error || '');
    if (/cooling down/i.test(text)) return 'Ожидание обновления токена';
    if (/RateLimited|429/i.test(text)) return 'Пауза 429';
    if (/Unauthorized|TokenError|invalid_grant/i.test(text)) return 'Ошибка авторизации';
    if (/Timeout|timed out/i.test(text)) return 'Нет ответа вовремя';
    return 'Ошибка проверки';
}

function formatJsonString(str) {
    var indent = 0, result = '', inStr = false;
    for (var i = 0; i < str.length; i++) {
        var ch = str[i];
        if (inStr) {
            result += ch;
            if (ch === '\\' && i + 1 < str.length) { result += str[++i]; }
            else if (ch === '"') { inStr = false; }
            continue;
        }
        if (ch === '"') { inStr = true; result += ch; continue; }
        if (ch === '{' || ch === '[') {
            indent++;
            result += ch + '\n' + '  '.repeat(indent);
            continue;
        }
        if (ch === '}' || ch === ']') {
            indent = Math.max(0, indent - 1);
            result += '\n' + '  '.repeat(indent) + ch;
            continue;
        }
        if (ch === ',') { result += ch + '\n' + '  '.repeat(indent); continue; }
        if (ch === ':') { result += ': '; continue; }
        result += ch;
    }
    return esc(result);
}

function highlightJsonString(str) {
    var html = esc(str);
    return html.replace(
        /("(?:\\.|[^"\\])*")\s*:|("(?:\\.|[^"\\])*")|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|(\btrue\b|\bfalse\b)|(\bnull\b)|([{}[\]])/g,
        function(m, key, str, num, bool, nul, bracket) {
            if (key) return '<span class="json-key">' + key + '</span>:';
            if (str) return '<span class="json-string">' + str + '</span>';
            if (num) return '<span class="json-number">' + num + '</span>';
            if (bool) return '<span class="json-boolean">' + bool + '</span>';
            if (nul) return '<span class="json-null">' + nul + '</span>';
            if (bracket) return '<span class="json-bracket">' + bracket + '</span>';
            return m;
        }
    );
}

function renderResponsePreview(preview) {
    if (!preview) return '';
    if (typeof preview === 'object') return highlightJsonString(JSON.stringify(preview, null, 2));
    if (typeof preview !== 'string') return esc(String(preview));
    try { var parsed = JSON.parse(preview); return highlightJsonString(JSON.stringify(parsed, null, 2)); }
    catch(_) {
        var cleaned = preview.replace(/\.\.\.\s*$/, '').trim();
        try { var parsed = JSON.parse(cleaned); return highlightJsonString(JSON.stringify(parsed, null, 2)); }
        catch(_2) { return formatJsonString(preview).replace(/\n/g, '<br>').replace(/  /g, '&nbsp;&nbsp;'); }
    }
}

function showTestDetails(id) {
    var r = _testResults[id];
    if (!r) { document.getElementById('error').textContent = 'Сначала запустите проверку аккаунтов.'; return; }
    var label = 'Unknown';
    var acc = window._accounts.find(function(x) { return x.id === id; });
    if (acc) label = esc(acc.label || acc.id.slice(0, 8));
    document.getElementById('testTitle').textContent = 'Проверка: ' + (acc ? acc.label || acc.id.slice(0, 8) : id.slice(0, 8));
    var html = '';
    html += '<div class="cred-row"><span class="cred-key">Status</span><span class="cred-value ' + (r.ok ? 'test-pass' : 'test-fail') + '"><strong>' + (r.ok ? 'PASS' : 'FAIL') + '</strong></span></div>';
    html += '<div class="cred-row"><span class="cred-key">HTTP Status</span><span class="cred-value">' + (r.status_code || '-') + '</span></div>';
    html += '<div class="cred-row"><span class="cred-key">Response Time</span><span class="cred-value">' + r.ms + 'ms</span></div>';
    html += '<div class="cred-row"><span class="cred-key">Token Expiry</span><span class="cred-value">' + timeStr(r.token_expires_at) + '</span></div>';
    html += '<div class="cred-row"><span class="cred-key">Active</span><span class="cred-value">' + (r.is_active ? 'Yes' : 'No') + '</span></div>';
    if (r.error) {
        html += '<div class="cred-row" style="margin-top:12px"><span class="cred-key">Error</span><span class="cred-value test-fail">' + esc(r.error) + '</span></div>';
    }
    var raw = r.response_body || r.response || r.response_preview;
    if (raw) {
        var pretty = renderResponsePreview(raw);
        html += '<div style="margin-top:16px;padding-top:12px;border-top:1px solid #30363d"><span class="cred-key" style="display:block;margin-bottom:8px">Full JSON Response</span>';
        html += '<pre style="background:#0d1117;border:1px solid #30363d;border-radius:6px;padding:12px;overflow-x:auto;white-space:pre-wrap;word-break:break-word;color:#c9d1d9;font-size:11px">' + pretty + '</pre></div>';
    }
    document.getElementById('testDetailsContent').innerHTML = html;
    document.getElementById('testOverlay').classList.add('open');
}

function closeTestDetails() {
    document.getElementById('testOverlay').classList.remove('open');
}

function trunc(s, n) {
    if (!s) return '';
    n = n || 40;
    return s.length > n ? s.slice(0, n) + '...' : s;
}

function openEdit(id) {
    editId = id;
    var a = window._accounts.find(function(x) { return x.id === id; });
    if (!a) return;
    document.getElementById('ed-label').value = a.label || '';
    document.getElementById('ed-user-id').value = a.user_id || '';
    document.getElementById('ed-client-id').value = a.client_id || '';
    document.getElementById('ed-client-secret').value = a.client_secret || '';
    document.getElementById('ed-refresh-token').value = a.refresh_token || '';
    document.getElementById('editTitle').textContent = 'Изменить «' + (a.label || a.id.slice(0, 8)) + '»';
    document.getElementById('editOverlay').classList.add('open');
}

function closeEdit() {
    editId = null;
    document.getElementById('editOverlay').classList.remove('open');
}

async function saveEdit() {
    var id = editId;
    if (!id) return;
    var body = {
        label: document.getElementById('ed-label').value,
        user_id: document.getElementById('ed-user-id').value || null,
        client_id: document.getElementById('ed-client-id').value,
        client_secret: document.getElementById('ed-client-secret').value,
        refresh_token: document.getElementById('ed-refresh-token').value,
    };
    try {
        var res = await fetch('/admin/accounts/' + id, {
            method: 'PATCH', headers: headers(), body: JSON.stringify(body)
        });
        if (res.ok) {
            closeEdit();
            fetchData();
        } else {
            var d = await res.json();
            document.getElementById('error').textContent = d.detail || 'Error';
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function fetchData() {
    try {
        var [statsRes, accountsRes] = await Promise.all([
            fetch('/admin/stats', { headers: headers() }),
            fetch('/admin/accounts', { headers: headers() })
        ]);
        if (statsRes.status === 401 || accountsRes.status === 401) { setKey(); return false; }

        if (!statsRes.ok) {
            var text = await statsRes.text();
            document.getElementById('error').textContent = 'Stats: ' + statsRes.status + ' ' + text.slice(0, 200);
            return;
        }
        if (!accountsRes.ok) {
            var text = await accountsRes.text();
            document.getElementById('error').textContent = 'Accounts: ' + accountsRes.status + ' ' + text.slice(0, 200);
            return;
        }

        var statsText = await statsRes.text();
        var accountsText = await accountsRes.text();
        var stats, accounts;
        try { stats = JSON.parse(statsText); } catch(e) { document.getElementById('error').textContent = 'Stats parse: ' + statsText.slice(0, 200); return; }
        try { accounts = JSON.parse(accountsText); } catch(e) { document.getElementById('error').textContent = 'Accounts parse: ' + accountsText.slice(0, 200); return; }
        window._accounts = accounts.accounts;

        window._stats = stats;
        renderOverview(stats);

        var html = '';
        var groupCards = { playback: '', catalog: '' };
        var groupSize = { playback: 0, catalog: 0 };
        if (accounts.accounts.length === 0) {
            html = '<div class="empty-state"><p>Аккаунтов пока нет</p><p class="hint">Подключите первый через OAuth — это займёт меньше минуты.</p><button class="btn btn-primary" style="margin-top:16px" onclick="openAddAccount()">Добавить аккаунт</button></div>';
        } else {
            for (var i = 0; i < accounts.accounts.length; i++) {
                var a = accounts.accounts[i];
                var group = a.is_catalog ? 'catalog' : 'playback';
                var number = ++groupSize[group];
                var label = a.label || a.id.slice(0, 8);
                var readiness = accountStatus(a);
                var statusClass = 'status-dot ' + readiness.tone;
                var statusText = readiness.text;
                var activeCls = a.is_active ? ' btn-active' : '';
                var toggleText = a.is_active ? 'Включён' : 'Включить';
                var tokenStr = tokenDescription(a);
                var disabledAt = !a.is_active && Number(a.disabled_at) > 0 ? Number(a.disabled_at) : 0;
                var disabledSince = disabledAt
                    ? new Date(disabledAt * 1000).toLocaleString(adminLanguage === 'ru' ? 'ru-RU' : 'en-GB', { day:'2-digit', month:'2-digit', year:'numeric', hour:'2-digit', minute:'2-digit' })
                    : 'время неизвестно';
                var uid = a.user_id || '-';
                var catalogBadge = a.is_catalog ? '<span class="status-label" style="color:#d2a8ff;background:rgba(210,168,255,0.1)">Каталог</span>' : '';
                var premiumLabel, premiumClass, premiumDescription;
                if (a.is_catalog) {
                    premiumLabel = 'НЕ ПРИМЕНЯЕТСЯ';
                    premiumClass = 'access-catalog';
                    premiumDescription = 'Аккаунт используется только для каталога';
                } else if (a.premium_status === 'premium') {
                    var freshFull = a.availability && a.availability.full_check_fresh;
                    premiumLabel = freshFull ? 'FULL' : 'FULL · УСТАРЕЛО';
                    premiumClass = freshFull ? 'access-full' : 'access-unknown';
                    premiumDescription = freshFull ? 'Полное воспроизведение подтверждено' : 'FULL подтверждался ранее; нужна новая проверка';
                } else if (a.premium_status === 'preview-only') {
                    premiumLabel = 'PREVIEW';
                    premiumClass = 'access-preview';
                    premiumDescription = 'Доступны только фрагменты';
                } else if (a.premium_checked_at) {
                    premiumLabel = 'НЕ ОПРЕДЕЛЕНО';
                    premiumClass = 'access-unknown';
                    premiumDescription = 'Последняя проверка не дала точного результата';
                } else {
                    premiumLabel = 'НЕ ПРОВЕРЕНО';
                    premiumClass = 'access-unknown';
                    premiumDescription = 'Автоматическая проверка ещё не выполнялась';
                }
                var premiumBadge = '<span class="playback-access ' + premiumClass + '" title="' + esc(premiumDescription) + '"><span class="access-caption">Доступ к трекам</span><strong>' + premiumLabel + '</strong></span>';
                var premiumCheckStat = a.is_catalog
                    ? '<span class="card-stat access-check-time">FULL/PREVIEW <strong>для каталога не проверяется</strong></span>'
                    : (a.premium_checked_at
                        ? '<span class="card-stat access-check-time">Последняя FULL/PREVIEW проверка <strong>' + new Date(a.premium_checked_at * 1000).toLocaleString(adminLanguage === 'ru' ? 'ru-RU' : 'en-GB') + '</strong></span>'
                        : '<span class="card-stat access-check-time">Автопроверка FULL/PREVIEW <strong>' +
                            ((a.availability || {}).state === 'no_token' ? 'ожидает токен' : (!a.is_active ? 'аккаунт отключён' : 'ожидает выполнения')) + '</strong></span>');
                if (!a.is_catalog && a.preview_streak > 0) premiumCheckStat += '<span class="card-stat" title="Автоматические проверки PREVIEW подряд. При достижении лимита аккаунт переводится только в каталог; результат FULL обнуляет счётчик.">PREVIEW подряд <strong>' +
                    metricNumber(a.preview_streak) + ' / ' + metricNumber(a.preview_streak_limit) + '</strong></span>';
                var catalogBtn = a.is_catalog
                    ? '<button class="btn" onclick="setCatalog(\'' + a.id + '\',false)" title="Вернуть в пул воспроизведения">Из каталога</button>'
                    : '<button class="btn" onclick="setCatalog(\'' + a.id + '\',true)" title="Использовать только для метаданных">В каталог</button>';
                groupCards[group] += '<div class="account-card role-' + group + '" data-search="' + esc((label + ' ' + uid + ' ' + a.client_id).toLowerCase()) + '">' +
                    '<div class="card-header">' +
                        '<div class="left"><span class="acc-num">' + number + '</span><span class="' + statusClass + '"></span><span class="label">' + esc(label) + '</span><span class="status-label ' + readiness.tone + '">' + statusText + '</span>' + (a.heal_next_retry > Date.now() / 1000 ? '<span class="status-label status-warn" data-no-i18n>' + tr('Пауза обновления') + '</span>' : '') + catalogBadge + premiumBadge + '</div>' +
                        '<div class="card-actions">' +
                            '<button class="btn" onclick="refreshAccount(\'' + a.id + '\')">Обновить токен</button>' +
                            '<button class="btn" onclick="checkPremium(\'' + a.id + '\')" title="Проверка FULL/PREVIEW отправляет до четырёх запросов к Tidal">Проверить FULL</button>' +
                            '<button class="btn" onclick="checkSubscription(\'' + a.id + '\')" title="Запрашивает у Tidal тариф и срок подписки">Подписка</button>' +
                            '<button class="btn" onclick="openEdit(\'' + a.id + '\')">Изменить</button>' +
                            '<button class="btn" onclick="duplicateAccount(\'' + a.id + '\')">Дублировать</button>' +
                            catalogBtn +
                            '<button class="btn' + activeCls + '" onclick="toggleAccount(\'' + a.id + '\',' + (!a.is_active) + ')">' + toggleText + '</button>' +
                            '<button class="btn btn-danger" onclick="removeAccount(\'' + a.id + '\')">Удалить</button>' +
                        '</div>' +
                    '</div>' +
                    '<div class="card-body">' +
                        '<div class="cred-row"><span class="cred-key">CLIENT_ID</span><span class="cred-value">' + esc(a.client_id) + '</span></div>' +
                        '<div class="cred-row"><span class="cred-key">USER_ID</span><span class="cred-value">' + esc(uid) + '</span></div>' +
                        countryRow(a) +
                        '<div class="cred-row"><span class="cred-key">Роль</span><span class="cred-value">' + (a.is_catalog ? 'Только каталог' : 'Воспроизведение') + '</span></div>' + subscriptionRow(a) + tokenRefreshDetails(a) +
                    '</div>' +
                    '<div class="account-uptime" data-no-i18n id="uptime-' + esc(a.id) + '">' + uptimeCard(a.uptime, a.id) + '</div>' +
                    '<div class="card-footer">' +
                        '<div class="card-stats">' +
                            '<span class="card-stat" title="Обращения к этому аккаунту с запуска сервера, включая повторы для одного API-запроса">Обращений с запуска <strong>' + metricNumber(a.request_count) + '</strong></span>' +
                            '<span class="card-stat" title="Ошибки запросов и обновления токена этого аккаунта с запуска сервера">Ошибок с запуска <strong>' + metricNumber(a.error_count) + '</strong></span>' +
                            '<span class="card-stat" title="Доля ошибок с учётом примерно последних 15 минут. Чем она выше, тем позже аккаунт получает следующий запрос.">Свежие ошибки <strong>' + metricNumber((a.recent_error_rate || 0) * 100) + '%</strong></span>' +
                            '<span class="card-stat" title="Запросы, начатые этим аккаунтом за последний час; лимит задаётся в настройках">За час <strong>' + metricNumber(a.requests_last_hour) + '</strong></span>' +
                            (a.heal_next_retry > Date.now() / 1000 ? '<span class="card-stat">Повтор обновления через <strong>' + esc(uptimeDuration(a.heal_next_retry - Date.now() / 1000)) + '</strong></span>' : (a.auto_disabled ? '<span class="card-stat">Восстановление <strong>ожидает попытки</strong></span>' : '')) +
                            (a.rate_limited_until > Date.now() / 1000 ? '<span class="card-stat">Пауза 429 ещё <strong>' + esc(uptimeDuration(a.rate_limited_until - Date.now() / 1000)) + '</strong></span>' : '') +
                            (!a.is_active ? '<span class="card-stat">' + (a.auto_disabled ? 'Автоотключён с' : 'Отключён с') + ' <strong>' + disabledSince + '</strong></span>' +
                                '<span class="card-stat">Без работы <strong>' + (disabledAt ? sleepDuration(disabledAt) : '—') + '</strong></span>' : '') +
                            '<span class="card-stat">' + ((a.availability || {}).token === 'valid' ? 'Токен действует ещё' : 'Токен') + ' <strong>' + tokenStr + '</strong></span>' +
                            premiumCheckStat +
                            '<span class="card-stat test-badge" id="test-' + a.id + '" onclick="showTestDetails(\'' + a.id + '\')">Последний тест <strong>не запускался</strong></span>' +
                        '</div>' +
                    '</div>' +
                '</div>';
            }
            html = accountGroup('playback', 'Воспроизведение · FULL', 'Отдают треки целиком; FREE-подписка переводит аккаунт в каталог', groupCards.playback, groupSize.playback) +
                accountGroup('catalog', 'Каталог', 'Только метаданные: поиск, альбомы, обложки; треки не воспроизводят', groupCards.catalog, groupSize.catalog);
        }
        document.getElementById('accounts-container').innerHTML = html;
        document.getElementById('subscriptionSummary').innerHTML = subscriptionSummary(accounts.accounts);
        document.getElementById('readyHistory').innerHTML = readyHistoryCard(accounts.accounts);
        var liveAccounts = Number(stats.live_accounts || 0);
        var totalAccounts = Number(stats.total_accounts || accounts.accounts.length || 0);
        document.getElementById('accountCount').innerHTML = '<span class="status-dot ' + (liveAccounts > 0 ? 'status-ok' : (totalAccounts > 0 ? 'status-warn' : 'status-unknown')) + '"></span><span>Готовы сейчас</span><strong>' + metricNumber(liveAccounts) + ' <span>из</span> ' + metricNumber(totalAccounts) + '</strong>';
        filterAccounts(document.getElementById('accountSearch').value);
        renderTestResults(_testResults);
        return true;
    } catch(e) {
        document.getElementById('error').textContent = 'Failed: ' + e.message;
    }
}

function accountGroup(role, title, hint, cards, count) {
    if (!count) return '';
    return '<div class="account-group" data-group="' + role + '"><div class="account-group-head"><span class="group-mark"></span><h3>' + title + '</h3><span class="group-count">' + metricNumber(count) + '</span><p>' + hint + '</p></div>' +
        '<div class="account-group-list">' + cards + '</div></div>';
}

function esc(s) {
    return String(s == null ? '' : s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;').replace(/'/g,'&#39;');
}

function togglePw(id, btn) {
    var input = document.getElementById(id);
    if (!input) return;
    var show = input.type === 'password';
    input.type = show ? 'text' : 'password';
    if (btn) btn.innerHTML = show ? '&#128064;' : '&#128065;';
}

async function refreshAccount(id) {
    try {
        var res = await fetch('/admin/accounts/' + id + '/refresh', {
            method: 'POST', headers: headers()
        });
        var data = await res.json();
        if (res.ok && data.status !== 'error') {
            document.getElementById('success').textContent = 'Token refreshed, account reactivated!';
            fetchData();
        } else {
            document.getElementById('error').textContent = 'Refresh failed: ' + (data.message || data.detail || res.status);
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function checkPremium(id) {
    try {
        var res = await fetch('/admin/accounts/' + id + '/check-premium', { method: 'POST', headers: headers() });
        var data = await res.json();
        if (!res.ok) throw new Error(data.detail || 'HTTP ' + res.status);
        var label = data.premium === 'premium' ? 'FULL' : (data.premium === 'preview-only' ? 'PREVIEW' : 'Не удалось определить');
        document.getElementById(data.premium === 'premium' ? 'success' : 'error').textContent =
            'Аккаунт ' + id.slice(0, 8) + ': ' + label + (data.reason ? ' — ' + data.reason : '');
        fetchData();
    } catch(e) {
        document.getElementById('error').textContent = 'Проверка FULL/PREVIEW: ' + e.message;
    }
}

async function checkSubscription(id) {
    try {
        var res = await fetch('/admin/accounts/' + id + '/subscription', { method: 'POST', headers: headers() });
        var data = await res.json();
        if (!res.ok) throw new Error(data.detail || 'HTTP ' + res.status);
        delete _subscriptions[id];
        if (data.is_catalog && subscriptionPlan(data.subscription).toUpperCase() === 'FREE') document.getElementById('success').textContent = tr('Подписка FREE: аккаунт переведён в каталог');
    } catch(e) {
        _subscriptions[id] = { error: e.message, checked_at: Date.now() / 1000 };
        document.getElementById('error').textContent = tr('Проверка подписки:') + ' ' + e.message;
    }
    fetchData();
}

async function toggleAccount(id, active) {
    try {
        var res = await fetch('/admin/accounts/' + id + '/toggle', {
            method: 'PUT', headers: headers(), body: JSON.stringify({ active: active })
        });
        if (res.ok) fetchData();
        else { var d = await res.json(); document.getElementById('error').textContent = d.detail || 'Error'; }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function removeAccount(id) {
    if (!confirm(tr('Удалить этот аккаунт? Он сразу перестанет обслуживать запросы.'))) return;
    try {
        var res = await fetch('/admin/accounts/' + id, {
            method: 'DELETE', headers: headers()
        });
        if (res.ok) { await fetchData(); loadProxyStatus(); }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function setCatalog(id, catalog) {
    try {
        var res = await fetch('/admin/accounts/' + id + '/catalog', {
            method: 'PUT', headers: headers(), body: JSON.stringify({ catalog: catalog })
        });
        if (res.ok) fetchData();
        else { var d = await res.json(); document.getElementById('error').textContent = d.detail || 'Error'; }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

function duplicateAccount(id) {
    var a = window._accounts.find(function(x) { return x.id === id; });
    if (!a) { document.getElementById('error').textContent = 'Account not found'; return; }
    document.getElementById('new-label').value = tr('Копия ') + (a.label || a.id.slice(0, 8));
    document.getElementById('new-user-id').value = a.user_id || '';
    document.getElementById('new-client-id').value = a.client_id || '';
    document.getElementById('new-client-secret').value = a.client_secret || '';
    document.getElementById('new-refresh-token').value = a.refresh_token || '';
    document.getElementById('new-catalog').checked = !!a.is_catalog;
    openAddAccount('manual');
}

async function addAccount() {
    var label = document.getElementById('new-label').value;
    var userId = document.getElementById('new-user-id').value || null;
    var client_id = document.getElementById('new-client-id').value;
    var client_secret = document.getElementById('new-client-secret').value;
    var refresh_token = document.getElementById('new-refresh-token').value;

    if (!client_id || !client_secret || !refresh_token) {
        document.getElementById('error').textContent = 'Укажите Client ID, Client Secret и Refresh Token.';
        return;
    }

    try {
        var res = await fetch('/admin/accounts', {
            method: 'POST', headers: headers(),
            body: JSON.stringify({ label: label, user_id: userId, client_id: client_id, client_secret: client_secret, refresh_token: refresh_token, catalog: document.getElementById('new-catalog').checked })
        });
        if (res.ok) {
            document.getElementById('success').textContent = 'Аккаунт добавлен.';
            closeAddAccount();
            await fetchData();
            loadProxyStatus();
            document.getElementById('new-label').value = '';
            document.getElementById('new-user-id').value = '';
            document.getElementById('new-client-id').value = '';
            document.getElementById('new-client-secret').value = '';
            document.getElementById('new-refresh-token').value = '';
            document.getElementById('new-catalog').checked = false;
        } else {
            var d = await res.json();
            document.getElementById('error').textContent = d.detail || 'Error';
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

var oauthSessionId = null;
var oauthPollInterval = null;

var oauthLabelTimer = null;

function startOAuth() {
    document.getElementById('oauth-modal-label').value = document.getElementById('new-label').value || '';
    document.getElementById('oauthUrl').textContent = 'Starting...';
    document.getElementById('oauthStatus').textContent = 'Подключаемся к Tidal…';
    closeAddAccount();
    document.getElementById('oauthOverlay').classList.add('open');
    var lbl = (document.getElementById('new-label').value || '').trim();
    var catalog = document.getElementById('new-catalog').checked;
    fetch('/admin/setup', { method: 'POST', headers: headers(), body: JSON.stringify({ label: lbl || null, catalog: catalog }) })
        .then(function(r) {
            if (!r.ok) throw new Error('HTTP ' + r.status);
            return r.json();
        })
        .then(function(data) {
            document.getElementById('oauthUrl').textContent = data.verification_uri;
            oauthSessionId = data.session_id;
            document.getElementById('oauthStatus').textContent = 'Откройте ссылку и подтвердите доступ. Ожидаем завершения…';
            if (oauthPollInterval) clearInterval(oauthPollInterval);
            oauthPollInterval = setInterval(pollOAuth, 3000);
        })
        .catch(function(e) {
            document.getElementById('oauthStatus').textContent = 'Error: ' + e.message;
        });
}

function updateOAuthLabel() {
    if (!oauthSessionId) return;
    if (oauthLabelTimer) clearTimeout(oauthLabelTimer);
    oauthLabelTimer = setTimeout(function() {
        var lbl = (document.getElementById('oauth-modal-label').value || '').trim();
        fetch('/admin/setup/' + oauthSessionId, {
            method: 'PATCH', headers: headers(), body: JSON.stringify({ label: lbl || null })
        }).catch(function() {});
    }, 500);
}

function pollOAuth() {
    if (!oauthSessionId) return;
    fetch('/admin/setup/' + oauthSessionId, { headers: headers() })
        .then(function(r) { return r.json(); })
        .then(function(data) {
            if (data.status === 'complete') {
                document.getElementById('oauthStatus').textContent = 'Аккаунт «' + data.label + '» добавлен.';
                if (oauthPollInterval) { clearInterval(oauthPollInterval); oauthPollInterval = null; }
                document.getElementById('new-label').value = '';
                document.getElementById('new-catalog').checked = false;
                setTimeout(function() { closeOAuth(); fetchData().then(loadProxyStatus); }, 1500);
            } else if (data.status === 'error') {
                document.getElementById('oauthStatus').textContent = 'Error: ' + data.error;
                if (oauthPollInterval) { clearInterval(oauthPollInterval); oauthPollInterval = null; }
            } else {
                document.getElementById('oauthStatus').textContent = 'Ожидаем подтверждения в браузере…';
            }
        })
        .catch(function(e) {
            document.getElementById('oauthStatus').textContent = 'Poll error: ' + e.message;
        });
}

function closeOAuth() {
    document.getElementById('oauthOverlay').classList.remove('open');
    oauthSessionId = null;
    if (oauthPollInterval) { clearInterval(oauthPollInterval); oauthPollInterval = null; }
}

function copyOAuthUrl() {
    var url = document.getElementById('oauthUrl').textContent;
    if (!url || url === '—' || url === 'Starting...' || url === tr('Starting...')) return;
    navigator.clipboard.writeText(url).then(function() {
        var btn = document.getElementById('copyOAuthBtn');
        btn.textContent = 'Скопировано';
        setTimeout(function() { btn.textContent = 'Копировать'; }, 2000);
    });
}

function openOAuthUrl() {
    var url = document.getElementById('oauthUrl').textContent;
    if (!url || url === '—' || url === 'Starting...' || url === tr('Starting...')) return;
    window.open(url, '_blank');
}

initLanguage();
bootstrap();
setInterval(function() {
    if (!authenticated || document.hidden) return;
    if (currentView === 'overview' || currentView === 'accounts') fetchData();
    if (currentView === 'overview') loadProxyStatus();
    if (currentView === 'access') loadApiKeys();
    if (currentView === 'system') { loadProxyStatus(); loadAlertStatus(); loadCacheStats(); }
}, 15000);
// The overview is a live dashboard: its counters and request log refresh faster than the rest.
// Traffic is counted per second, so two polls a second show each one as soon as it completes.
setInterval(function() {
    if (authenticated && !document.hidden && currentView === 'overview') refreshStats();
}, 500);
setInterval(function() {
    if (authenticated && !document.hidden && currentView === 'overview' && !selectingLog()) loadRequestLog();
}, 2000);

// Re-rendering the log would drop a selection the user is about to copy.
function selectingLog() {
    var selection = window.getSelection();
    var node = selection.isCollapsed ? null : selection.anchorNode;
    return !!node && !!(node.parentElement || node).closest('#rq-recent,#er-list');
}

async function loadAlertStatus() {
    try {
        var res = await fetch('/admin/alerts', { headers: headers() });
        if (!res.ok) return;
        var a = (await res.json()).alerts || {};
        document.getElementById('al-discord').textContent = a.discord_configured ? 'Подключён' : 'Не настроен';
        document.getElementById('disableWebhookBtn').disabled = !a.discord_configured;
        ['alertTestBtn', 'reportStatusBtn', 'reportAccountsBtn'].forEach(function(id) {
            document.getElementById(id).disabled = !a.discord_configured;
        });
    } catch(e) {}
}

async function saveDiscordWebhook() {
    var input = document.getElementById('al-webhook');
    var url = input.value.trim();
    if (!url) {
        document.getElementById('error').textContent = 'Введите URL вебхука.';
        return;
    }
    var btn = document.getElementById('saveWebhookBtn');
    btn.disabled = true;
    try {
        var res = await fetch('/admin/alerts/webhook', { method: 'PUT', headers: headers(), body: JSON.stringify({ url: url }) });
        var data = await res.json();
        if (!res.ok) throw new Error(data.detail || 'Не удалось сохранить вебхук.');
        input.value = '';
        document.getElementById('success').textContent = 'Вебхук сохранён.';
        await loadAlertStatus();
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    } finally {
        btn.disabled = false;
    }
}

async function disableDiscordWebhook() {
    if (!confirm(tr('Удалить сохранённый вебхук и отключить уведомления Discord?'))) return;
    var btn = document.getElementById('disableWebhookBtn');
    btn.disabled = true;
    try {
        var res = await fetch('/admin/alerts/webhook', { method: 'DELETE', headers: headers() });
        var data = await res.json();
        if (!res.ok) throw new Error(data.detail || 'Не удалось отключить вебхук.');
        document.getElementById('al-webhook').value = '';
        document.getElementById('success').textContent = 'Вебхук отключён.';
        await loadAlertStatus();
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    } finally {
        btn.disabled = false;
    }
}

async function sendReport(kind) {
    var btn = document.getElementById(kind === 'status' ? 'reportStatusBtn' : 'reportAccountsBtn');
    btn.textContent = 'Sending...';
    btn.disabled = true;
    try {
        var res = await fetch('/admin/alerts/report', { method: 'POST', headers: headers(), body: JSON.stringify({ kind: kind }) });
        var data = await res.json();
        if (res.ok) {
            document.getElementById('success').textContent = data.message || 'Report sent!';
        } else {
            document.getElementById('error').textContent = data.detail || 'Error';
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    } finally {
        btn.textContent = kind === 'status' ? 'Статус' : 'Аккаунты';
        btn.disabled = false;
    }
}

async function testAlert() {
    var btn = document.getElementById('alertTestBtn');
    btn.textContent = 'Sending...';
    btn.disabled = true;
    try {
        var res = await fetch('/admin/alerts/test', { method: 'POST', headers: headers() });
        var data = await res.json();
        if (res.ok) {
            document.getElementById('success').textContent = data.message || 'Test alert sent!';
        } else {
            document.getElementById('error').textContent = data.detail || 'Error';
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    } finally {
        btn.textContent = 'Тест';
        btn.disabled = false;
    }
}
async function loadApiKeys() {
    try {
        var res = await fetch('/admin/keys', { headers: headers() });
        if (!res.ok) return;
        var keys = (await res.json()).api_keys || [];
        var html = '';
        for (var k of keys) {
            var quota = metricNumber(k.used) + ' / ' + (k.quota > 0 ? metricNumber(k.quota) : '∞');
            var keyStatus = !k.is_active ? 'Отключён' : (k.quota > 0 && k.used >= k.quota ? 'Квота исчерпана' : 'Доступен');
            html += '<div class="cred-row"><span class="cred-key">' + esc(k.label || k.key_prefix) + '</span>' +
                '<span class="cred-value"><span data-no-i18n>' + esc(k.key_prefix) + '…</span> · <span>Использовано:</span> ' + quota + ' · <span>' + keyStatus + '</span></span>' +
                '<span style="margin-left:auto;display:flex;gap:6px">' +
                '<button class="btn" onclick="toggleApiKey(\'' + k.id + '\',' + (!k.is_active) + ')">' + (k.is_active ? 'Отключить' : 'Включить') + '</button>' +
                '<button class="btn btn-danger" onclick="removeApiKey(\'' + k.id + '\')">Удалить</button>' +
                '</span></div>';
        }
        document.getElementById('keys-container').innerHTML = html || '<div class="empty-state"><p>Ключей пока нет</p><p class="hint">Публичные маршруты API открыты.</p></div>';
    } catch(e) {}
}

async function addApiKey() {
    try {
        var res = await fetch('/admin/keys', {
            method: 'POST', headers: headers(),
            body: JSON.stringify({ label: document.getElementById('new-key-label').value, quota: parseInt(document.getElementById('new-key-quota').value) || 0 })
        });
        var data = await res.json();
        if (res.ok) {
            document.getElementById('keyResult').innerHTML = '<div style="background:#10271d;border:1px solid #285b42;border-radius:9px;padding:13px"><strong style="display:block;margin-bottom:8px">Сохраните ключ сейчас — он больше не появится</strong><code id="newApiKeyValue" style="word-break:break-all">' + esc(data.api_key) + '</code><button class="btn" style="margin-left:10px" onclick="copyApiKey()">Копировать</button></div>';
            document.getElementById('new-key-label').value = '';
            document.getElementById('new-key-quota').value = '';
            loadApiKeys();
        } else {
            document.getElementById('error').textContent = data.detail || 'Error';
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

function copyApiKey() {
    var value = document.getElementById('newApiKeyValue');
    if (!value) return;
    navigator.clipboard.writeText(value.textContent).then(function() {
        document.getElementById('success').textContent = 'API-ключ скопирован.';
    }).catch(function() {
        document.getElementById('error').textContent = 'Не удалось скопировать. Выделите ключ и скопируйте вручную.';
    });
}

async function toggleApiKey(id, active) {
    try {
        var res = await fetch('/admin/keys/' + id + '/toggle', {
            method: 'PUT', headers: headers(), body: JSON.stringify({ active: active })
        });
        if (res.ok) loadApiKeys();
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function removeApiKey(id) {
    if (!confirm(tr('Удалить API-ключ? Клиенты с этим ключом потеряют доступ.'))) return;
    try {
        var res = await fetch('/admin/keys/' + id, { method: 'DELETE', headers: headers() });
        if (res.ok) loadApiKeys();
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}

async function downloadBackup() {
    try {
        var res = await fetch('/admin/backup', { headers: headers() });
        if (!res.ok) { document.getElementById('error').textContent = 'Backup failed: ' + res.status; return; }
        var blob = await res.blob();
        var url = URL.createObjectURL(blob);
        var a = document.createElement('a'); a.href = url; a.download = 'hifi-backup.db'; a.click();
        URL.revokeObjectURL(url);
        document.getElementById('success').textContent = 'Backup downloaded';
    } catch(e) { document.getElementById('error').textContent = e.message; }
}

async function restoreBackup(e) {
    var file = e.target.files[0]; if (!file) return;
    if (!confirm(tr('Restore database from ') + file.name + tr('? Current accounts, keys and settings will be replaced.'))) { e.target.value = ''; return; }
    try {
        var buf = await file.arrayBuffer();
        var res = await fetch('/admin/backup/restore', { method: 'POST', headers: headers(), body: buf });
        var data = await res.json();
        if (res.ok) {
            document.getElementById('success').textContent = data.message || 'Restored!';
            document.getElementById('restoreResult').textContent = '';
            fetchData();
            loadApiKeys();
        } else {
            document.getElementById('error').textContent = data.detail || 'Restore failed';
        }
    } catch(err) { document.getElementById('error').textContent = 'Restore error: ' + err.message; }
    e.target.value = '';
}

async function clearCache() {
    var btn = document.getElementById('clearCacheBtn');
    btn.textContent = 'Clearing...';
    btn.disabled = true;
    try {
        var res = await fetch('/admin/cache/clear', { method: 'POST', headers: headers() });
        var data = await res.json();
        if (res.ok) {
            document.getElementById('success').textContent = data.message || 'Cache cleared!';
            loadRequestLog();
        } else {
            document.getElementById('error').textContent = data.detail || 'Error';
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    } finally {
        btn.textContent = 'Clear Cache';
        btn.disabled = false;
    }
}

async function resetUptime() {
    if (!confirm(tr('Сбросить историю аптайма всех аккаунтов? Отсчёт начнётся заново, вернуть историю будет нельзя.'))) return;
    var btn = document.getElementById('resetUptimeBtn');
    btn.disabled = true;
    try {
        var res = await fetch('/admin/uptime/reset', { method: 'POST', headers: headers() });
        var data = await res.json();
        if (!res.ok) throw new Error(data.detail || 'HTTP ' + res.status);
        document.getElementById('success').textContent = 'История аптайма сброшена.';
        fetchData();
    } catch(e) {
        document.getElementById('error').textContent = 'Сброс аптайма: ' + e.message;
    } finally {
        btn.disabled = false;
    }
}

async function clearPlaybackQueue() {
    var btn = document.getElementById('clearPlaybackBtn');
    btn.disabled = true;
    try {
        var res = await fetch('/admin/playback/clear', { method: 'POST', headers: headers() });
        var data = await res.json();
        if (!res.ok) throw new Error(data.detail || 'HTTP ' + res.status);
        document.getElementById('success').textContent =
            'Очередь очищена: ожидавших ' + data.cancelled_queued + ', выполнявшихся ' + data.marked_processing;
        fetchData();
    } catch(e) {
        document.getElementById('error').textContent = 'Очистка очереди: ' + e.message;
    } finally {
        btn.disabled = false;
    }
}

async function loadCacheStats() {
    try {
        var res = await fetch('/admin/cache', { headers: headers() });
        if (!res.ok) return;
        var c = (await res.json()).cache || {};
        document.getElementById('cc-hits').textContent = c.hits != null ? c.hits : '—';
        document.getElementById('cc-misses').textContent = c.misses != null ? c.misses : '—';
        var lookups = (c.hits || 0) + (c.misses || 0);
        document.getElementById('cc-rate').textContent = lookups > 0
            ? (c.hits * 100 / lookups).toLocaleString(adminLanguage === 'ru' ? 'ru-RU' : 'en-GB', {minimumFractionDigits:1, maximumFractionDigits:1}) + '%' : '—';
        document.getElementById('cc-stale').textContent = c.stale != null ? c.stale : '—';
        document.getElementById('cc-negative').textContent = c.negative != null ? c.negative : '—';
        document.getElementById('cc-coalesced').textContent = c.coalesced != null ? c.coalesced : '—';
        document.getElementById('cc-tracks').textContent = c.tracks != null ? c.tracks : '—';
    } catch(e) {}
}

var requestLogLimit = 100;
var renderedRequestLogLimit = 0;

function toggleRequestLogDetails() {
    var terminal = document.getElementById('requestLogTerminal');
    var button = document.getElementById('requestLogDetails');
    var visible = terminal.classList.toggle('show-extra');
    button.classList.toggle('btn-active', visible);
    button.setAttribute('aria-pressed', visible ? 'true' : 'false');
}

function showMoreRequests() {
    requestLogLimit = Math.min(requestLogLimit * 2, 5000);
    loadRequestLog();
}

async function loadRequestLog() {
    try {
        var limit = requestLogLimit;
        var res = await fetch('/admin/requests?limit=' + limit, { headers: headers() });
        if (!res.ok) return;
        var r = (await res.json()).requests || {};
        if (limit !== requestLogLimit) return;
        document.getElementById('rq-total').textContent = r.total != null ? r.total : '—';
        var recent = r.recent || [];
        document.getElementById('rq-shown').textContent = recent.length;
        document.getElementById('requestLogMore').hidden = recent.length >= (r.total || 0) || limit >= 5000;
        document.getElementById('rq-errors').textContent = r.errors != null ? r.errors : '—';
        document.getElementById('rq-user-errors').textContent = r.user_errors != null ? r.user_errors : '—';
        document.getElementById('rq-upstream-errors').textContent = r.upstream_errors != null ? r.upstream_errors : '—';
        document.getElementById('rq-p50').textContent = r.p50_ms != null ? r.p50_ms + 'ms' : '—';
        document.getElementById('rq-p95').textContent = r.p95_ms != null ? r.p95_ms + 'ms' : '—';
        var ep = '';
        for (var e of (r.by_endpoint || []).slice(0, 8)) {
            ep += '<span>' + esc(e.endpoint) + ' <strong>×' + e.hits + '</strong></span>';
        }
        document.getElementById('rq-endpoints').innerHTML = ep;
        var tt = '';
        for (var t of (r.top_tracks || []).slice(0, 5)) {
            tt += '<span>#' + esc(t.id) + ' <strong>×' + t.hits + '</strong></span>';
        }
        document.getElementById('rq-tracks').innerHTML = tt ? '<span style="color:#5f6f60">top:</span> ' + tt : '';
        var slow = '';
        for (var item of (r.slowest || [])) {
            slow += '<span>' + esc(item.endpoint) + (item.detail ? ' #' + esc(item.detail) : '') +
                ' <strong>' + item.latency_ms + 'ms</strong></span>';
        }
        document.getElementById('rq-slowest').innerHTML = slow ? '<span>Самые медленные:</span> ' + slow : '';
        var rows = '';
        for (var q of recent.slice().reverse()) {
            var cls = q.status >= 500 ? 'test-fail' : (q.status >= 400 ? 'test-pending' : 'test-pass');
            var chain = accountChain(q.accounts);
            var accountInfo = chain ? ' <span class="term-extra term-account">ACCOUNT ' + chain + '</span>' : '';
            rows += '<div class="term-line"><span class="term-time">' + logTime(q.ts) + '</span> ' +
                '<span class="term-method m-' + q.method + '">' + q.method + '</span> ' +
                '<span class="term-path">' + esc(q.path) + '</span>' +
                (q.detail ? ' <span class="term-id">#' + esc(q.detail) + '</span>' : '') + ' ' +
                '<span class="' + cls + '">' + q.status + '</span> ' +
                '<span class="term-dim">' + q.latency_ms + 'ms ' + esc(q.client_ip) + '</span>' +
                (q.cache ? ' <span class="term-id">CACHE ' + esc(q.cache) + '</span>' : '') +
                (q.slow ? ' <span class="test-pending">SLOW</span>' : '') + accountInfo + '</div>';
        }
        var box = document.getElementById('rq-recent');
        var previousHeight = box.scrollHeight;
        var previousTop = box.scrollTop;
        var atBottom = previousHeight - previousTop - box.clientHeight < 32;
        if (rows) {
            box.innerHTML = rows + '<div class="term-line"><span class="term-dim">$</span> <span class="term-cursor"></span></div>';
            if (atBottom) box.scrollTop = box.scrollHeight;
            else if (limit > renderedRequestLogLimit) box.scrollTop = previousTop + box.scrollHeight - previousHeight;
            else box.scrollTop = previousTop;
        } else {
            box.innerHTML = '<div class="term-line"><span class="term-dim">$ waiting for traffic…</span> <span class="term-cursor"></span></div>';
        }
        renderedRequestLogLimit = limit;
        renderErrors(r.recent_errors || []);
    } catch(e) {}
}

function logTime(ts) {
    if (!ts) return '';
    var d = new Date(ts * 1000);
    return ('0' + d.getHours()).slice(-2) + ':' + ('0' + d.getMinutes()).slice(-2) + ':' + ('0' + d.getSeconds()).slice(-2);
}

// Accounts tried for one request, in attempt order.
function accountChain(accounts) {
    return (accounts || []).map(function(account) {
        var label = account.label || account.id || '—';
        var shortId = account.id && account.id !== 'catalog-token' ? ' [' + account.id.slice(0, 8) + ']' : '';
        var role = account.role === 'catalog' ? ' · catalog' : '';
        return esc(label) + esc(shortId + role);
    }).join(' → ');
}

// What an error response means: [cause, explanation]. The server's own
// message is matched first; the status alone is the fallback.
function errorCause(q) {
    var detail = String(q.error || '').toLowerCase();
    var has = function(text) { return detail.indexOf(text) !== -1; };
    if (has('different track')) return ['Подмена трека', 'Tidal отдал аудио под другим id, и это другая запись: ISRC или длительность не совпали либо сравнить их не удалось — причина в скобках в тексте ответа. Замену с тем же ISRC и длительностью сервис отдаёт без ошибки.'];
    if (has('preview only')) return ['Только превью', 'Tidal отдал 30-секундный фрагмент: трек недоступен целиком в регионе аккаунтов или у них нет нужной подписки.'];
    if (has('did not return high')) return ['Нет качества HIGH', 'Tidal не отдал трек в AAC 320 kbps, выбранном как формат по умолчанию.'];
    if (has('queue full')) return ['Очередь переполнена', 'Запросов воспроизведения больше, чем успевает обслужить пул аккаунтов. Клиенту стоит повторить через несколько секунд.'];
    if (has('playback accounts are busy')) return ['Все аккаунты заняты', 'Каждый аккаунт воспроизведения уже отправлял в Tidal максимум одновременных запросов или отдыхал между запросами, и ни один не освободился за отведённое время. Клиент получил 429 с Retry-After и может повторить; если это повторяется, увеличьте лимит или паузу уменьшите, либо добавьте аккаунты.'];
    if (has('concurrent request limit')) return ['Каталог занят', 'Все аккаунты каталога заняты, а пула воспроизведения для переноса нагрузки нет.'];
    if (has('all accounts') || has('no tidal credentials')) return ['Нет рабочих аккаунтов', 'Ни один аккаунт не смог выполнить запрос: аккаунтов нет, токены истекли, аккаунты отключены или все попытки завершились ошибкой.'];
    if (has('proxy')) return ['Прокси недоступен', 'Нет рабочего прокси для запроса к Tidal, а прямое соединение отключено.'];
    if (has('connection error')) return ['Нет связи с Tidal', 'Не удалось установить соединение с Tidal — проблема сети или прокси.'];
    if (has('timeout')) return ['Таймаут', 'Запрос не уложился в отведённое время: Tidal или очередь отвечали слишком долго. Клиент может повторить.'];
    if (has('cancelled')) return ['Запрос отменён', 'Клиент отменил запрос воспроизведения до его завершения.'];
    if (q.status === 429) return ['Лимит Tidal', 'Tidal ограничил частоту запросов; аккаунт поставлен на паузу до конца ограничения.'];
    if (q.status === 404) return ['Не найдено', 'Запрошенный ресурс не найден: неверный id или контент удалён из Tidal.'];
    if (q.status === 400) return ['Неверный запрос', 'Клиент прислал неполные или некорректные параметры.'];
    if (q.status === 401) return ['Токен отклонён', 'Tidal не принял токен аккаунта, и обновить его не удалось.'];
    if (q.status === 403) return ['Доступ запрещён', 'Tidal отказал аккаунту в доступе — возможна блокировка аккаунта или региона.'];
    if (has('upstream api error') || has('parse tidal response')) return ['Ошибка Tidal', 'Tidal ответил этим статусом или прислал ответ, который не удалось разобрать.'];
    if (q.status === 503) return ['Сервис недоступен', 'Запрос не удалось обслужить прямо сейчас; подробности — в тексте ответа.'];
    if (q.status >= 500) return ['Внутренняя ошибка', 'Сбой внутри сервиса; подробности — в тексте ответа и логах сервера.'];
    return ['Другая ошибка', 'Причина не распознана; смотрите текст ответа.'];
}

function errorTally(counts) {
    return Object.keys(counts).sort(function(a, b) { return counts[b] - counts[a]; }).slice(0, 8).map(function(name) {
        return '<span>' + name + ' <strong>×' + counts[name] + '</strong></span>';
    }).join('');
}

// Latest API errors, newest first, each with its cause and the accounts tried.
function errorsPanel(errors) {
    var causes = {}, accounts = {}, rows = '';
    for (var q of errors) {
        var cause = errorCause(q);
        var title = '<span>' + cause[0] + '</span>';
        causes[title] = (causes[title] || 0) + 1;
        for (var account of (q.accounts || [])) {
            var name = '<span data-no-i18n>' + esc(account.label || account.id || '—') + '</span>';
            accounts[name] = (accounts[name] || 0) + 1;
        }
        var chain = accountChain(q.accounts);
        rows += '<div class="err-item"><div class="term-line"><span class="term-time">' + logTime(q.ts) + '</span> ' +
            '<span class="term-method m-' + esc(q.method) + '">' + esc(q.method) + '</span> ' +
            '<span class="term-path" data-no-i18n>' + esc(q.path) + '</span>' +
            (q.detail ? ' <span class="term-id" data-no-i18n>#' + esc(q.detail) + '</span>' : '') + ' ' +
            '<span class="' + (q.status >= 500 ? 'test-fail' : 'test-pending') + '">' + esc(q.status) + '</span> ' +
            '<span class="term-dim">' + esc(q.latency_ms) + 'ms ' + esc(q.client_ip) + '</span>' +
            (q.cache ? ' <span class="term-id">CACHE ' + esc(q.cache) + '</span>' : '') + '</div>' +
            '<div class="err-cause"><strong>' + cause[0] + '</strong> <span>' + cause[1] + '</span></div>' +
            (q.error ? '<div class="term-dim" data-no-i18n>' + esc(q.error) + '</div>' : '') +
            (chain ? '<div class="term-account" data-no-i18n>ACCOUNT ' + chain + '</div>' : '') + '</div>';
    }
    var causeList = errorTally(causes), accountList = errorTally(accounts);
    return {
        count: errors.length,
        causes: causeList ? '<span>Причины:</span> ' + causeList : '',
        accounts: accountList ? '<span>Аккаунты:</span> ' + accountList : '',
        list: rows || '<div class="term-line"><span class="term-dim">Ошибок пока нет.</span></div>',
    };
}

function renderErrors(errors) {
    var panel = errorsPanel(errors);
    document.getElementById('er-count').textContent = panel.count;
    document.getElementById('er-causes').innerHTML = panel.causes;
    document.getElementById('er-accounts').innerHTML = panel.accounts;
    var box = document.getElementById('er-list');
    var top = box.scrollTop;
    box.innerHTML = panel.list;
    box.scrollTop = top;
}

async function loadProxyStatus() {
    try {
        var res = await fetch('/admin/proxies', { headers: headers() });
        if (!res.ok) return;
        var data = await res.json();
        var p = data.proxies || {};
        var assignments = p.assignments || [];
        proxyEnabled = !!p.enabled;
        var trying = p.last_try && Date.now() / 1000 - p.last_try < 20;
        var verified = assignments.filter(function(a) { return a.verified; }).length;
        var status = !p.enabled ? 'Напрямую' : (verified > 0
            ? (verified === assignments.length ? 'Назначения проверены' : 'Частично проверены')
            : (p.ready ? 'Есть доступный прокси' : (!p.last_try ? 'Ожидают проверки' : (trying ? 'Проверяем прокси' : 'Нет рабочего прокси'))));
        document.getElementById('px-status').textContent = status;
        document.getElementById('px-current').textContent = p.enabled ? assignments.length : '—';
        document.getElementById('px-pool').textContent = p.pool_size != null ? p.pool_size : '—';
        document.getElementById('px-verified').textContent = p.enabled ? verified + ' / ' + assignments.length : '—';
        document.getElementById('px-fails').textContent = assignments.reduce(function(sum, a) { return sum + (a.consecutive_fails || 0); }, 0);
        overviewProxy = { enabled: !!p.enabled, status: status, verified: verified, assigned: assignments.length, pool_size: p.pool_size,
            tone: !p.enabled ? 'status-unknown' : (verified > 0 ? (verified === assignments.length ? 'status-ok' : 'status-warn')
                : (p.ready ? 'status-ok' : (!p.last_try || trying ? 'status-warn' : 'status-err'))) };
        if (window._stats) document.getElementById('stats').innerHTML = overviewCards(window._stats);
        document.getElementById('px-assignments').textContent = p.enabled ? assignments.map(function(a) {
            var account = (window._accounts || []).find(function(item) { return item.id === a.account_id; });
            return (account ? account.label : a.account_id.slice(0, 8)) + ' → ' + a.proxy + (a.verified ? '' : ' · проверяется');
        }).join('\n') : '';
        document.getElementById('pxToggleBtn').textContent = p.enabled ? 'Выключить прокси' : 'Включить прокси';
        document.getElementById('pxToggleBtn').classList.toggle('btn-danger', !!p.enabled);
        document.getElementById('pxToggleBtn').classList.toggle('btn-primary', !p.enabled);
        document.getElementById('px-persist').textContent = data.persistent ? 'Список и режим сохраняются после перезапуска.' : 'Без базы данных настройки действуют до перезапуска.';
        if (!proxyListDirty) document.getElementById('px-list').value = (p.entries || []).join('\n');
    } catch(e) {}
}

async function updateProxies(enabled, toggled) {
    var entries = document.getElementById('px-list').value.split(/\r?\n/).map(function(url) { return url.trim(); }).filter(Boolean);
    var toggle = document.getElementById('pxToggleBtn');
    var save = document.getElementById('pxSaveBtn');
    toggle.disabled = true; save.disabled = true;
    try {
        var res = await fetch('/admin/proxies', { method:'PUT', headers:headers(), body:JSON.stringify({ enabled:enabled, proxies:entries }) });
        var data = await res.json();
        if (!res.ok) throw new Error(data.detail || 'Не удалось сохранить прокси');
        proxyListDirty = false;
        document.getElementById('success').textContent = toggled ? (enabled ? 'Прокси включены. Проверяем соединение…' : 'Прокси выключены. Используется прямое соединение.') : 'Список прокси сохранён.';
        loadProxyStatus();
    } catch(e) {
        document.getElementById('error').textContent = e.message;
        loadProxyStatus();
    } finally {
        toggle.disabled = false; save.disabled = false;
    }
}

function toggleProxies() { updateProxies(!proxyEnabled, true); }
function saveProxyList() { updateProxies(proxyEnabled, false); }

// 0 is a real value here (no limit, no pause), so it must not fall back to the default.
function fillPlaybackLimits(r) {
    document.getElementById('rl-track-concurrency').value = r.track_concurrency_per_account != null ? r.track_concurrency_per_account : 1;
    document.getElementById('rl-track-delay').value = r.track_request_delay_ms != null ? r.track_request_delay_ms : 0;
    document.getElementById('rl-auth-disable').value = r.auth_failure_disable_hours != null ? r.auth_failure_disable_hours : 24;
    document.getElementById('rl-per-hour').value = r.requests_per_hour_per_account != null ? r.requests_per_hour_per_account : 0;
}

async function loadSettings() {
    try {
        var res = await fetch('/admin/settings', { headers: headers() });
        if (!res.ok) return;
        var d = await res.json();
        var r = d.settings || d.rate_limits || {};
        document.getElementById('rl-atmos').value = r.atmos_mode || 'off';
        document.getElementById('rl-v1-endpoint').value = r.v1_playback_endpoint || 'playbackinfopostpaywall';
        fillPlaybackLimits(r);
        document.getElementById('rl-track-requests').value = r.track_requests_per_account || 1;
        document.getElementById('rl-catalog-requests').value = r.catalog_requests_per_account || 1;
        document.getElementById('rl-catalog-concurrency').value = r.catalog_concurrency_per_account || 0;
    } catch(e) {}
}

async function exportCredentials() {
    try {
        var res = await fetch('/admin/accounts/export', { headers: headers() });
        if (!res.ok) { document.getElementById('error').textContent = 'Export failed: ' + res.status; return; }
        var data = await res.json();
        var list = data.accounts || data;
        var blob = new Blob([JSON.stringify(list, null, 2)], { type: 'application/json' });
        var url = URL.createObjectURL(blob);
        var a = document.createElement('a'); a.href = url; a.download = 'credentials.json'; a.click();
        URL.revokeObjectURL(url);
        document.getElementById('success').textContent = 'Exported ' + list.length + ' accounts';
    } catch(e) { document.getElementById('error').textContent = e.message; }
}

async function importCredentials(e) {
    var file = e.target.files[0]; if (!file) return;
    try {
        var text = await file.text();
        var json = JSON.parse(text);
        var payload = json.accounts ? json : (Array.isArray(json) ? { accounts: json } : json);
        var res = await fetch('/admin/accounts/import', { method: 'POST', headers: headers(), body: JSON.stringify(payload) });
        var data = await res.json();
        if (res.ok) {
            document.getElementById('success').textContent = 'Imported ' + data.imported + ' accounts (skipped ' + data.skipped + ')';
            document.getElementById('importResult').textContent = data.errors && data.errors.length ? 'Errors: ' + JSON.stringify(data.errors).slice(0, 400) : '';
            await fetchData();
            loadProxyStatus();
        } else { document.getElementById('error').textContent = data.detail || 'Import failed'; }
    } catch(err) { document.getElementById('error').textContent = 'Import error: ' + err.message; }
    e.target.value = '';
}

async function saveSettings() {
    var body = {
        settings: {
            atmos_mode: document.getElementById('rl-atmos').value,
            v1_playback_endpoint: document.getElementById('rl-v1-endpoint').value,
            track_requests_per_account: Number(document.getElementById('rl-track-requests').value || 1),
            catalog_requests_per_account: Number(document.getElementById('rl-catalog-requests').value || 1),
            catalog_concurrency_per_account: Number(document.getElementById('rl-catalog-concurrency').value || 0),
            track_concurrency_per_account: Number(document.getElementById('rl-track-concurrency').value || 0),
            track_request_delay_ms: Number(document.getElementById('rl-track-delay').value || 0),
            auth_failure_disable_hours: Number(document.getElementById('rl-auth-disable').value || 0),
            requests_per_hour_per_account: Number(document.getElementById('rl-per-hour').value || 0)
        }
    };
    try {
        var res = await fetch('/admin/settings', {
            method: 'PUT', headers: headers(), body: JSON.stringify(body)
        });
        if (res.ok) {
            document.getElementById('success').textContent = 'Settings saved!';
            var d = await res.json();
            var r = d.settings || d.rate_limits || {};
            document.getElementById('rl-atmos').value = r.atmos_mode || 'off';
            document.getElementById('rl-v1-endpoint').value = r.v1_playback_endpoint || 'playbackinfopostpaywall';
            fillPlaybackLimits(r);
            document.getElementById('rl-track-requests').value = r.track_requests_per_account || 1;
            document.getElementById('rl-catalog-requests').value = r.catalog_requests_per_account || 1;
            document.getElementById('rl-catalog-concurrency').value = r.catalog_concurrency_per_account || 0;
        } else {
            var d = await res.json();
            document.getElementById('error').textContent = d.detail || 'Error saving settings';
        }
    } catch(e) {
        document.getElementById('error').textContent = e.message;
    }
}
</script>
</body>
</html>"#;
