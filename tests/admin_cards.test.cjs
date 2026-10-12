// Run with: node --test tests/admin_cards.test.cjs
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { test } = require('node:test');

const root = path.resolve(__dirname, '..');
const i18n = fs.readFileSync(path.join(root, 'src/admin/i18n.js'), 'utf8');
const source = fs.readFileSync(path.join(root, 'src/admin/ui.rs'), 'utf8');
const script = source.match(/<script>([\s\S]*?)<\/script>/)[1].replace('__HIFI_I18N__', i18n);
new vm.Script(script); // Validate the complete embedded script, including its bootstrap.

function context(language = 'en') {
    const elements = new Map();
    const ctx = vm.createContext({
        window: { _accounts: [] },
        document: { getElementById: id => {
            if (!elements.has(id)) elements.set(id, { innerHTML: '', classList: { toggle() {} } });
            return elements.get(id);
        } },
        proxyListDirty: false,
        headers: () => ({}),
    });
    vm.runInContext(i18n, ctx);
    vm.runInContext(script.slice(script.indexOf('function timeStr('), script.indexOf('var _testResults = {};')), ctx);
    vm.runInContext(script.slice(script.indexOf('function esc(s)'), script.indexOf('function togglePw(')), ctx);
    ctx.adminLanguage = language;
    return { ctx, elements };
}

function localized(ctx, html) {
    return html.replace(/>([^<>]+)</g, (_, text) => '>' + ctx.tr(text) + '<')
        .replace(/title="([^"]*)"/g, (_, title) => 'title="' + ctx.tr(title) + '"');
}

const counts = { total: 13, enabled: 13, ready: 5, disabled: 0, no_token: 6, needs_check: 2, preview_only: 0, rate_limited: 0, metadata_ready: 7 };
const empty = Object.fromEntries(Object.keys(counts).map(key => [key, 0]));
const snapshot = {
    total_requests: 352374, recent_p95_ms: 300,
    request_rates: { all: { now: 7.4, avg_60s: 6.15, peak_60s: 12 }, playback: { now: 2.2, avg_60s: 1.75, peak_60s: 3 }, search: { now: 4, avg_60s: 3.5, peak_60s: 6 },
        catalog: { now: 1.2, avg_60s: 0.9, peak_60s: 4 }, cached: { now: 0.6, avg_60s: 0.5, peak_60s: 1234 }, throttled_60s: 3,
        history: Array.from({ length: 60 }, (_, second) => second === 59 ? [2, 6, 4] : (second === 10 ? [3, 0, 0] : [0, 0, 0])) },
    recent_requests: 5000, total_errors: 61, recent_error_rate_percent: 1.22,
    account_readiness: counts, playback_readiness: counts, catalog_readiness: empty,
    playback: { active: 0, pending: 0, pool_size: 7 }, catalog: { mode: 'pool' }, redis: { configured: false },
};

