// SPDX-License-Identifier: Apache-2.0
// Native WebView integration QA. Only run in a disposable Windows CI profile.
const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const fs = require('node:fs/promises');
const path = require('node:path');
const http = require('node:http');
const { chromium } = require('../.tools/qa/node_modules/playwright');
if (process.env.CI !== 'true') throw new Error('Settings integration requires a disposable CI Windows profile.');
const exe = path.resolve(__dirname, '../src-tauri/target/debug/bif-app.exe');
const evidence = path.resolve(__dirname, '../test-results');
let host, browser, settingsPage, dashboard, blocker;
const errors = [];
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
    if (host.exitCode !== null) throw new Error(`Host exited: ${host.exitCode}`);
    return chromium.connectOverCDP('http://127.0.0.1:9229');
  }, 'WebView debugging connection');
  settingsPage = await until(() => browser.contexts().flatMap((c) => c.pages()).find((p) => p.url().endsWith('/settings.html')), 'Settings window');
  settingsPage.on('pageerror', (error) => errors.push(error.message));
  await until(() => settingsPage.locator('#save').isEnabled(), 'Settings ready');
  dashboard = await until(() => browser.contexts().flatMap((c) => c.pages()).find((p) => p.url().startsWith('http://127.0.0.1:')), 'Original Bifrost window');
}
async function stop() {
  if (host && host.exitCode === null) { host.kill(); await until(() => host.exitCode !== null, 'Host exits', 10000); }
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
    await screenshot('initial');
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

    await settingsPage.locator('#host').selectOption('0.0.0.0');
    assert(await settingsPage.locator('#save').isDisabled());
    await settingsPage.locator('#allow-lan').check();
    await save();
    assert((await settingsPage.locator('#current-bind').textContent()).includes('0.0.0.0:'));
    assert((await settingsPage.locator('#current-url').textContent()).includes('127.0.0.1:'));
    await settingsPage.setViewportSize({ width: 480, height: 580 });
    await screenshot('lan-small');
    assert(await settingsPage.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth));
    await settingsPage.emulateMedia({ colorScheme: 'dark' });
    await screenshot('lan-dark');
    await settingsPage.emulateMedia({ colorScheme: 'light' });

    await settingsPage.locator('#host').selectOption(original.host);
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
    console.log('PASS: isolated native Settings WebView, IPC denied to original UI, validation, rollback, fallback, preference persistence, restart, LAN consent, loopback UI, light/dark/minimum layout, independent close');
    await stop();
    await until(async () => { try { await fetch(`http://127.0.0.1:${restored}/health`, { signal: AbortSignal.timeout(1000) }); return false; } catch { return true; } }, 'Owned sidecar cleaned after QA');
  } finally {
    if (blocker) await new Promise((resolve) => blocker.close(resolve));
    await stop();
  }
})().catch((error) => { console.error(error); process.exitCode = 1; });
