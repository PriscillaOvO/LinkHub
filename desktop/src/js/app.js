// ── Tauri IPC helper ─────────────────────────────────────────────────

async function tauriInvoke(cmd, args = {}) {
  try {
    return await window.__TAURI_INTERNALS__.invoke(cmd, args);
  } catch (err) {
    throw new Error(typeof err === 'string' ? err : (err.message || JSON.stringify(err)));
  }
}

// ── Settings (stored in localStorage) ──────────────────────────────

const DEFAULTS = {
  identityPath: 'secure:C:\\LinkHub\\local-identity.secure.txt',
  trustStorePath: 'C:\\LinkHub\\trust-store.txt',
  listenerAddr: '127.0.0.1:8787',
  receiveDir: 'C:\\LinkHub\\inbox',
  historyPath: 'C:\\LinkHub\\history.json',
};

function getSetting(key) {
  return localStorage.getItem('linkhub_' + key) || DEFAULTS[key] || '';
}
function setSetting(key, value) {
  localStorage.setItem('linkhub_' + key, value);
}

function getPeerAddress(deviceId) {
  if (!deviceId) return '';
  return localStorage.getItem('linkhub_peer_addr_' + deviceId) || '';
}

function setPeerAddress(deviceId, address) {
  if (!deviceId || !address) return;
  localStorage.setItem('linkhub_peer_addr_' + deviceId, address);
}

// ── Tab switching ──────────────────────────────────────────────────

document.querySelectorAll('.tab-bar .tab').forEach(btn => {
  btn.addEventListener('click', () => {
    document.querySelectorAll('.tab-bar .tab').forEach(b => b.classList.remove('active'));
    document.querySelectorAll('.tab-content').forEach(s => s.classList.remove('active'));
    btn.classList.add('active');
    const target = document.getElementById('tab-' + btn.dataset.tab);
    if (target) target.classList.add('active');

    if (btn.dataset.tab === 'devices') renderDevicesTab();
    if (btn.dataset.tab === 'send') renderSendTab();
    if (btn.dataset.tab === 'history') renderHistoryTab();
    if (btn.dataset.tab === 'service') { buildServiceTab(); checkListenerStatus(); }
  });
});

// ── Event delegation ───────────────────────────────────────────────
// Inline on* handlers (onclick="fn()") do NOT fire in Tauri's packaged
// builds: Tauri injects a nonce into the CSP script-src, which makes the
// browser ignore 'unsafe-inline', disabling inline event-handler attributes.
// So all interactive elements declare data-act / data-change instead, and a
// single delegated listener invokes the named global function with the
// data-a0..a3 string arguments.
function collectActionArgs(el) {
  const args = [];
  for (let i = 0; el.dataset['a' + i] !== undefined; i++) args.push(el.dataset['a' + i]);
  return args;
}

document.addEventListener('click', (e) => {
  const el = e.target.closest('[data-act]');
  if (!el) return;
  const fn = window[el.dataset.act];
  if (typeof fn === 'function') fn(...collectActionArgs(el));
});

document.addEventListener('change', (e) => {
  const el = e.target.closest('[data-change]');
  if (!el) return;
  const fn = window[el.dataset.change];
  if (typeof fn === 'function') fn(...collectActionArgs(el));
});

// ── Status bar ─────────────────────────────────────────────────────

function setStatus(msg, type) {
  const el = document.getElementById('connection-status');
  el.textContent = msg;
  el.style.color = type === 'error' ? 'var(--danger)' : type === 'ok' ? 'var(--success)' : '';
}

// ── Utility ────────────────────────────────────────────────────────

function showMessage(containerId, text, type) {
  const container = document.getElementById(containerId);
  if (!container) return;
  const div = document.createElement('div');
  div.className = 'msg msg-' + type;
  div.textContent = text;
  container.appendChild(div);
  setTimeout(() => div.remove(), 6000);
}

