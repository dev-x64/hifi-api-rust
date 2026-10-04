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
    total_requests: 352374, requests_per_second_60s: 6.15, recent_p95_ms: 300,
    recent_requests: 5000, total_errors: 61, recent_error_rate_percent: 1.22,
    account_readiness: counts, playback_readiness: counts, catalog_readiness: empty,
    playback: { active: 0, pending: 0, pool_size: 13 }, catalog: { mode: 'pool' }, redis: { configured: false },
};

for (const language of ['en', 'ru']) {
    test(`overview separates current requests, concurrency and readiness (${language})`, () => {
        const { ctx } = context(language);
        ctx.stats = snapshot;
        const html = localized(ctx, vm.runInContext('overviewCards(stats)', ctx));
        assert.equal((html.match(/class="stat-card"/g) || []).length, 9);
        assert.equal((html.match(/class="ov-title"/g) || []).length, 3);
        assert.doesNotMatch(html, /0\/13|undefined|NaN/);
        assert.match(html, language === 'en' ? /Concurrent request limit<\/span><strong>13</ : /Лимит одновременно<\/span><strong>13</);
        assert.match(html, language === 'en' ? /No valid token<\/span><strong>6</ : /Без токена<\/span><strong>6</);
        assert.match(html, language === 'en' ? /FULL ready<\/span><strong>5</ : /Готовы к FULL<\/span><strong>5</);
        // The limit counts enabled accounts, so the gap to FULL-ready ones is spelled out.
        assert.match(html, /class="stat-note">[^]*<strong>5<\/strong>[^]*<strong>13<\/strong>/);
        // Ready, waiting and disabled segments are sized by account count; empty ones are omitted.
        assert.match(html, /status-ok" style="flex-grow:5"><\/i><i class="status-warn" style="flex-grow:8"><\/i><\/div>/);
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
    assert.match(html, /No playback accounts are enabled/);
});

test('the limit note disappears once every enabled playback account is FULL-ready', () => {
    const { ctx } = context();
    const ready = { ...counts, ready: 13, no_token: 0, needs_check: 0, metadata_ready: 13 };
    ctx.stats = { ...snapshot, account_readiness: ready, playback_readiness: ready };
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
