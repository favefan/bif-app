// SPDX-License-Identifier: Apache-2.0
// Presentation-only UI smoke runner. It never starts the Tauri host or touches user data.
import { spawn, execFile } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import fs from 'node:fs';
import fsp from 'node:fs/promises';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { Transform } from 'node:stream';
import { finished } from 'node:stream/promises';
import { createRequire } from 'node:module';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const desktop = path.join(repo, 'desktop');
const ui = path.join(repo, 'ui');
const resultsRoot = path.join(desktop, 'test-results');
const toolsRoot = path.join(desktop, '.tools');
const modes = new Map([
  ['ordinary', { desktopUI: '0', output: path.join(toolsRoot, 'oss-ui-standard') }],
  ['desktop', { desktopUI: '1', output: path.join(toolsRoot, 'oss-ui-desktop') }],
]);
const inheritedGatewaySettings = [
  'BIFROST_CONFIG_FILE',
  'BIFROST_UI_DEV',
  'BIFROST_PROFILER',
  'BIFROST_PPROF_PORT',
];

function fail(message) {
  throw new Error(`oss-ui-smoke: ${message}`);
}

function parseArgs(argv) {
  let mode = 'both';
  let config;
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--mode') {
      mode = argv[++index];
      if (!mode || mode.startsWith('--')) fail('--mode requires both, ordinary, or desktop');
    } else if (arg.startsWith('--mode=')) {
      mode = arg.slice('--mode='.length);
    } else if (arg === '--config') {
      config = argv[++index];
      if (!config || config.startsWith('--')) fail('--config requires a path');
    } else if (arg.startsWith('--config=')) {
      config = arg.slice('--config='.length);
    } else {
      fail(`unknown argument ${arg}; use --mode both|ordinary|desktop and optional --config <path>`);
    }
  }
  if (!['both', ...modes.keys()].includes(mode)) fail(`invalid mode ${mode}; use both, ordinary, or desktop`);
  if (config === '') fail('--config requires a path');
  return { mode, config };
}

function smokeEnv(overrides = {}) {
  const env = { ...process.env };
  for (const name of inheritedGatewaySettings) delete env[name];
  return { ...env, ...overrides };
}

async function findDesktopConfig(explicitConfig) {
  if (explicitConfig) {
    const resolved = path.resolve(repo, explicitConfig);
    if (!fs.existsSync(resolved)) fail(`Playwright config does not exist: ${resolved}`);
    return resolved;
  }
  const matches = [];
  async function walk(directory) {
    for (const entry of await fsp.readdir(directory, { withFileTypes: true })) {
      if (entry.name === 'node_modules' || entry.name === '.tools' || entry.name === 'target') continue;
      const candidate = path.join(directory, entry.name);
      if (entry.isDirectory()) await walk(candidate);
      else if (/playwright.*\.config\.ts$/i.test(entry.name)) matches.push(candidate);
    }
  }
  await walk(desktop);
  if (matches.length === 0) fail('no standalone desktop Playwright *.config.ts was found; pass --config <path> after it is added');
  if (matches.length !== 1) fail(`ambiguous desktop Playwright configs: ${matches.join(', ')}; pass --config <path>`);
  return matches[0];
}

async function freePort() {
  const server = net.createServer();
  await new Promise((resolve, reject) => server.once('error', reject).listen(0, '127.0.0.1', resolve));
  const { port } = server.address();
  await new Promise(resolve => server.close(resolve));
  return port;
}

function request(url) {
  return new Promise((resolve, reject) => {
    const req = http.get(url, { timeout: 2_000 }, response => {
      response.resume();
      resolve(response.statusCode);
    });
    req.on('timeout', () => req.destroy(new Error('request timed out')));
    req.on('error', reject);
  });
}

function observeChild(child) {
  let result;
  const exit = new Promise(resolve => {
    child.once('error', error => {
      result = { error };
      resolve(result);
    });
    child.once('exit', (code, signal) => {
      result = { code, signal };
      resolve(result);
    });
  });
  return { exit, get result() { return result; } };
}

function describeChildResult(result) {
  if (result.error) return `spawn error: ${result.error.message}`;
  if (result.signal) return `signal ${result.signal}`;
  return `exit ${result.code}`;
}