// ── Auto-discovery (background mDNS refresh) ────────────────────────
// Periodically resolves trusted devices' LAN addresses so the Devices and
// Send tabs stay current without the user pressing "Scan LAN". Reuses the
// existing scan_trusted_mdns command + setPeerAddress storage.

const DISCOVERY_INTERVAL_MS = 8000;
const PEER_OFFLINE_AFTER_MS = 24000; // ~3 missed cycles -> offline

function setPeerLastSeen(deviceId, ts) {
  if (!deviceId) return;
  localStorage.setItem('linkhub_peer_seen_' + deviceId, String(ts));
}
function getPeerLastSeen(deviceId) {
  const v = localStorage.getItem('linkhub_peer_seen_' + deviceId);
  return v ? parseInt(v, 10) : 0;
}

// Returns { address, online, ageMs } for a trusted device.
function peerPresence(deviceId) {
  const address = getPeerAddress(deviceId);
  const seen = getPeerLastSeen(deviceId);
  const ageMs = seen ? Date.now() - seen : Infinity;
  return { address, online: !!address && ageMs <= PEER_OFFLINE_AFTER_MS, ageMs };
}

function presenceLabel(deviceId) {
  const p = peerPresence(deviceId);
  if (!p.address) return '离线 · 地址未知';
  if (!p.online) return `离线 · 上次 ${p.address}`;
  const secs = Math.max(0, Math.round(p.ageMs / 1000));
  const ago = secs <= 1 ? '刚刚' : `${secs} 秒前`;
  return `在线 · ${p.address} · ${ago}`;
}

let autoDiscoveryTimer = null;

async function runAutoDiscoveryOnce() {
  const trustStorePath = getSetting('trustStorePath');
  if (!trustStorePath) return;
  let peers;
  try {
    peers = await tauriInvoke('scan_trusted_mdns', { trustStorePath, timeoutSeconds: 3 });
  } catch (_) {
    return; // stay silent; the manual Scan LAN button surfaces errors
  }
  const now = Date.now();
  peers.forEach(peer => {
    setPeerAddress(peer.device_id, peer.address);
    setPeerLastSeen(peer.device_id, now);
  });

  const devicesActive = document.getElementById('tab-devices')?.classList.contains('active');
  const sendActive = document.getElementById('tab-send')?.classList.contains('active');
  if (devicesActive && typeof refreshDevicePresence === 'function') refreshDevicePresence();
  if (sendActive && typeof autoFillSelectedAddresses === 'function') autoFillSelectedAddresses();
}

function startAutoDiscovery() {
  if (autoDiscoveryTimer) return;
  runAutoDiscoveryOnce();
  autoDiscoveryTimer = setInterval(runAutoDiscoveryOnce, DISCOVERY_INTERVAL_MS);
}

// ── Cross-platform default paths ───────────────────────────────────
// Ask the Rust side for OS-appropriate default paths (app data dir) and seed
// them only when a setting is not already stored, so existing setups keep
// their current paths and the client is no longer pinned to C:\LinkHub.

async function seedDefaultPaths() {
  let cfg;
  try {
    cfg = await tauriInvoke('default_config');
  } catch (_) {
    return; // not running under Tauri / command unavailable -> keep static defaults
  }
  const map = {
    identityPath: cfg.identity_path,
    trustStorePath: cfg.trust_store_path,
    receiveDir: cfg.receive_dir,
    historyPath: cfg.history_path,
    listenerAddr: cfg.listener_addr,
  };
  Object.keys(map).forEach(key => {
    if (!map[key]) return;
    DEFAULTS[key] = map[key];
    if (localStorage.getItem('linkhub_' + key) === null) setSetting(key, map[key]);
  });
  // The pairing tab built its settings inputs at load with the static defaults;
  // rebuild it so they reflect the seeded OS-appropriate paths.
  if (typeof buildPairingTab === 'function' &&
      document.getElementById('tab-pairing')?.classList.contains('active')) {
    buildPairingTab();
  }
}

window.addEventListener('DOMContentLoaded', async () => {
  await seedDefaultPaths();
  startAutoDiscovery();
});