for (const language of ['en', 'ru']) {
    test(`overview separates current requests, concurrency and readiness (${language})`, () => {
        const { ctx } = context(language);
        ctx.stats = snapshot;
        const html = localized(ctx, vm.runInContext('overviewCards(stats)', ctx));
        assert.equal((html.match(/class="stat-card[ "]/g) || []).length, 9);
        assert.equal((html.match(/class="ov-title"/g) || []).length, 3);
        assert.doesNotMatch(html, /0\/13|undefined|NaN/);
        assert.match(html, language === 'en' ? /Concurrent request limit<\/span><strong>7</ : /Лимит одновременно<\/span><strong>7</);
        // Requests per second: the headline is the current total; each class shows now, the 60-second average, then its busiest second.
        const n = value => language === 'en' ? value : value.replace('.', ',');
        assert.match(html, new RegExp('class="value">' + n('7.40') + ' <span class="metric-unit">' + (language === 'en' ? 'now' : 'сейчас') + '<'));
        assert.ok(html.includes('<span class="rate-head">' + (language === 'en' ? 'Peak' : 'Пик') + '</span>'));
        for (const [en, ru, now, avg, peak] of [['Total', 'Всего', '7.40', '6.15', '12'], ['Playback', 'Воспроизведение', '2.20', '1.75', '3'], ['Search', 'Поиск', '4.00', '3.50', '6'],
            ['Catalog', 'Каталог', '1.20', '0.90', '4'], ['From cache', 'Из кэша', '0.60', '0.50', language === 'en' ? '1,234' : '1\u00a0234']]) {
            assert.ok(html.includes('>' + (language === 'en' ? en : ru) + '</span><strong>' + n(now) + '</strong><strong>' + n(avg) + '</strong><strong>' + peak + '</strong>'), en);
        }
        // One bar per second, scaled to the busiest one; each class keeps its own colour and the tooltip spells it out.
        assert.equal((html.match(/class="rate-bar"/g) || []).length, 60);
        assert.match(html, /<i class="rate-playback" style="height:16\.7%"><\/i><i class="rate-search" style="height:50\.0%"><\/i><i class="rate-catalog" style="height:33\.3%"><\/i>/);
        assert.match(html, /<i class="rate-playback" style="height:25\.0%"><\/i><\/div>/);
        assert.ok(html.includes(language === 'en' ? 'data-tip="1 s ago\nPlayback: 2\nSearch: 6\nCatalog: 4" data-edge="end"' : 'data-tip="1 с назад\nВоспроизведение: 2\nПоиск: 6\nКаталог: 4" data-edge="end"'));
        // Limits: the load each serving account carries, and Tidal's throttling.
        const unit = ' <span class="rate-unit">' + (language === 'en' ? '/s' : '/с') + '</span></strong>';
        assert.ok(html.includes((language === 'en' ? 'Playback per account' : 'Воспроизведение на один аккаунт') + '</span><strong>' + n('0.25') + unit));
        assert.ok(html.includes((language === 'en' ? 'Metadata per account' : 'Метаданные на один аккаунт') + '</span><strong>' + n('0.63') + unit));
        assert.match(html, language === 'en' ? /status-warn"><\/span><span>429 responses in 60 s<\/span><strong>3</ : /status-warn"><\/span><span>Ответов 429 за 60 с<\/span><strong>3</);
        assert.match(html, language === 'en' ? /No valid token<\/span><strong>6</ : /Без токена<\/span><strong>6</);
        assert.match(html, language === 'en' ? /FULL ready<\/span><strong>5</ : /Готовы к FULL<\/span><strong>5</);
        // The limit follows accounts that can serve now, so the gap to enabled ones is spelled out.
        assert.match(html, /class="stat-note">[^]*<strong>7<\/strong>[^]*<strong>13<\/strong>/);
        // The accounts card is a plain list of counts, without a chart.
        assert.doesNotMatch(vm.runInContext('accountsCard(stats)', ctx), /stat-bar|flex-grow/);
        assert.match(html, /<strong>7<\/strong>/); // Metadata tokens do not require FULL.
        if (language === 'en') assert.doesNotMatch(html, /[А-Яа-яЁё]/);
    });
}

test('empty traffic has no invented error rate or latency', () => {
    const { ctx } = context();
    ctx.stats = { ...snapshot, total_requests: 0, recent_requests: 0, total_errors: 0, recent_error_rate_percent: null, recent_p95_ms: null,
        account_readiness: empty, playback_readiness: empty, playback: { active: 0, pending: 0, pool_size: 1 } };
    const html = localized(ctx, vm.runInContext('overviewCards(stats)', ctx));
    assert.doesNotMatch(html, /0\.00%|undefined|NaN/);
    assert.match(html, /No requests have completed yet/);
    // A stats payload without rates (older server) renders dashes, not zeros.
    ctx.stats.request_rates = undefined;
    const bare = vm.runInContext('rateCard(stats.request_rates)', ctx);
    assert.match(bare, /class="value">— /);
    assert.doesNotMatch(bare, /rate-chart|rate-unit|undefined|NaN/);
    assert.equal((bare.match(/<strong>—<\/strong>/g) || []).length, 15);
    // A static metadata token has no accounts to spread the load over.
    assert.doesNotMatch(vm.runInContext('rateCard(' + JSON.stringify(snapshot.request_rates) + ', {catalog:{mode:"static_token"}})', ctx), /Метаданные на один аккаунт/);
    assert.match(html, /No playback accounts are enabled/);
});