function settleWithin(promise, timeout) {
  return new Promise(resolve => {
    const timer = setTimeout(() => resolve(false), timeout);
    promise.then(() => {
      clearTimeout(timer);
      resolve(true);
    });
  });
}

function createRedactingLog(encryptionKey) {
  let pending = '';
  const tailLength = encryptionKey.length - 1;
  const redact = value => value.replaceAll(encryptionKey, '[REDACTED]');
  return new Transform({
    transform(chunk, encoding, callback) {
      pending += Buffer.isBuffer(chunk) ? chunk.toString('utf8') : chunk;
      // Replace complete matches before splitting; otherwise a match spanning
      // the emitted prefix and retained suffix would escape redaction.
      pending = redact(pending);
      const safeLength = Math.max(0, pending.length - tailLength);
      if (safeLength > 0) this.push(redact(pending.slice(0, safeLength)));
      pending = pending.slice(safeLength);
      callback();
    },
    flush(callback) {
      this.push(redact(pending));
      callback();
    },
  });
}

async function waitForHealth(child, childExit, port) {
  const deadline = Date.now() + 90_000;
  while (Date.now() < deadline) {
    if (childExit.result || child.exitCode !== null) {
      const result = childExit.result || { code: child.exitCode, signal: child.signalCode };
      fail(`gateway exited before health check (${describeChildResult(result)}); see ${path.join(resultsRoot, 'oss-ui-gateway.log')}`);
    }
    try {
      if (await request(`http://127.0.0.1:${port}/health`) === 200) return;
    } catch { /* The process may still be starting. */ }
    await new Promise(resolve => setTimeout(resolve, 250));
  }
  fail(`gateway did not become healthy within 90 seconds; see ${path.join(resultsRoot, 'oss-ui-gateway.log')}`);
}

async function stopGateway(child, childExit) {
  if (!child || childExit.result || child.exitCode !== null) return true;
  try {
    child.stdin?.end();
  } catch { /* The process may have failed while its stdio was being created. */ }
  if (await settleWithin(childExit.exit, 40_000)) return true;

  // The PID came from spawn in this runner, so the forced fallback targets only our owned process tree.
  if (!Number.isInteger(child.pid) || child.pid <= 0) return false;
  if (process.platform === 'win32') {
    await execFileAsync('taskkill', ['/pid', String(child.pid), '/t', '/f'], { timeout: 10_000, windowsHide: true }).catch(() => {});
  } else {
    try {
      child.kill('SIGKILL');
    } catch { /* The owned process may have exited between the timeout and kill. */ }
  }
  return settleWithin(childExit.exit, 10_000);
}

async function stopPreview(server) {
  if (server) await server.close();
}

async function closeLog(redactor, log) {
  if (!redactor || !log) return;
  redactor.end();
  await settleWithin(finished(log).catch(() => {}), 5_000);
}

async function buildUI(vite, name, details) {
  const previous = process.env.BIFROST_DESKTOP_HIDE_ENTERPRISE_UI;
  try {
    if (details.desktopUI === '1') process.env.BIFROST_DESKTOP_HIDE_ENTERPRISE_UI = 'true';
    else delete process.env.BIFROST_DESKTOP_HIDE_ENTERPRISE_UI;
    await vite.build({
      root: ui,
      configFile: path.join(ui, 'vite.config.mts'),
      build: { outDir: details.output, emptyOutDir: true },
    });
  } finally {
    if (previous === undefined) delete process.env.BIFROST_DESKTOP_HIDE_ENTERPRISE_UI;
    else process.env.BIFROST_DESKTOP_HIDE_ENTERPRISE_UI = previous;
  }
  if (!fs.existsSync(path.join(details.output, 'index.html'))) fail(`${name} Vite build did not produce index.html`);
}

