// SPDX-License-Identifier: Apache-2.0
const el = (id) => document.getElementById(id);
const invoke = window.__TAURI__.core.invoke;
let busy = true;
function message(text, error = false) {
  el('status').textContent = text;
  el('status').classList.toggle('error', error);
}
function validHost(value) {
  const host = value.trim();
  if (host.toLowerCase() === 'localhost') return true;
  if (/^(0|[1-9]\d{0,2})(\.(0|[1-9]\d{0,2})){3}$/.test(host)) {
    return host.split('.').every((part) => Number(part) <= 255);
  }
  if (host.includes(':') && /^[0-9a-fA-F:.]+$/.test(host)) {
    try { return new URL(`http://[${host}]/`).hostname.startsWith('['); } catch { return false; }
  }
  return false;
}
function availability() {
  const valid = validHost(el('host').value);
  const error = valid ? '' : '请输入有效的 IP 地址或 localhost，不要包含协议、端口或路径。';
  el('host').setCustomValidity(error);
  el('host').setAttribute('aria-invalid', String(!valid));
  el('host-error').textContent = error;
  el('host-error').hidden = valid;
  el('fields').disabled = busy;
  el('save').disabled = busy || !valid;
}
function render(state) {
  el('host').value = state.settings.host;
  el('port').value = state.settings.preferred_port;
  el('auto-port').checked = state.settings.auto_port;
  el('current-url').textContent = state.api_url || 'Gateway 未运行';
  el('run-label').textContent = state.api_url ? 'API 地址' : '当前运行状态';
  el('current-bind').textContent = state.actual_port
    ? `实际监听：${state.settings.host.includes(':') ? `[${state.settings.host}]` : state.settings.host}:${state.actual_port}${state.actual_port !== state.settings.preferred_port ? ' · 已自动换用端口' : ''}` : '';
  busy = state.busy;
  availability();
}
el('host').addEventListener('input', availability);
el('close').addEventListener('click', () => window.__TAURI__.window.getCurrentWindow().close());
el('settings-form').addEventListener('submit', async (event) => {
  event.preventDefault();
  if (busy || !el('settings-form').reportValidity()) return;
  const settings = { host: el('host').value.trim(), preferred_port: Number(el('port').value), auto_port: el('auto-port').checked };
  busy = true; availability();
  message('正在保存并重启 Gateway，请稍候…');
  try {
    const state = await invoke('apply_desktop_settings', { settings });
    render(state);
    message(`设置已生效。当前 API：${state.api_url}`);
  } catch (error) {
    try { render(await invoke('get_desktop_settings')); } catch { /* Keep the form recoverable. */ }
    message(String(error), true);
  } finally { busy = false; availability(); }
});
(async function load() {
  try {
    render(await invoke('get_desktop_settings'));
    if (busy) setTimeout(load, 250);
  }
  catch (error) { busy = false; availability(); message(`无法读取设置：${error}`, true); }
})();
