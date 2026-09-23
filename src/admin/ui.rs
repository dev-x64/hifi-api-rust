use axum::response::Html;

pub async fn admin_index() -> Html<&'static str> {
    Html(ADMIN_HTML)
}

const ADMIN_HTML: &str = r#"<!DOCTYPE html>
<html lang="ru">
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

.stats { display:grid; grid-template-columns:repeat(auto-fit,minmax(150px,1fr)); gap:12px; margin-bottom:24px; }
.stat-card { background:#161b22; border:1px solid #30363d; border-radius:10px; padding:18px 20px; transition:border-color 0.2s; }
.stat-card:hover { border-color:#484f58; }
.stat-card .label { font-size:11px; color:#8b949e; text-transform:uppercase; letter-spacing:0.5px; }
.stat-card .value { font-size:26px; font-weight:700; margin-top:4px; color:#f0f6fc; letter-spacing:-0.5px; }

.accounts-grid { display:flex; flex-direction:column; gap:20px; margin-bottom:24px; }
.account-card { background:#161b22; border:1px solid #30363d; border-radius:10px; overflow:hidden; transition:border-color 0.2s, box-shadow 0.2s; }
.account-card:hover { border-color:#484f58; box-shadow:0 4px 24px rgba(0,0,0,0.3); }

.card-header { display:flex; align-items:center; justify-content:space-between; padding:14px 20px; background:#1c2128; border-bottom:1px solid #30363d; flex-wrap:wrap; gap:10px; }
.card-header .left { display:flex; align-items:center; gap:10px; min-width:0; }
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
  .stats { grid-template-columns:repeat(2,1fr); gap:10px; }
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
  .stats { grid-template-columns:1fr; }
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
.term-body { font-family:'SF Mono','Fira Code',Menlo,Consolas,monospace; font-size:12px; line-height:1.75; padding:12px 14px; height:280px; overflow-y:auto; color:#c9e8d2; scrollbar-width:thin; scrollbar-color:#1d3a24 transparent; }
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
.term-cursor { display:inline-block; width:8px; height:14px; background:#3fb950; vertical-align:-2px; animation:termBlink 1.1s infinite; }
@media (max-width:768px) { .term-body { height:220px; font-size:11px; } }

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
.stats { grid-template-columns:repeat(3,minmax(0,1fr)); gap:12px; }
.stat-card { border-color:var(--line); background:linear-gradient(145deg,#151a22,#11161d); border-radius:12px; padding:18px; min-height:100px; }
.stat-card:hover { border-color:#394452; }
.stat-card .label { color:var(--muted); font-family:Inter,sans-serif; font-size:10px; font-weight:700; }
.stat-card .value { font-family:Inter,sans-serif; color:var(--text); font-size:25px; margin-top:10px; }
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
@media (max-width:980px) {
  .stats { grid-template-columns:repeat(2,minmax(0,1fr)); }
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
  .stats { grid-template-columns:1fr 1fr; }
  .toolbar,.section-head { align-items:stretch; flex-direction:column; }
  .search-wrap { width:100%; }
}
@media (max-width:480px) {
  .stats { grid-template-columns:1fr; }
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

<div class="mobile-bar"><button class="btn icon-btn btn-quiet" onclick="toggleSidebar(true)" aria-label="Открыть меню">☰</button><strong>HiFi API</strong><span id="mobileVersion" style="font-size:11px;color:var(--muted)">v2.10</span></div>
<main class="workspace"><div class="container">
  <header class="header">
    <div><div class="page-kicker" id="pageKicker">Состояние сервиса</div><h1 id="pageTitle">Обзор</h1><p class="page-subtitle" id="pageSubtitle">Главные показатели и последние запросы в одном месте.</p></div>
    <div class="header-actions"><button class="btn" onclick="refreshCurrentView()">Обновить</button><button class="btn btn-primary mobile-keep" onclick="showView('accounts');openAddAccount()">+ Аккаунт</button><span class="badge" id="version">v2.10</span></div>
  </header>
  <div id="error" class="error"></div><div id="success" class="success"></div>

  <section id="view-overview" class="view active">
    <div id="stats" class="stats"></div>
    <div class="section-head"><div><h2>Журнал запросов</h2><p>Последняя активность обновляется автоматически каждые 15 секунд.</p></div><button class="btn" onclick="loadRequestLog()">Обновить журнал</button></div>
    <div class="terminal">
      <div class="term-bar"><span class="term-dots"><i></i><i></i><i></i></span><span class="term-title">hifi-api — live request log</span><span class="term-live" id="term-live">● LIVE</span></div>
      <div class="term-meta"><span>Всего <strong id="rq-total">—</strong></span><span>Ошибок <strong id="rq-errors">—</strong></span><span>p50 <strong id="rq-p50">—</strong></span><span>p95 <strong id="rq-p95">—</strong></span><span id="rq-endpoints"></span><span id="rq-tracks" style="color:#d2a8ff"></span></div>
      <div id="rq-recent" class="term-body"></div>
    </div>
  </section>

  <section id="view-accounts" class="view">
    <div class="section-head"><div><h2>Tidal-аккаунты</h2><p>Управляйте пулом воспроизведения, токенами и каталогом.</p></div><div class="section-actions"><button class="btn" onclick="testAll()" id="testAllBtn">Проверить все</button><button class="btn btn-primary" onclick="openAddAccount()">+ Добавить</button></div></div>
    <div class="toolbar"><div class="search-wrap"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/></svg><input id="accountSearch" type="search" placeholder="Найти аккаунт..." oninput="filterAccounts(this.value)"></div><div class="helper" id="accountCount">Загрузка…</div></div>
    <div id="accounts-container" class="accounts-grid"></div>
    <div class="test-results-section" id="testResultsSection" style="display:none"><div class="test-results-header"><h3>Результаты проверки</h3><div class="test-summary" id="testSummary"></div></div><div class="test-results-body" id="testResultsList"></div></div>
  </section>

  <section id="view-access" class="view">
    <div class="section-head"><div><h2>Ключи доступа</h2><p>Контролируйте клиентов API и их квоты.</p></div></div>
    <div class="form-section">
      <h3>Новый API-ключ</h3><p class="form-copy">После создания ключ показывается только один раз. Квота 0 означает неограниченный доступ.</p>
      <div class="form-row"><div class="form-group"><label>Название</label><input type="text" id="new-key-label" placeholder="Например, мобильное приложение"></div><div class="form-group"><label>Квота запросов</label><input type="number" id="new-key-quota" min="0" placeholder="0"></div></div>
      <button class="btn btn-primary" onclick="addApiKey()">Создать ключ</button><div id="keyResult" style="font-size:12px;margin-top:12px;color:var(--green);word-break:break-all"></div>
    </div>
    <div class="form-section"><h3>Активные ключи</h3><p class="form-copy">Если ключей нет, публичные маршруты API остаются открытыми.</p><div id="keys-container"></div></div>
  </section>

  <section id="view-system" class="view">
    <div class="section-head"><div><h2>Системные настройки</h2><p>Интеграции, резервные копии и обслуживание.</p></div></div>
    <div class="system-grid">
      <div class="form-section"><h3>Воспроизведение</h3><p class="form-copy">Настройки выбора формата и автоматического восстановления.</p><div class="form-group" style="margin-bottom:16px"><label>Формат по умолчанию</label><select id="rl-atmos" class="select" style="width:100%;padding:10px 12px"><option value="high">HIGH · AAC 320 kbps (v1)</option><option value="off">FLAC в приоритете</option><option value="prefer">Atmos в приоритете</option></select></div><label style="display:flex;align-items:center;gap:9px;font-size:12px;color:#c9d1d9;margin-bottom:18px"><input type="checkbox" id="rl-autoheal"> Автовосстановление отключённых системой аккаунтов</label><button class="btn btn-primary" onclick="saveSettings()">Сохранить</button></div>
      <div class="form-section"><h3>Прокси</h3><p class="form-copy">Маршрутизация исходящих запросов. Изменения применяются сразу.</p><div class="card-stats" style="margin-bottom:16px"><span class="card-stat">Статус <strong id="px-status">—</strong></span><span class="card-stat">Текущий <strong id="px-current">—</strong></span><span class="card-stat">В пуле <strong id="px-pool">—</strong></span><span class="card-stat">Сбоев <strong id="px-fails">—</strong></span></div><div class="form-group"><label for="px-list">Адреса прокси · по одному в строке</label><textarea id="px-list" spellcheck="false" placeholder="http://user:password@host:port" oninput="proxyListDirty=true"></textarea></div><div class="section-actions" style="margin-top:12px"><button class="btn btn-primary" id="pxToggleBtn" onclick="toggleProxies()">Включить прокси</button><button class="btn" id="pxSaveBtn" onclick="saveProxyList()">Сохранить список</button></div><p class="helper" id="px-persist" style="margin-top:11px"></p></div>
      <div class="form-section"><h3>Уведомления</h3><p class="form-copy">Discord-оповещения о риске блокировки и недоступности аккаунтов.</p><div class="card-stats" style="margin-bottom:15px"><span class="card-stat">Discord <strong id="al-discord">—</strong></span></div><div class="section-actions"><button class="btn" onclick="testAlert()" id="alertTestBtn">Тест</button><button class="btn" onclick="sendReport('status')" id="reportStatusBtn">Статус</button><button class="btn" onclick="sendReport('accounts')" id="reportAccountsBtn">Аккаунты</button></div></div>
      <div class="form-section"><h3>Кэш</h3><p class="form-copy">Очистка безопасна, но первые ответы после неё могут быть медленнее.</p><div class="card-stats" style="margin-bottom:15px"><span class="card-stat">Попадания <strong id="cc-hits">—</strong></span><span class="card-stat">Промахи <strong id="cc-misses">—</strong></span></div><button class="btn" onclick="clearCache()" id="clearCacheBtn">Очистить кэш</button></div>
      <div class="form-section"><h3>Учётные данные</h3><p class="form-copy">Экспортируйте или импортируйте Tidal-аккаунты в JSON. Дубликаты токенов будут пропущены.</p><div class="section-actions"><button class="btn" onclick="exportCredentials()">Экспорт JSON</button><button class="btn" onclick="document.getElementById('importFile').click()">Импорт JSON</button><input type="file" id="importFile" accept=".json,application/json" style="display:none" onchange="importCredentials(event)"></div><div id="importResult" class="helper" style="margin-top:10px"></div></div>
      <div class="form-section"><h3>База данных</h3><p class="form-copy">Скачайте полный снимок или восстановите состояние без перезапуска.</p><div class="section-actions"><button class="btn" onclick="downloadBackup()">Скачать копию</button><button class="btn btn-danger" onclick="document.getElementById('restoreFile').click()">Восстановить</button><input type="file" id="restoreFile" accept=".db,.sqlite,.sqlite3,application/x-sqlite3" style="display:none" onchange="restoreBackup(event)"></div><div id="restoreResult" class="helper" style="margin-top:10px"></div></div>
    </div>
  </section>
</div></main>
</div>

<div id="addAccountOverlay" class="overlay" onclick="if(event.target===this)closeAddAccount()"><div class="modal" style="width:620px"><div class="section-head"><div><div class="page-kicker">Новый аккаунт</div><h3 style="margin:0">Подключить Tidal</h3></div><button class="btn icon-btn btn-quiet" onclick="closeAddAccount()" aria-label="Закрыть">×</button></div><p class="form-copy">Самый простой вариант — OAuth. Ручной ввод подходит для уже готовых credentials.</p><div class="form-row"><div class="form-group"><label>Название</label><input type="text" id="new-label" placeholder="Основной аккаунт"></div><div class="form-group"><label>User ID · необязательно</label><input type="text" id="new-user-id" placeholder="208921067"></div></div><div class="form-group" style="margin-bottom:14px"><label>Client ID</label><input type="text" id="new-client-id" placeholder="client_id"></div><div class="form-row"><div class="form-group"><label>Client Secret</label><div class="pw-wrap"><input type="password" id="new-client-secret" placeholder="client_secret"><button type="button" class="pw-toggle" onclick="togglePw('new-client-secret', this)">&#128065;</button></div></div><div class="form-group"><label>Refresh Token</label><div class="pw-wrap"><input type="password" id="new-refresh-token" placeholder="refresh_token"><button type="button" class="pw-toggle" onclick="togglePw('new-refresh-token', this)">&#128065;</button></div></div></div><label style="display:flex;align-items:center;gap:8px;font-size:12px;color:var(--muted);margin:16px 0"><input type="checkbox" id="new-catalog"> Только каталог — без воспроизведения</label><div class="modal-actions"><button class="btn btn-primary" onclick="startOAuth()" id="oauthBtn">Подключить через OAuth</button><button class="btn" onclick="addAccount()">Добавить вручную</button></div></div></div>
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
    window.scrollTo({ top:0, behavior:'smooth' });
}

function toggleSidebar(open) {
    document.getElementById('sidebar').classList.toggle('open', !!open);
    document.getElementById('sidebarScrim').classList.toggle('open', !!open);
}

function openAddAccount() { document.getElementById('addAccountOverlay').classList.add('open'); setTimeout(function(){ document.getElementById('new-label').focus(); }, 100); }
function closeAddAccount() { document.getElementById('addAccountOverlay').classList.remove('open'); }

function filterAccounts(query) {
    var q = (query || '').trim().toLowerCase();
    document.querySelectorAll('#accounts-container .account-card').forEach(function(card) {
        card.style.display = !q || (card.dataset.search || '').indexOf(q) !== -1 ? '' : 'none';
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
        if (!res.ok) { showLogin('Не удалось подключиться к сервису: HTTP ' + res.status); return; }
        showApp();
        refreshAll();
    } catch(e) { showLogin('Не удалось подключиться к сервису: ' + e.message); }
}

async function refreshAll() {
    await Promise.all([fetchData(), loadRequestLog(), loadSettings(), loadProxyStatus(), loadAlertStatus(), loadCacheStats(), loadApiKeys()]);
}

function refreshCurrentView() {
    if (currentView === 'overview') { fetchData(); loadRequestLog(); }
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

function playbackCard(pb) {
    pb = pb || {};
    var active = pb.active != null ? pb.active : '—';
    var pending = pb.pending != null ? pb.pending : '—';
    var pool = pb.pool_size != null ? pb.pool_size : '—';
    return '<div class="stat-card"><div class="label">Воспроизведение · ' + active + '/' + pool + '</div><div class="value">' + pending + ' <span style="font-size:14px;color:var(--muted);font-weight:500">в очереди</span></div></div>';
}

function catalogCard(cat) {
    cat = cat || {};
    var mode = cat.mode || 'pool';
    var label = mode === 'static_token' ? 'Статичный токен' : (mode === 'account' ? esc(cat.label || 'Каталог') : 'Общий пул');
    var color = mode === 'pool' ? '#8b949e' : '#d2a8ff';
    return '<div class="stat-card"><div class="label">Каталог</div><div class="value" style="font-size:18px;color:' + color + '">' + label + '</div></div>';
}

function redisCard(redis) {    redis = redis || {};
    if (!redis.configured) {
        return '<div class="stat-card"><div class="label">Синхронизация Redis</div><div class="value" style="font-size:18px;color:#8b949e">Один сервер</div></div>';
    }
    var ep = redis.endpoint ? '<div style="font-size:11px;color:#8b949e;margin-top:4px;word-break:break-all">' + esc(redis.endpoint) + '</div>' : '';
    if (redis.status === 'ok') {
        return '<div class="stat-card" style="border-color:#3fb950"><div class="label">Синхронизация Redis</div><div class="value" style="color:#3fb950;font-size:18px">Работает</div>' + ep + '</div>';
    }
    return '<div class="stat-card" style="border-color:#f85149"><div class="label">Синхронизация Redis</div><div class="value" style="color:#f85149;font-size:18px">Нет связи</div>' + ep + '</div>';
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
            badge.className = r.ok ? 'card-stat test-pass' : 'card-stat test-fail';
            badge.innerHTML = 'Проверка <strong>' + (r.ok ? 'OK ' + r.ms + ' мс' : 'Ошибка ' + esc(r.error || '')) + '</strong>';
        }
    }
    document.getElementById('testResultsList').innerHTML = html;
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

        document.getElementById('stats').innerHTML =
            '<div class="stat-card"><div class="label">Всего запросов</div><div class="value">' + (stats.total_requests || 0) + '</div></div>' +
            '<div class="stat-card"><div class="label">Доля ошибок</div><div class="value">' + (stats.error_rate || '0.00%') + '</div></div>' +
            '<div class="stat-card"><div class="label">Активные аккаунты</div><div class="value">' + (stats.healthy_accounts || 0) + '<span style="font-size:15px;color:var(--muted);font-weight:500"> / ' + (stats.total_accounts || 0) + '</span></div></div>' +
            playbackCard(stats.playback) +
            catalogCard(stats.catalog) +
            redisCard(stats.redis);

        var html = '';
        if (accounts.accounts.length === 0) {
            html = '<div class="empty-state"><p>Аккаунтов пока нет</p><p class="hint">Подключите первый через OAuth — это займёт меньше минуты.</p><button class="btn btn-primary" style="margin-top:16px" onclick="openAddAccount()">Добавить аккаунт</button></div>';
        } else {
            for (var i = 0; i < accounts.accounts.length; i++) {
                var a = accounts.accounts[i];
                var label = a.label || a.id.slice(0, 8);
                var statusClass = 'status-dot ' + (a.is_active ? 'status-ok' : 'status-err');
                var statusText = a.is_active ? 'Активен' : 'Отключён';
                var activeCls = a.is_active ? ' btn-active' : '';
                var toggleText = a.is_active ? 'Включён' : 'Включить';
                var tokenStr = timeStr(a.token_expires_at);
                var uid = a.user_id || '-';
                var catalogBadge = a.is_catalog ? '<span class="status-label" style="color:#d2a8ff;background:rgba(210,168,255,0.1)">Каталог</span>' : '';
                var catalogBtn = a.is_catalog
                    ? '<button class="btn" onclick="setCatalog(\'' + a.id + '\',false)" title="Вернуть в пул воспроизведения">Из каталога</button>'
                    : '<button class="btn" onclick="setCatalog(\'' + a.id + '\',true)" title="Использовать только для метаданных">В каталог</button>';
                html += '<div class="account-card" data-search="' + esc((label + ' ' + uid + ' ' + a.client_id).toLowerCase()) + '">' +
                    '<div class="card-header">' +
                        '<div class="left"><span class="acc-num">' + (i + 1) + '</span><span class="' + statusClass + '"></span><span class="label">' + esc(label) + '</span><span class="status-label ' + (a.is_active ? 'status-ok' : 'status-err') + '">' + statusText + '</span>' + catalogBadge + '</div>' +
                        '<div class="card-actions">' +
                            '<button class="btn" onclick="refreshAccount(\'' + a.id + '\')">Обновить токен</button>' +
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
                        '<div class="cred-row"><span class="cred-key">Роль</span><span class="cred-value">' + (a.is_catalog ? 'Только каталог' : 'Воспроизведение') + '</span></div>' +
                    '</div>' +
                    '<div class="card-footer">' +
                        '<div class="card-stats">' +
                            '<span class="card-stat">Запросов <strong>' + a.request_count + '</strong></span>' +
                            '<span class="card-stat">Ошибок <strong>' + a.error_count + '</strong></span>' +
                            (a.auto_disabled ? '<span class="card-stat">Автовосстановление <strong>повтор</strong></span>' : '') +
                            '<span class="card-stat">Токен <strong>' + tokenStr + '</strong></span>' +
                            '<span class="card-stat test-badge" id="test-' + a.id + '" onclick="showTestDetails(\'' + a.id + '\')">Проверка <strong>—</strong></span>' +
                        '</div>' +
                    '</div>' +
                '</div>';
            }
        }
        document.getElementById('accounts-container').innerHTML = html;
        document.getElementById('accountCount').textContent = accounts.accounts.length + ' ' + plural(accounts.accounts.length, 'аккаунт', 'аккаунта', 'аккаунтов');
        filterAccounts(document.getElementById('accountSearch').value);
        renderTestResults(_testResults);
        return true;
    } catch(e) {
        document.getElementById('error').textContent = 'Failed: ' + e.message;
    }
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
    if (!confirm('Удалить этот аккаунт? Он сразу перестанет обслуживать запросы.')) return;
    try {
        var res = await fetch('/admin/accounts/' + id, {
            method: 'DELETE', headers: headers()
        });
        if (res.ok) fetchData();
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
    document.getElementById('new-label').value = 'Копия ' + (a.label || a.id.slice(0, 8));
    document.getElementById('new-user-id').value = a.user_id || '';
    document.getElementById('new-client-id').value = a.client_id || '';
    document.getElementById('new-client-secret').value = a.client_secret || '';
    document.getElementById('new-refresh-token').value = a.refresh_token || '';
    document.getElementById('new-catalog').checked = !!a.is_catalog;
    openAddAccount();
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
            fetchData();
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
    fetch('/admin/setup', { method: 'POST', headers: headers(), body: JSON.stringify({ label: lbl || null }) })
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
                setTimeout(function() { closeOAuth(); fetchData(); }, 1500);
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
    if (!url || url === '—' || url === 'Starting...') return;
    navigator.clipboard.writeText(url).then(function() {
        var btn = document.getElementById('copyOAuthBtn');
        btn.textContent = 'Скопировано';
        setTimeout(function() { btn.textContent = 'Копировать'; }, 2000);
    });
}

function openOAuthUrl() {
    var url = document.getElementById('oauthUrl').textContent;
    if (!url || url === '—' || url === 'Starting...') return;
    window.open(url, '_blank');
}

bootstrap();
setInterval(function() {
    if (!authenticated || document.hidden) return;
    if (currentView === 'overview' || currentView === 'accounts') fetchData();
    if (currentView === 'overview') loadRequestLog();
    if (currentView === 'access') loadApiKeys();
    if (currentView === 'system') { loadProxyStatus(); loadAlertStatus(); loadCacheStats(); }
}, 15000);

async function loadAlertStatus() {
    try {
        var res = await fetch('/admin/alerts', { headers: headers() });
        if (!res.ok) return;
        var a = (await res.json()).alerts || {};
        document.getElementById('al-discord').textContent = a.discord_configured ? 'Подключён' : 'Не настроен';
    } catch(e) {}
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
            var quota = (k.quota && k.quota > 0) ? (k.used + '/' + k.quota) : (k.used + '/∞');
            html += '<div class="cred-row"><span class="cred-key">' + esc(k.label || k.key_prefix) + '</span>' +
                '<span class="cred-value">' + esc(k.key_prefix) + '… · использовано ' + quota + ' · ' + (k.is_active ? 'активен' : 'отключён') + '</span>' +
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
    if (!confirm('Удалить API-ключ? Клиенты с этим ключом потеряют доступ.')) return;
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
    if (!confirm('Restore database from ' + file.name + '? Current accounts, keys and settings will be replaced.')) { e.target.value = ''; return; }
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

async function loadCacheStats() {
    try {
        var res = await fetch('/admin/cache', { headers: headers() });
        if (!res.ok) return;
        var c = (await res.json()).cache || {};
        document.getElementById('cc-hits').textContent = c.hits != null ? c.hits : '—';
        document.getElementById('cc-misses').textContent = c.misses != null ? c.misses : '—';
    } catch(e) {}
}

async function loadRequestLog() {
    try {
        var res = await fetch('/admin/requests?limit=20', { headers: headers() });
        if (!res.ok) return;
        var r = (await res.json()).requests || {};
        document.getElementById('rq-total').textContent = r.total != null ? r.total : '—';
        document.getElementById('rq-errors').textContent = r.errors != null ? r.errors : '—';
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
        var rows = '';
        for (var q of (r.recent || [])) {
            var cls = q.status >= 500 ? 'test-fail' : (q.status >= 400 ? 'test-pending' : 'test-pass');
            var t = '';
            if (q.ts) {
                var d = new Date(q.ts * 1000);
                t = ('0' + d.getHours()).slice(-2) + ':' + ('0' + d.getMinutes()).slice(-2) + ':' + ('0' + d.getSeconds()).slice(-2);
            }
            rows += '<div class="term-line"><span class="term-time">' + t + '</span> ' +
                '<span class="term-method m-' + q.method + '">' + q.method + '</span> ' +
                '<span class="term-path">' + esc(q.path) + '</span>' +
                (q.detail ? ' <span class="term-id">#' + esc(q.detail) + '</span>' : '') + ' ' +
                '<span class="' + cls + '">' + q.status + '</span> ' +
                '<span class="term-dim">' + q.latency_ms + 'ms ' + esc(q.client_ip) + '</span></div>';
        }
        var box = document.getElementById('rq-recent');
        if (rows) {
            box.innerHTML = rows + '<div class="term-line"><span class="term-dim">$</span> <span class="term-cursor"></span></div>';
            box.scrollTop = box.scrollHeight;
        } else {
            box.innerHTML = '<div class="term-line"><span class="term-dim">$ waiting for traffic…</span> <span class="term-cursor"></span></div>';
        }
    } catch(e) {}
}

async function loadProxyStatus() {
    try {
        var res = await fetch('/admin/proxies', { headers: headers() });
        if (!res.ok) return;
        var data = await res.json();
        var p = data.proxies || {};
        proxyEnabled = !!p.enabled;
        var trying = !p.last_try || Date.now() / 1000 - p.last_try < 20;
        document.getElementById('px-status').textContent = !p.enabled ? 'Напрямую' : (p.ready ? 'Активен' : (trying ? 'Проверяем прокси' : 'Нет рабочего прокси'));
        document.getElementById('px-current').textContent = p.current || (p.enabled ? '—' : 'direct');
        document.getElementById('px-pool').textContent = p.pool_size != null ? p.pool_size : '—';
        document.getElementById('px-fails').textContent = p.consecutive_fails != null ? p.consecutive_fails : '—';
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

async function loadSettings() {
    try {
        var res = await fetch('/admin/settings', { headers: headers() });
        if (!res.ok) return;
        var d = await res.json();
        var r = d.settings || d.rate_limits || {};
        document.getElementById('rl-autoheal').checked = r.auto_heal !== false;
        document.getElementById('rl-atmos').value = r.atmos_mode || 'off';
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
            fetchData();
        } else { document.getElementById('error').textContent = data.detail || 'Import failed'; }
    } catch(err) { document.getElementById('error').textContent = 'Import error: ' + err.message; }
    e.target.value = '';
}

async function saveSettings() {
    var body = {
        settings: {
            auto_heal: document.getElementById('rl-autoheal').checked,
            atmos_mode: document.getElementById('rl-atmos').value
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
            document.getElementById('rl-autoheal').checked = r.auto_heal !== false;
            document.getElementById('rl-atmos').value = r.atmos_mode || 'off';
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