test('the limit note disappears once every enabled playback account can serve', () => {
    const { ctx } = context();
    const ready = { ...counts, ready: 13, no_token: 0, needs_check: 0, metadata_ready: 13 };
    ctx.stats = { ...snapshot, request_rates: { ...snapshot.request_rates, throttled_60s: 0 },
        account_readiness: ready, playback_readiness: ready, playback: { active: 0, pending: 0, pool_size: 13 } };
    const html = localized(ctx, vm.runInContext('overviewCards(stats)', ctx));
    assert.doesNotMatch(html, /stat-note/);
    assert.doesNotMatch(html, /status-warn/);
});

test('proxy status is an overview card fed by the proxy endpoint', () => {
    const { ctx } = context();
    assert.match(vm.runInContext('proxyCard(null)', ctx), /—/);
    ctx.proxy = { enabled: true, status: 'Частично проверены', tone: 'status-warn', verified: 1, assigned: 2, pool_size: 20 };
    const html = localized(ctx, vm.runInContext('proxyCard(proxy)', ctx));
    assert.match(html, /status-warn"><\/span><span>Partially verified/);
    assert.match(html, /Verified assignments<\/span><strong>1 \/ 2</);
    assert.match(html, /In pool<\/span><strong>20</);
    ctx.proxy = { enabled: false, status: 'Напрямую', tone: 'status-unknown' };
    assert.match(localized(ctx, vm.runInContext('proxyCard(proxy)', ctx)), /Direct[^]*without a proxy/);
});

test('disabled Redis does not imply there is only one server, and endpoints are escaped', () => {
    const { ctx } = context();
    const disabled = localized(ctx, vm.runInContext('redisCard({configured:false})', ctx));
    assert.match(disabled, /Not configured/);
    assert.doesNotMatch(disabled, /Single server/);
    ctx.redis = { configured: true, status: 'unreachable', endpoint: '<script>bad()</script>' };
    const offline = localized(ctx, vm.runInContext('redisCard(redis)', ctx));
    assert.match(offline, /Redis unreachable/);
    assert.doesNotMatch(offline, /<script>/);
});

test('static metadata token is described as configured, not verified', () => {
    const { ctx } = context();
    const html = localized(ctx, vm.runInContext('catalogCard({mode:"static_token"}, {}, {enabled:2})', ctx));
    assert.match(html, /access is checked on request/);
    assert.match(html, /Fallback catalog accounts:/);
});

test('account status distinguishes enabled from ready and identifies unusable tokens', () => {
    const { ctx } = context();
    for (const state of ['no_token', 'needs_check', 'preview_only', 'rate_limited']) {
        ctx.account = { is_active: true, availability: { state, token: 'expired' } };
        assert.equal(vm.runInContext('accountStatus(account).tone', ctx), 'status-warn');
    }
    ctx.account = { availability: { state: 'ready' }, is_catalog: false };
    assert.equal(vm.runInContext('accountStatus(account).tone', ctx), 'status-ok');
    ctx.account = { availability: { state: 'disabled' }, auto_disabled: false };
    assert.equal(vm.runInContext('accountStatus(account).text', ctx), 'Отключён вручную');
    ctx.account = { availability: { token: 'rejected' }, token_expires_at: Date.now() / 1000 + 3600 };
    assert.equal(vm.runInContext('tokenDescription(account)', ctx), 'Отклонён Tidal');
});

test('API keys at their quota are shown as exhausted, including the exact boundary', async () => {
    const { ctx, elements } = context();
    ctx.fetch = async () => ({ ok: true, json: async () => ({ api_keys: [
        { id: 'a', label: 'Exhausted', key_prefix: 'a', is_active: true, used: 10, quota: 10 },
        { id: 'b', label: 'Unlimited', key_prefix: 'b', is_active: true, used: 100, quota: 0 },
        { id: 'c', label: 'Disabled', key_prefix: 'c', is_active: false, used: 0, quota: 10 },
    ] }) });
    vm.runInContext(script.slice(script.indexOf('async function loadApiKeys('), script.indexOf('async function addApiKey(')), ctx);
    await vm.runInContext('loadApiKeys()', ctx);
    const html = localized(ctx, elements.get('keys-container').innerHTML);
    assert.match(html, /Quota exhausted/);
    assert.match(html, /Available/);
    assert.match(html, /Disabled/);
    assert.match(html, /100 \/ ∞/);
});

test('one verified proxy assignment is not described as a fully verified pool', async () => {
    const { ctx, elements } = context();
    ctx.fetch = async () => ({ ok: true, json: async () => ({ persistent: true, proxies: {
        enabled: true, ready: true, pool_size: 20, entries: [], assignments: [
            { account_id: 'a', proxy: 'proxy-a', verified: true },
            { account_id: 'b', proxy: 'proxy-b', verified: false },
        ],
    } }) });
    vm.runInContext(script.slice(script.indexOf('async function loadProxyStatus('), script.indexOf('async function updateProxies(')), ctx);
    await vm.runInContext('loadProxyStatus()', ctx);
    assert.equal(elements.get('px-status').textContent, 'Частично проверены');
    assert.equal(elements.get('px-verified').textContent, '1 / 2');
});

test('cache card shows the hit rate, and a dash before any lookup', async () => {
    const { ctx, elements } = context();
    vm.runInContext(script.slice(script.indexOf('async function loadCacheStats('), script.indexOf('var requestLogLimit')), ctx);
    ctx.fetch = async () => ({ ok: true, json: async () => ({ cache: { hits: 4699, misses: 30496, stale: 364, negative: 1, coalesced: 0 } }) });
    await vm.runInContext('loadCacheStats()', ctx);
    assert.equal(elements.get('cc-rate').textContent, '13.4%');
    ctx.adminLanguage = 'ru';
    await vm.runInContext('loadCacheStats()', ctx);
    assert.equal(elements.get('cc-rate').textContent, '13,4%');
    ctx.fetch = async () => ({ ok: true, json: async () => ({ cache: { hits: 0, misses: 0 } }) });
    await vm.runInContext('loadCacheStats()', ctx);
    assert.equal(elements.get('cc-rate').textContent, '—');
});

test('uptime reset is sent only after confirmation and then reloads accounts', async () => {
    const { ctx, elements } = context();
    const calls = [];
    let reloads = 0;
    ctx.fetch = async (url, options) => { calls.push([url, options.method]); return { ok: true, json: async () => ({ accounts: 2 }) }; };
    ctx.fetchData = () => { reloads++; };
    vm.runInContext(script.slice(script.indexOf('async function resetUptime('), script.indexOf('async function clearPlaybackQueue(')), ctx);
    ctx.confirm = question => { assert.match(question, /^Reset the uptime history of all accounts\?/); return false; };
    await vm.runInContext('resetUptime()', ctx);
    assert.deepEqual(calls, []);
    ctx.confirm = () => true;
    await vm.runInContext('resetUptime()', ctx);
    assert.deepEqual(calls, [['/admin/uptime/reset', 'POST']]);
    assert.equal(reloads, 1);
    assert.equal(ctx.tr(elements.get('success').textContent), 'Uptime history reset.');
    assert.equal(elements.get('resetUptimeBtn').disabled, false);
    // A failed reset is reported and leaves the cards alone.
    ctx.fetch = async () => ({ ok: false, status: 500, json: async () => ({ detail: 'Database error' }) });
    await vm.runInContext('resetUptime()', ctx);
    assert.equal(ctx.tr(elements.get('error').textContent), 'Uptime reset: Database error');
    assert.equal(reloads, 1);
});

for (const language of ['en', 'ru']) {
    test(`uptime is one strip and retains mixed states within each bar (${language})`, () => {
        const { ctx } = context(language);
        ctx.uptime = { window_start: 0, window_end: 6000, percentage: 50,
            up_seconds: 2950, down_seconds: 50, waiting_seconds: 100, unknown_seconds: 2900,
            segments: [
                { start: 0, end: 2900, active: null, status: null },
                { start: 2900, end: 2950, active: true, status: 'up' },
                { start: 2950, end: 3050, active: true, status: 'waiting' },
                { start: 3050, end: 3100, active: false, status: 'down' },
                { start: 3100, end: 6000, active: true, status: 'up' },
            ] };
        const html = vm.runInContext('uptimeCard(uptime)', ctx);
        assert.equal((html.match(/class="uptime-bar"/g) || []).length, 60);
        assert.doesNotMatch(html, /uptime-day|uptime-dates|undefined|NaN/);
        assert.match(html, /uptime-span uptime-up" style="height:50%/);
        assert.match(html, /uptime-span uptime-waiting" style="height:50%/);
        assert.match(html, /uptime-span uptime-down" style="height:50%/);
        // Every bar stacks in the same order, so the valid-token share reads as one chart.
        assert.match(html, /uptime-up" style="height:50%"><\/span><span class="uptime-span uptime-waiting"/);
        assert.match(html, /uptime-waiting" style="height:50%"><\/span><span class="uptime-span uptime-down"/);
        if (language === 'en') assert.doesNotMatch(html, /[А-Яа-яЁё]/);
    });

    test(`expired token shows the refresh failure, safely escaped (${language})`, () => {
        const { ctx } = context(language);
        ctx.account = { availability: { token: 'expired' }, heal_next_retry: Date.now() / 1000 + 60,
            last_refresh_error: 'Tidal auth HTTP 403: <script>bad()</script>' };
        const html = vm.runInContext('tokenRefreshDetails(account)', ctx);
        assert.match(html, /HTTP 403/);
        assert.match(html, /&lt;script&gt;/);
        assert.doesNotMatch(html, /<script>/);
        assert.match(html, language === 'en' ? /Token refresh failed:/ : /Обновление токена не удалось:/);
        ctx.account.last_refresh_error = null;
        const expired = vm.runInContext('tokenRefreshDetails(account)', ctx);
        assert.match(expired, language === 'en' ? /expiry time/ : /Срок действия/);
        ctx.account.availability.token = 'valid';
        ctx.account.heal_next_retry = 0;
        ctx.account.last_refresh_error = 'Old error';
        assert.equal(vm.runInContext('tokenRefreshDetails(account)', ctx), '');
    });
}

function errorsContext(language) {
    const { ctx, elements } = context(language);
    vm.runInContext(script.slice(script.indexOf('function logTime('), script.indexOf('async function loadProxyStatus(')), ctx);
    return { ctx, elements };
}

for (const language of ['en', 'ru']) {
    test(`recent errors explain the cause and name the accounts tried (${language})`, () => {
        const { ctx } = errorsContext(language);
        ctx.errors = [
            { ts: 1, method: 'GET', path: '/track/:id', detail: '361994292', status: 503, latency_ms: 1514, client_ip: '104.28.225.117', cache: '',
                error: 'Preview only for track: account abcdef0123 cannot provide FULL (subscription required)',
                accounts: [{ id: 'abcdef0123456', label: 'Main', role: 'playback' }, { id: 'ffff00001111', label: '<b>Spare</b>', role: 'catalog' }] },
            { ts: 2, method: 'GET', path: '/track/:id', detail: '63137944', status: 409, latency_ms: 163, client_ip: '152.53.148.121', cache: '',
                error: 'Tidal returned audio for track 1 for requested 63137944: refusing to serve a different track',
                accounts: [{ id: 'abcdef0123456', label: 'Main', role: 'playback' }] },
            { ts: 3, method: 'GET', path: '/search', detail: 'Копия <i>', status: 429, latency_ms: 4, client_ip: '10.0.0.1', cache: 'NEGATIVE', error: '', accounts: [] },
        ];
        const panel = vm.runInContext('errorsPanel(errors)', ctx);
        const list = localized(ctx, panel.list);
        assert.equal(panel.count, 3);
        assert.equal((list.match(/class="err-item"/g) || []).length, 3);
        assert.match(list, language === 'en' ? /<strong>Preview only<\/strong> <span>Tidal returned a 30-second snippet/ : /<strong>Только превью<\/strong> <span>Tidal отдал 30-секундный фрагмент/);
        assert.match(list, language === 'en' ? /<strong>Track substituted<\/strong>/ : /<strong>Подмена трека<\/strong>/);
        assert.match(list, language === 'en' ? /<strong>Tidal rate limit<\/strong>/ : /<strong>Лимит Tidal<\/strong>/);
        // The server's own message and the account chain are shown as received.
        assert.match(list, /data-no-i18n>Preview only for track: account abcdef0123 cannot provide FULL/);
        assert.match(list, /ACCOUNT Main \[abcdef01\] → &lt;b&gt;Spare&lt;\/b&gt; \[ffff0000\] · catalog</);
        assert.match(list, /class="test-fail">503</);
        assert.match(list, /class="test-pending">409</);
        assert.match(list, /CACHE NEGATIVE/);
        assert.doesNotMatch(list, /<b>|<i>|undefined|NaN/);
        // Causes and accounts are tallied across the list, most frequent first.
        const causes = localized(ctx, panel.causes);
        assert.match(causes, language === 'en' ? /^<span>Causes:<\/span> / : /^<span>Причины:<\/span> /);
        assert.equal((causes.match(/×1</g) || []).length, 3);
        assert.match(panel.accounts, /<span data-no-i18n>Main<\/span> <strong>×2<\/strong><\/span><span><span data-no-i18n>&lt;b&gt;Spare/);
        if (language === 'en') assert.doesNotMatch(causes + list.replace(/Копия/, ''), /[А-Яа-яЁё]/);
    });
}

test('every error cause is recognised from the server message or, failing that, the status', () => {
    const { ctx, elements } = errorsContext('en');
    const cause = (status, error) => { ctx.q = { status, error }; return ctx.tr(vm.runInContext('errorCause(q)[0]', ctx)); };
    for (const [status, error, expected] of [
        [409, 'Tidal returned manifest for track unknown for requested 5: refusing to serve a different track', 'Track substituted'],
        [503, 'Preview only (FULL_REQUIRES_SUBSCRIPTION): track 5 not available as FULL', 'Preview only'],
        [503, 'Tidal did not return HIGH audio', 'No HIGH quality'],
        [503, 'Playback queue full (40/40); retry shortly', 'Queue full'],
        [503, 'Catalog accounts are at their concurrent request limit', 'Catalog busy'],
        [503, 'All accounts are inactive or have expired tokens', 'No working accounts'],
        [503, 'All accounts failed after fallback', 'No working accounts'],
        [500, 'No Tidal credentials available; add an account via the admin panel', 'No working accounts'],
        [503, 'No working proxy available and direct fallback is disabled', 'Proxy unavailable'],
        [503, 'Connection error to Tidal', 'Tidal unreachable'],
        [429, 'Upstream timeout', 'Timeout'],
        [410, 'Playback request was cancelled', 'Request cancelled'],
        [429, 'Tidal rate limited the account', 'Tidal rate limit'],
        [404, 'Resource not found', 'Not found'],
        [400, 'Missing id', 'Bad request'],
        [401, 'Tidal rejected refreshed token', 'Token rejected'],
        [403, 'Upstream API error', 'Access denied'],
        [502, 'Upstream API error', 'Tidal error'],
        [200, 'Failed to parse Tidal response: EOF', 'Tidal error'],
        [503, 'Account changed during refresh', 'Service unavailable'],
        [500, 'Database error', 'Internal error'],
        [418, '', 'Other error'],
        [404, undefined, 'Not found'],
    ]) assert.equal(cause(status, error), expected, status + ' ' + error);

    // The panel is collapsed until opened, and an empty log says so instead of showing a blank box.
    assert.match(source, /<details class="terminal errors-panel" id="errorsPanel">\s*<summary/);
    vm.runInContext('renderErrors([])', ctx);
    assert.equal(elements.get('er-count').textContent, 0);
    assert.equal(elements.get('er-causes').innerHTML, '');
    assert.match(localized(ctx, elements.get('er-list').innerHTML), /No errors yet\./);
});