async function runMode(vite, config, name, details, gateway) {
  await buildUI(vite, name, details);
  const previewPort = await freePort();
  const preview = await vite.preview({
    root: ui,
    configFile: path.join(ui, 'vite.config.mts'),
    preview: {
      host: '127.0.0.1', port: previewPort, strictPort: true,
      proxy: {
        '/api': { target: gateway, changeOrigin: true, ws: true },
        '/ws': { target: gateway, changeOrigin: true, ws: true },
      },
    },
    build: { outDir: details.output },
  });
  try {
    const cli = path.join(repo, 'tests', 'e2e', 'node_modules', '@playwright', 'test', 'cli.js');
    if (!fs.existsSync(cli)) fail(`Playwright CLI is missing: ${cli}; run npm ci --prefix tests/e2e`);
    const output = path.join(resultsRoot, `oss-ui-${name}`);
    await fsp.mkdir(output, { recursive: true });
    const env = smokeEnv({
      BASE_URL: `http://127.0.0.1:${previewPort}`,
      BIFROST_E2E_DESKTOP_UI: details.desktopUI,
      BIFROST_E2E_BROWSER_CHANNEL: process.env.BIFROST_E2E_BROWSER_CHANNEL || 'chrome',
      PLAYWRIGHT_CHANNEL: process.env.PLAYWRIGHT_CHANNEL || 'chrome',
      SKIP_WEB_SERVER: '1',
    });
    const test = spawn(process.execPath, [cli, 'test', '--config', config, '--output', output], {
      cwd: repo, env, stdio: 'inherit', windowsHide: true,
    });
    const testExit = observeChild(test);
    const result = await testExit.exit;
    if (result.error || result.code !== 0 || result.signal) {
      throw new Error(`${name} Playwright run failed with ${describeChildResult(result)}`);
    }
  } finally {
    await stopPreview(preview);
  }
}

async function main() {
  const { mode, config: explicitConfig } = parseArgs(process.argv.slice(2));
  // Do not let a developer's running gateway/UI settings alter this isolated smoke run.
  for (const name of inheritedGatewaySettings) delete process.env[name];
  const config = await findDesktopConfig(explicitConfig);
  const sidecar = path.resolve(process.env.BIFROST_OSS_UI_SIDECAR || path.join(desktop, 'src-tauri', 'binaries', 'bifrost-http.exe'));
  if (!fs.existsSync(sidecar)) fail(`bundled sidecar is missing: ${sidecar}; run ./desktop/scripts/build.ps1 sidecar first`);
  const viteRequire = createRequire(path.join(ui, 'package.json'));
  const vite = await import(pathToFileURL(viteRequire.resolve('vite')).href);

  let appDir;
  let child;
  let childExit;
  let redactor;
  let log;
  let cleanupFailed = false;
  try {
    await fsp.mkdir(resultsRoot, { recursive: true });
    appDir = await fsp.mkdtemp(path.join(os.tmpdir(), 'bif-app-oss-ui-'));
    const encryptionKey = randomBytes(32).toString('hex');
    redactor = createRedactingLog(encryptionKey);
    log = fs.createWriteStream(path.join(resultsRoot, 'oss-ui-gateway.log'), { flags: 'w' });
    redactor.pipe(log);
    const port = await freePort();
    child = spawn(sidecar, ['-app-dir', appDir, '-host', '127.0.0.1', '-port', String(port), '-shutdown-on-stdin-close'], {
      cwd: appDir,
      windowsHide: true,
      stdio: ['pipe', 'pipe', 'pipe'],
      env: smokeEnv({ BIFROST_ENCRYPTION_KEY: encryptionKey }),
    });
    childExit = observeChild(child);
    child.stdout.pipe(redactor, { end: false });
    child.stderr.pipe(redactor, { end: false });
    await waitForHealth(child, childExit, port);
    for (const [name, details] of modes) if (mode === 'both' || mode === name) await runMode(vite, config, name, details, `http://127.0.0.1:${port}`);
  } finally {
    if (child && childExit && !await stopGateway(child, childExit)) {
      cleanupFailed = true;
      console.error('oss-ui-smoke: owned gateway did not exit after bounded shutdown');
    }
    await closeLog(redactor, log);
    // mkdtemp created this exact leaf under the OS temp directory; never remove another path.
    if (appDir && path.dirname(appDir) === path.resolve(os.tmpdir()) && path.basename(appDir).startsWith('bif-app-oss-ui-')) {
      await fsp.rm(appDir, { recursive: true, force: true });
    }
  }
  if (cleanupFailed) fail('owned gateway cleanup failed');
}

main().catch(error => { console.error(error.message); process.exitCode = 1; });
