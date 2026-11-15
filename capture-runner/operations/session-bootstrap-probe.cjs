'use strict';
// Fresh official demo bootstrap only. No clicks, spins, purchase calls, WS sends,
// stealth modifications, proxy selection, certificate bypasses or CAPTCHA solving.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { chromium, request } = require('playwright');
const VERSION = require('playwright/package.json').version;
const PROVIDERS = ['evoplay', 'gamesglobal', 'kagaming', 'kendoo', 'redtiger', 'threeoaks'];
const EXPECTED = {
  evoplay: ['venicefestival', 'treeoflightfabledfruits'], gamesglobal: ['GoldBlitzExtreme', '12MasksofFireDrums'],
  kagaming: ['TropicalHoliday', 'ThiefDog'], kendoo: ['mariachi-lock', 'coin-dragon'],
  redtiger: ['pandagoesboom000', 'happycappy000000'], threeoaks: ['lava_coins_2', 'lady_fortune']
};
const MiB = 1024 * 1024;
// Exact entries from capture-runner/bindings/kendoo-extra.json, not slug inference.
const KENDOO_NATIVE = { 'mariachi-lock': 'mariachi_lock', 'coin-dragon': 'coin_dragon' };
process.umask(0o077);
const args = process.argv.slice(2);
function arg(name) { const i = args.indexOf(name); return i < 0 ? undefined : args[i + 1]; }
function save(file, data) { fs.writeFileSync(file, JSON.stringify(data, null, 2) + '\n', { mode: 0o600 }); }
function log(file, data) { fs.appendFileSync(file, JSON.stringify({ at: new Date().toISOString(), ...data }) + '\n', { mode: 0o600 }); }
function hash(bytes) { return crypto.createHash('sha256').update(bytes).digest('hex'); }
function delay(ms) { return new Promise(resolve => setTimeout(resolve, ms)); }
function urlFor(c) {
  const game = encodeURIComponent(c.game);
  if (c.provider === 'evoplay') return 'https://demo.evoplay.games/demo/fullstate/html5/evoplay/' + game;
  if (c.provider === 'gamesglobal') return 'https://www.gamesglobal.com/api/launch/' + game + '?launchAsMobile=false';
  if (c.provider === 'kagaming') return 'https://gamesdemo.kaga88.com/?' + new URLSearchParams({ g: c.game, p: 'demo', u: String(crypto.randomInt(100000000, 1000000000)), t: '1238', ak: 'accessKey', cr: 'USD', loc: 'es' });
  if (c.provider === 'kendoo') return 'https://kendoo.com/games/' + game;
  if (c.provider === 'redtiger') return 'https://redtiger.com/demo/' + game + '?showNavbar=true';
  return 'https://3oaks.com/api/v1/games/' + game + '/play?lang=en';
}
function belongs(provider, value) {
  let u; try { u = new URL(value); } catch { return false; }
  if (!['https:', 'wss:'].includes(u.protocol)) return false;
  const h = u.hostname;
  const suffix = d => h === d || h.endsWith('.' + d);
  return provider === 'evoplay' ? suffix('evoplay.games') || suffix('demo-evoplay.games')
    : provider === 'gamesglobal' ? ['gamesglobal.com', 'valueactive.eu', 'valueactive.com', 'gameassists.co.uk', 'gameassists.com'].some(suffix)
    : provider === 'kagaming' ? ['gamesdemo.kaga88.com', 'rmpdemo.kaga88.com'].includes(h)
    : provider === 'kendoo' ? h === 'kendoo.com' || h === 'betman-demo.head.kendoo.pro'
    : provider === 'redtiger' ? ['redtiger.com', 'jinx.prod.cmsevo.com', 'fansite.evo-games.com'].includes(h) || suffix('redtiger.cash')
    : suffix('3oaks.com');
}
function staticClient(c, r) {
  if (r.method !== 'GET' || r.type !== 'document' || r.status !== 200 || !belongs(c.provider, r.url)) return false;
  const u = new URL(r.url), p = u.pathname;
  // Read-only client documents actually requested by the browser. Never re-call
  // session issuers, /entry, /api/launch, /play, API actions or a WS endpoint.
  return c.provider === 'evoplay' ? !p.includes('/demo/fullstate/') && (p.endsWith('.html') || p.endsWith('/'))
    : c.provider === 'gamesglobal' ? p.startsWith('/htmlgames/game/')
    : c.provider === 'kagaming' ? u.hostname === 'gamesdemo.kaga88.com' && p === '/'
    : c.provider === 'kendoo' ? u.hostname === 'betman-demo.head.kendoo.pro' && p === '/betman-demo/static/game.html'
    : c.provider === 'redtiger' ? u.hostname === 'fansite.evo-games.com' && p.startsWith('/frontend/evo/')
    : u.hostname === 'static.3oaks.com' && p.startsWith('/gs/clients_');
}
function marks(body) {
  const s = body.replace(/<!--[\s\S]*?-->/g, '')
    .replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, '')
    .replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi, '').toLowerCase();
  return { captcha: /verify you are human|(?:complete|solve)[^<\n]{0,60}captcha|class=["'][^"']*(?:cf-turnstile|g-recaptcha|h-captcha)/.test(s),
    geo: /not available in (your|this) (country|region)|country is (blocked|restricted)|ipblocked/.test(s),
    challenge: /just a moment|cloudflare ray id|cf-mitigated|attention required|access denied/.test(s) };
}
function nativeSession(provider, url, body) {
  if (!belongs(provider, url)) return false;
  let v; try { v = JSON.parse(body); } catch { return false; }
  const text = x => typeof x === 'string' && x.length > 0;
  if (provider === 'evoplay') return v.init && typeof v.init === 'object' && v.init.ranges != null;
  if (provider === 'kagaming') return text(v.si) && v.gm === 0 && v.e === false;
  if (provider === 'gamesglobal') return url.includes('/accounts/login/demo') && Array.isArray(v.tokens?.launchTokens) && v.tokens.launchTokens.length > 0;
  if (provider === 'redtiger') return url.includes('/platform/game/settings') && v.success === true && v.result?.user?.userId != null;
  return ['login', 'start'].includes(v.command) && v.status?.code === 'OK' && text(v.session_id);
}
function initialized(provider, url, body) {
  if (!belongs(provider, url)) return false;
  let v; try { v = JSON.parse(body); } catch { return false; }
  if (provider === 'gamesglobal') return /\/casino\/play\/public\/v1\/games\/module\/(101053|101417)\/client\/50300\/play(?:\?|$)/.test(url) && v.context?.isReadOnly === true && v.packet?.packetType === 8 && v.packet?.isBase64Encoded === false && typeof v.packet?.payload === 'string' && /<Id\b[^>]*\bverb=["']Refresh["']/.test(v.packet.payload);
  if (provider === 'kendoo' || provider === 'threeoaks') return v.command === 'start' && v.status?.code === 'OK' && typeof v.session_id === 'string' && v.session_id.length > 0 && v.context?.round_finished === true;
  return !!nativeSession(provider, url, body);
}
async function apiGet(api, url, label, dir, headers = {}) {
  const record = { label, request: { method: 'GET', url, headers }, outcome: 'transport_error' };
  try {
    record.cookie_jar_before = (await api.storageState()).cookies;
    const response = await api.get(url, { timeout: 7000, maxRedirects: 0, failOnStatusCode: false, headers });
    const body = await response.body();
    record.status = response.status(); record.response_headers = response.headersArray();
    record.url = response.url(); record.body_bytes = body.length; record.body_sha256 = hash(body);
    record.body_base64 = body.subarray(0, 4 * MiB).toString('base64'); record.truncated = body.length > 4 * MiB;
    record.cookie_jar_after = (await api.storageState()).cookies;
    record.outcome = 'response'; await response.dispose();
  } catch (e) { record.error = String(e); }
  save(path.join(dir, label + '.json'), record);
  return { status: record.status ?? null, outcome: record.outcome };
}
async function runCase(browser, c, output) {
  const dir = path.join(output, 'private', c.provider, c.game);
  fs.mkdirSync(path.join(dir, 'bodies'), { recursive: true, mode: 0o700 });
  const events = path.join(dir, 'events.jsonl'), pending = new Set(), docs = [], requestIds = new WeakMap();
  let requestSeq = 0;
  const requestId = req => { if (!requestIds.has(req)) requestIds.set(req, ++requestSeq); return requestIds.get(req); };
  const summary = { provider: c.provider, game: c.game, outcome: 'unconfirmed', status_counts: {},
    request_count: 0, response_count: 0, websocket_connections: 0, websocket_frames_sent: 0, websocket_frames_received: 0,
    cookie_count: 0, cookie_names: [], storage_origins: 0, session_found: false, initialized: false, captcha: false, geo_denied: false,
    challenge: false, body_files: 0, body_bytes: 0, body_omitted: 0, screenshot_count: 0, deadline_reached: false };
  const start = Date.now(), launch = urlFor(c); let context, timer, pageSeq = 0;
  let gamesGlobalAuth = false, gamesGlobalRefresh = false;
  const track = promise => { pending.add(promise); promise.catch(e => log(events, { event: 'capture_error', error: String(e) })).finally(() => pending.delete(promise)); };
  const remain = () => Math.max(1, 90000 - (Date.now() - start));
  try {
    context = await browser.newContext({ viewport: { width: 1365, height: 900 }, locale: 'en-US',
      recordHar: { path: path.join(dir, 'network.har'), content: 'omit', mode: 'full' } });
    timer = setTimeout(() => { summary.deadline_reached = true; context.close().catch(() => {}); }, 90000);
    context.on('request', req => {
      summary.request_count++;
      const id = requestId(req);
      track((async () => log(events, { event: 'http_request', id, method: req.method(), url: req.url(), type: req.resourceType(),
        headers: await req.headersArray(), post_data_base64: req.postDataBuffer()?.toString('base64') ?? null,
        redirected_from: req.redirectedFrom()?.url() ?? null }))());
    });
    context.on('requestfailed', req => log(events, { event: 'request_failed', id: requestId(req), url: req.url(), failure: req.failure() }));
    context.on('response', res => track((async () => {
      const req = res.request(), id = requestId(req), status = res.status(), type = req.resourceType();
      summary.response_count++; summary.status_counts[status] = (summary.status_counts[status] || 0) + 1;
      const headers = await res.headersArray(), headerMap = await res.allHeaders();
      const row = { id, event: 'http_response', url: res.url(), status, method: req.method(), type, headers };
      if (type === 'document') docs.push({ ...row, request_headers: await req.allHeaders() });
      log(events, row);
      const candidate = ['document', 'xhr', 'fetch'].includes(type) || (type === 'script' && belongs(c.provider, res.url()));
      if (!candidate || summary.body_bytes >= 64 * MiB || Number(headerMap['content-length'] || 0) > 12 * MiB) { summary.body_omitted++; return; }
      const body = await res.body(), cap = Math.min(12 * MiB, 64 * MiB - summary.body_bytes);
      const kept = body.subarray(0, Math.max(0, cap));
      const file = 'bodies/' + String(id).padStart(5, '0') + '.bin';
      fs.writeFileSync(path.join(dir, file), kept, { mode: 0o600 }); summary.body_files++; summary.body_bytes += kept.length;
      log(events, { event: 'http_body', id, file, original_bytes: body.length, captured_bytes: kept.length, sha256: hash(kept), truncated: kept.length !== body.length });
      if (type !== 'script') {
        const text = kept.toString('utf8');
        if (belongs(c.provider, res.url()) && [401, 403].includes(status)) {
          const m = marks(text); summary.captcha ||= m.captcha; summary.geo_denied ||= m.geo; summary.challenge ||= m.challenge;
        }
        if (status >= 200 && status < 300) {
          summary.session_found ||= !!nativeSession(c.provider, res.url(), text);
          if (c.provider === 'gamesglobal') {
            try { const v = JSON.parse(text); gamesGlobalAuth ||= res.url().includes('/Shared/Communications/Public/v1/accounts/authenticate') && v.userType === 5 && v.sessionProductId === 22619 && typeof v.userTokenDetails?.userToken === 'string' && v.userTokenDetails.userToken.length > 0; } catch {}
            gamesGlobalRefresh ||= initialized(c.provider, res.url(), text);
            summary.initialized ||= gamesGlobalAuth && gamesGlobalRefresh;
          } else summary.initialized ||= initialized(c.provider, res.url(), text);
        }
      }
    })()));
    context.on('page', page => {
      const pageId = ++pageSeq;
      page.on('console', msg => log(events, { event: 'console', page: pageId, type: msg.type(), text: msg.text() }));
      page.on('pageerror', e => log(events, { event: 'page_error', page: pageId, error: String(e) }));
      page.on('websocket', ws => {
        summary.websocket_connections++; log(events, { event: 'ws_open', page: pageId, url: ws.url() });
        for (const [kind, count] of [['framesent', 'websocket_frames_sent'], ['framereceived', 'websocket_frames_received']]) ws.on(kind, frame => {
          summary[count]++; const binary = Buffer.isBuffer(frame.payload);
          log(events, { event: 'ws_' + kind, page: pageId, url: ws.url(), binary, payload_base64: Buffer.from(frame.payload).toString('base64') });
        });
        ws.on('close', () => log(events, { event: 'ws_close', page: pageId, url: ws.url() }));
        ws.on('socketerror', e => log(events, { event: 'ws_error', page: pageId, url: ws.url(), error: String(e) }));
      });
      track((async () => {
        const cdp = await context.newCDPSession(page); await cdp.send('Network.enable');
        for (const event of ['webSocketCreated', 'webSocketWillSendHandshakeRequest', 'webSocketHandshakeResponseReceived', 'webSocketFrameError', 'webSocketClosed'])
          cdp.on('Network.' + event, data => log(events, { event: 'cdp_' + event, page: pageId, data }));
      })());
    });
    const page = await context.newPage();
    const userAgent = await page.evaluate(() => navigator.userAgent);
    save(path.join(dir, 'runtime.json'), { playwright: VERSION, browser: browser.version(), user_agent: userAgent,
      node: process.version, platform: process.platform, headed: true, launch_url: launch, started_at: new Date(start).toISOString() });
    const fresh = await request.newContext({ userAgent });
    try { summary.preflight = await apiGet(fresh, launch, 'api-before-clean', dir, { 'user-agent': userAgent }); } finally { await fresh.dispose(); }
    try { await page.goto(launch, { waitUntil: 'domcontentloaded', timeout: Math.min(25000, remain()) }); }
    catch (e) { log(events, { event: 'navigation_error', error: String(e) }); }
    if (c.provider === 'kendoo' && !page.isClosed()) {
      const observed = await page.evaluate(() => ({ links: Array.from(document.querySelectorAll('a[href],iframe[src]'), e => e.href || e.src), next: Array.isArray(window.__next_f) ? window.__next_f : [] }));
      const links = observed.links.slice();
      // Exact public bootstrap field already used by the native adapter; no guessed route.
      const hydration = observed.next.filter(row => Array.isArray(row) && typeof row[1] === 'string').map(row => row[1]).join('');
      for (const m of hydration.matchAll(/"linkToDemoWebsite"\s*:\s*("(?:[^"\\]|\\.)*")/g)) { try { links.push(JSON.parse(m[1])); } catch {} }
      const candidates = [...new Set(links)].filter(s => { try { const u = new URL(s); return u.origin === 'https://betman-demo.head.kendoo.pro' && u.pathname === '/betman-demo/static/game.html' && u.searchParams.get('game') === KENDOO_NATIVE[c.game] && u.searchParams.get('wl') === 'demo'; } catch { return false; } });
      log(events, { event: 'kendoo_observed_demo_links', candidates });
      if (candidates.length === 1) await page.goto(candidates[0], { waitUntil: 'domcontentloaded', timeout: Math.min(20000, remain()) });
    }
    const until = Math.min(start + 60000, Date.now() + 30000);
    while (Date.now() < until && !page.isClosed()) {
      if (belongs(c.provider, page.url())) try {
        const visible = await page.locator('body').innerText({ timeout: 1000 });
        const m = marks(visible); summary.captcha ||= m.captcha; summary.geo_denied ||= m.geo; summary.challenge ||= m.challenge;
      } catch {}
      if (summary.geo_denied || summary.captcha || (summary.initialized && Date.now() > start + 20000)) break;
      await delay(500);
    }
    const client = [...docs].reverse().find(r => staticClient(c, r));
    summary.comparison = { outcome: 'no_observed_static_client' };
    if (client && !summary.captcha && !summary.geo_denied && remain() > 17000) {
      // Same observed URL and harmless headers; only cookie jar differs. These GETs
      // do not execute JavaScript. APIRequestContext is not the Rust reqwest transport.
      const headers = { 'user-agent': userAgent };
      for (const name of ['accept', 'accept-language', 'referer', 'origin']) if (client.request_headers[name]) headers[name] = client.request_headers[name];
      const clean = await request.newContext({ userAgent });
      let cleanResult; try { cleanResult = await apiGet(clean, client.url, 'api-after-clean', dir, headers); } finally { await clean.dispose(); }
      const sharedResult = await apiGet(context.request, client.url, 'api-after-browser-cookies', dir, headers);
      summary.comparison = { outcome: 'compared', browser_status: client.status, clean: cleanResult, browser_cookies: sharedResult };
    } else if (summary.captcha || summary.geo_denied) summary.comparison = { outcome: 'stopped_at_access_control' };
    summary.outcome = summary.geo_denied ? 'geo_denied' : summary.captcha ? 'captcha' : summary.initialized ? 'native_initialized' : summary.session_found ? 'native_session_seen' : summary.challenge ? 'challenge_or_denial' : 'unconfirmed';
  } catch (e) { summary.outcome = 'capture_error'; log(events, { event: 'case_error', error: String(e) }); }
  finally {
    if (context) try {
    const state = await context.storageState({ indexedDB: true }); save(path.join(dir, 'storage-state.json'), state);
    summary.cookie_count = state.cookies.length; summary.storage_origins = state.origins.length;
    summary.cookie_names = [...new Set(state.cookies.map(x => /^[A-Za-z0-9_.:-]{1,80}$/.test(x.name) ? x.name : '[other]'))].sort();
    } catch (e) { log(events, { event: 'storage_error', error: String(e) }); }
    let i = 0;
    if (context)
    for (const p of context.pages().slice(0, 3)) {
      const prefix = 'page-' + (++i);
      try { await p.screenshot({ path: path.join(dir, prefix + '.png'), fullPage: false, timeout: Math.min(3000, remain()) }); summary.screenshot_count++; } catch (e) { log(events, { event: 'screenshot_error', error: String(e) }); }
      try { const html = await p.content(); fs.writeFileSync(path.join(dir, prefix + '.html'), html.slice(0, 2 * MiB), { mode: 0o600 }); } catch {}
      for (const frame of p.frames().slice(0, 20)) try {
        const storage = await frame.evaluate(() => ({ url: location.href, session_storage: Object.fromEntries(Object.entries(sessionStorage)) }));
        log(path.join(dir, 'session-storage.jsonl'), storage);
      } catch (e) { log(events, { event: 'session_storage_error', error: String(e) }); }
    }
    if (context) { try { await context.close(); } catch (e) { log(events, { event: 'close_error', error: String(e) }); } }
    clearTimeout(timer); await Promise.allSettled([...pending]);
    if (summary.outcome !== 'capture_error') summary.outcome = summary.geo_denied ? 'geo_denied' : summary.captcha ? 'captcha' : summary.initialized ? 'native_initialized' : summary.session_found ? 'native_session_seen' : summary.challenge ? 'challenge_or_denial' : 'unconfirmed';
    if (summary.deadline_reached) summary.outcome = 'deadline';
    summary.duration_ms = Date.now() - start; save(path.join(dir, 'summary.json'), summary);
  }
  return summary;
}
async function main() {
  const configFile = arg('--config'), output = arg('--output');
  if (!configFile || !output) throw new Error('Required: --config PATH --output PATH [--validate]');
  const config = JSON.parse(fs.readFileSync(configFile, 'utf8'));
  if (VERSION !== '1.62.1') throw new Error('Expected Playwright 1.62.1');
  if (config.version !== 1 || config.cases.length !== 12) throw new Error('Expected 12 reviewed cases');
  for (const provider of PROVIDERS) {
    const selected = config.cases.filter(c => c.provider === provider).map(c => c.game).sort();
    if (JSON.stringify(selected) !== JSON.stringify([...EXPECTED[provider]].sort())) throw new Error('Unexpected game selection');
  }
  if (args.includes('--validate')) { process.stdout.write('Validated 12 cases, 6 provider lanes; no browser or network started.\n'); return; }
  if (fs.existsSync(output)) throw new Error('Output already exists; use a fresh directory');
  fs.mkdirSync(path.join(output, 'private'), { recursive: true, mode: 0o700 });
  fs.mkdirSync(path.join(output, 'public'), { recursive: true, mode: 0o700 });
  save(path.join(output, 'private', 'config.json'), config);
  let browser;
  try {
    browser = await chromium.launch({ headless: false, timeout: 30000 });
    const coreRoot = path.dirname(require.resolve('playwright-core/package.json'));
    const registry = JSON.parse(fs.readFileSync(path.join(coreRoot, 'browsers.json'), 'utf8'));
    const expected = registry.browsers.find(x => x.name === 'chromium');
    save(path.join(output, 'private', 'runtime.json'), { playwright: VERSION, chromium_expected: expected, chromium_observed: browser.version(), executable: chromium.executablePath(), node: process.version });
    if (browser.version() !== expected.browserVersion) throw new Error('Chromium version differs from pinned Playwright browser');
    const results = await Promise.all(PROVIDERS.map(async provider => {
      const rows = []; for (const c of config.cases.filter(x => x.provider === provider)) rows.push(await runCase(browser, c, output)); return rows;
    }));
    const cases = results.flat();
    const report = { format: 'session-bootstrap-public-summary', version: 1, cases };
    save(path.join(output, 'public', 'summary.json'), report);
    const rows = cases.map(c => '| ' + [c.provider, c.game, c.outcome, c.preflight?.status ?? '-', c.comparison?.clean?.status ?? '-', c.comparison?.browser_cookies?.status ?? '-', c.session_found, c.initialized, c.cookie_count, c.websocket_frames_received].join(' | ') + ' |');
    fs.writeFileSync(path.join(output, 'public', 'summary.md'), '# Demo session bootstrap\n\n| Provider | Game | Outcome | Preflight HTTP | Clean client HTTP | Browser cookies HTTP | Session observed | Init observed | Cookies | WS received |\n|---|---|---|---|---|---|---|---|---|---|\n' + rows.join('\n') + '\n\nNo spins or purchases were requested. HTTP comparison uses Playwright APIRequestContext; it is not reqwest. A session observed is not a completed round.\n');
    process.stdout.write('Finished ' + cases.length + ' cases; public summary written.\n');
    if (cases.some(c => ['capture_error', 'deadline'].includes(c.outcome))) process.exitCode = 2;
  } catch (e) { save(path.join(output, 'private', 'fatal.json'), { error: String(e) }); throw new Error('Probe failed; inspect encrypted private evidence.'); }
  finally { if (browser) await browser.close(); }
}
main().catch(e => { process.stderr.write(String(e.message) + '\n'); process.exitCode = 1; });
