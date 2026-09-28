// SPDX-License-Identifier: Apache-2.0
// Native WebView integration QA. Only run in a disposable Windows CI profile.
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const fs = require('node:fs/promises');
const path = require('node:path');
const http = require('node:http');
const os = require('node:os');
const hostCases = require('../tests/host-cases.json');
const { chromium } = require('../.tools/qa/node_modules/playwright');
if (process.env.CI !== 'true') throw new Error('Settings integration requires a disposable CI Windows profile.');
const exe = path.resolve(__dirname, '../src-tauri/target/debug/bif-app.exe');
const evidence = path.resolve(__dirname, '../test-results');
let host, browser, settingsPage, dashboard, blocker;
const errors = [];
const exited = (process) => process.exitCode !== null || process.signalCode !== null;
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function until(fn, description, ms = 120000) {
  const deadline = Date.now() + ms;
  let last;
  do {
    try { const value = await fn(); if (value) return value; } catch (error) { last = error.message; }
    await delay(250);
  } while (Date.now() < deadline);
  throw new Error(`${description}${last ? ': ' + last : ''}`);
}
async function start() {
  host = spawn(exe, ['--test-settings-window'], { windowsHide: true, stdio: 'ignore', env: {
    ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=9229'
  }});
  host.on('error', (error) => errors.push(error.message));
  browser = await until(async () => {
    if (exited(host)) throw new Error(`Host exited: ${host.exitCode ?? host.signalCode}`);
    return chromium.connectOverCDP('http://127.0.0.1:9229');
  }, 'WebView debugging connection');
  settingsPage = await until(() => browser.contexts().flatMap((c) => c.pages()).find((p) => p.url().endsWith('/settings.html')), 'Settings window');
  settingsPage.on('pageerror', (error) => errors.push(error.message));
  await until(() => settingsPage.locator('#save').isEnabled(), 'Settings ready');
  dashboard = await until(() => browser.contexts().flatMap((c) => c.pages()).find((p) => p !== settingsPage && p.url().startsWith('http://') && !p.url().includes('tauri.localhost')), 'Original Bifrost window');
}
async function stop() {
  if (host && !exited(host)) { host.kill(); await until(() => exited(host), 'Host exits', 10000); }
  await until(async () => {
    try { await fetch('http://127.0.0.1:9229/json/version', { signal: AbortSignal.timeout(500) }); return false; }
    catch { return true; }
  }, 'Previous test WebView exits', 15000);
  browser = undefined;
}
async function port() { return Number((await settingsPage.locator('#current-url').textContent()).match(/:(\d+)\/v1/)[1]); }
async function save(expectError = false) {
  await settingsPage.locator('#save').click();
  await until(async () => {
    const text = await settingsPage.locator('#status').textContent();
    return expectError ? text.includes('已恢复之前') : text.includes('设置已生效');
  }, expectError ? 'Failed change rolls back' : 'Settings applied');
  await until(() => settingsPage.locator('#save').isEnabled(), 'Controls re-enabled');
}
async function screenshot(name) { await settingsPage.screenshot({ path: path.join(evidence, `settings-${name}.png`), fullPage: true }); }
(async () => {
  await fs.mkdir(evidence, { recursive: true });
  try {
    await start();
    const original = { host: await settingsPage.locator('#host').inputValue(), preferred: await settingsPage.locator('#port').inputValue(), auto: await settingsPage.locator('#auto-port').isChecked() };
    assert.equal(original.host, '127.0.0.1');
    assert.equal(await settingsPage.locator('#host').getAttribute('placeholder'), '127.0.0.1');
    assert.equal(await settingsPage.locator('#host').getAttribute('type'), 'text');
    await screenshot('initial');
    for (const value of hostCases.invalid) {
      await settingsPage.locator('#host').fill(value);
      assert(await settingsPage.locator('#save').isDisabled(), `Invalid host enabled Save: ${value}`);
      assert(await settingsPage.locator('#host-error').isVisible());
    }
    await screenshot('invalid-host');
    for (const value of hostCases.valid) {
      await settingsPage.locator('#host').fill(value);
      assert(await settingsPage.locator('#save').isEnabled(), `Valid host rejected: ${value}`);
      assert(await settingsPage.locator('#host-error').isHidden());
    }
    await settingsPage.locator('#host').fill(original.host);
    const denied = await dashboard.evaluate(async () => {
      try { await window.__TAURI__.core.invoke('get_desktop_settings'); return false; }
      catch { return true; }
    });
    assert(denied, 'Original Bifrost UI must not access desktop IPC');
    const beforeInvalid = await port();
    await settingsPage.locator('#port').fill('0');
    await settingsPage.locator('#save').click();
    assert.equal(await settingsPage.locator('#port').evaluate((node) => node.validity.valid), false);
    assert.equal(await port(), beforeInvalid);
    assert.equal((await fetch(`http://127.0.0.1:${beforeInvalid}/health`)).status, 200);

    blocker = http.createServer((req, res) => { res.writeHead(200); res.end('unowned fixture'); });
    await new Promise((resolve) => blocker.listen(0, '127.0.0.1', resolve));
    const occupied = blocker.address().port;
    await settingsPage.locator('#port').fill(String(occupied));
    await settingsPage.locator('#auto-port').uncheck();
    await save(true);
    assert.equal(await port(), beforeInvalid);
    assert(blocker.listening);
    await screenshot('rollback');

    await settingsPage.locator('#port').fill(String(occupied));
    await settingsPage.locator('#auto-port').check();
    await save();
    const actual = await port();
    assert.notEqual(actual, occupied);
    assert.equal(await settingsPage.locator('#port').inputValue(), String(occupied));
    await until(() => dashboard.url().startsWith(`http://127.0.0.1:${actual}/`), 'Original UI follows actual port');
    assert.equal(await dashboard.title(), 'Bifrost');
    await screenshot('fallback');
    await stop();
    await start();
    assert.equal(await port(), actual);
    assert.equal(await settingsPage.locator('#port').inputValue(), String(occupied));

    await settingsPage.locator('#host').fill('0.0.0.0');
    await save();
    const lanPort = await port();
    assert.notEqual(lanPort, occupied, 'Wildcard bind must not share an unowned localhost listener');
    const lanModels = await (await fetch(`http://127.0.0.1:${lanPort}/v1/models`)).json();
    assert(Array.isArray(lanModels.data), 'LAN mode must still reach the owned Bifrost API');
    await until(() => dashboard.url().startsWith(`http://127.0.0.1:${lanPort}/`), 'LAN-mode original UI follows actual port');
    assert((await settingsPage.locator('#current-bind').textContent()).includes('0.0.0.0:'));
    assert((await settingsPage.locator('#current-url').textContent()).includes('127.0.0.1:'));
    await settingsPage.setViewportSize({ width: 480, height: 580 });
    await screenshot('lan-small');
    assert(await settingsPage.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth));
    await settingsPage.emulateMedia({ colorScheme: 'dark' });
    await screenshot('lan-dark');
    await settingsPage.emulateMedia({ colorScheme: 'light' });

    const nic = Object.values(os.networkInterfaces()).flat().find((entry) => entry && !entry.internal && entry.family === 'IPv4');
    assert(nic, 'Windows runner has a usable assigned IPv4 address');
    for (const hostValue of ['localhost', '127.0.0.2', '::1', nic.address]) {
      await settingsPage.locator('#host').fill(hostValue);
      await save();
      const api = await settingsPage.locator('#current-url').textContent();
      const ui = api.replace(/\/v1$/, '');
      assert.equal(new URL(api).hostname, hostValue === 'localhost' ? '127.0.0.1' : hostValue === '::1' ? '[::1]' : hostValue);
      assert.equal((await fetch(`${ui}/health`)).status, 200);
      assert(Array.isArray((await (await fetch(`${api}/models`)).json()).data));
      await until(() => dashboard.url().startsWith(`${ui}/`), 'Dashboard follows custom IP');
      assert.equal(await dashboard.title(), 'Bifrost');
      await screenshot(hostValue === nic.address ? 'custom-nic' : hostValue === '::1' ? 'ipv6' : 'custom-host');
    }
    const persistedHost = await settingsPage.locator('#host').inputValue();
    const persistedApi = await settingsPage.locator('#current-url').textContent();
    await stop();
    await start();
    assert.equal(await settingsPage.locator('#host').inputValue(), persistedHost);
    assert.equal(await settingsPage.locator('#current-url').textContent(), persistedApi);
    await settingsPage.locator('#host').fill(original.host);
    await settingsPage.locator('#port').fill(original.preferred);
    await settingsPage.locator('#auto-port').setChecked(original.auto);
    await save();
    const restored = await port();
    assert.equal((await fetch(`http://127.0.0.1:${restored}/health`)).status, 200);
    assert.equal((await fetch(`http://127.0.0.1:${restored}/v1/models`)).status, 200);
    await settingsPage.locator('#close').click();
    await until(() => settingsPage.isClosed(), 'Settings closes independently');
    assert(!dashboard.isClosed());
    assert.equal((await fetch(`http://127.0.0.1:${restored}/health`)).status, 200);
    assert.deepEqual(errors, []);
    console.log('PASS: isolated native Settings WebView, IPC denied to original UI, validation, rollback, fallback, preference persistence, restart, custom IPv4/IPv6/localhost validation, assigned NIC and persisted endpoint navigation, light/dark/minimum layout, independent close');
    await stop();
    await until(async () => { try { await fetch(`http://127.0.0.1:${restored}/health`, { signal: AbortSignal.timeout(1000) }); return false; } catch { return true; } }, 'Owned sidecar cleaned after QA');
  } finally {
    if (blocker) await new Promise((resolve) => blocker.close(resolve));
    await stop();
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
