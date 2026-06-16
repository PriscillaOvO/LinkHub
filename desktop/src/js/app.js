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

window.addEventListener('DOMContentLoaded', startAutoDiscovery);
