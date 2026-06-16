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
