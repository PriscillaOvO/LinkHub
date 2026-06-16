// ── Send Tab ────────────────────────────────────────────────────────

let cachedPeers = [];
// Tracks the last address auto-filled per kind, so background discovery can
// refresh it without clobbering a value the user typed manually.
let lastAutoAddr = { text: '', file: '' };

function buildSendTab() {
  const section = document.getElementById('tab-send');
  section.innerHTML = `
    <div class="card">
      <h3>Send Text</h3>
      <div class="form-group">
        <label>Target device</label>
        <select id="send-device-select" onchange="onSendPeerChanged('text')">
          <option value="">-- Load device list first --</option>
        </select>
      </div>
      <div class="form-group">
        <label>Target address (IP:port)</label>
        <div class="inline-row">
          <input type="text" id="send-addr" placeholder="127.0.0.1:8787">
          <button class="btn btn-secondary" onclick="scanAddressForSend('text')">Scan LAN</button>
        </div>
      </div>
      <div class="form-group">
        <label>Message</label>
        <textarea id="send-text" placeholder="Type your message..."></textarea>
      </div>
      <button class="btn btn-primary" onclick="sendText()">Send Text</button>
    </div>

    <div class="card">
      <h3>Send File</h3>
      <div class="form-group">
        <label>Target device</label>
        <select id="send-file-device-select" onchange="onSendPeerChanged('file')">
          <option value="">-- Load device list first --</option>
        </select>
      </div>
      <div class="form-group">
        <label>Target address (IP:port)</label>
        <div class="inline-row">
          <input type="text" id="send-file-addr" placeholder="127.0.0.1:8787">
          <button class="btn btn-secondary" onclick="scanAddressForSend('file')">Scan LAN</button>
        </div>
      </div>
      <div class="form-group">
        <label>File path</label>
        <div class="inline-row">
          <input type="text" id="send-file-path" placeholder="C:\\path\\to\\file.txt">
          <button class="btn btn-secondary" onclick="chooseFileForSend()">Browse...</button>
        </div>
      </div>
      <button class="btn btn-primary" onclick="sendFile()">Send File</button>
    </div>
    <div id="send-msg"></div>
  `;
}

async function renderSendTab() {
  if (!document.getElementById('send-device-select')) {
    buildSendTab();
  }

  // Load device list
  const identityPath = getSetting('identityPath');
  const trustStorePath = getSetting('trustStorePath');
  if (!identityPath) return;

  try {
    const result = await tauriInvoke('get_local_status', {
      identityPath, trustStorePath
    });
    cachedPeers = result.trusted_devices;

    const opts = '<option value="">-- Select device --</option>' +
      cachedPeers.map(d =>
        `<option value="${escHtml(d.device_id)}">${escHtml(d.device_name)} (${escHtml(d.device_id)})</option>`
      ).join('');

    document.getElementById('send-device-select').innerHTML = opts;
    document.getElementById('send-file-device-select').innerHTML = opts;
  } catch (err) {
    showMessage('send-msg', 'Failed to load devices: ' + err.message, 'error');
  }
}

function onSendPeerChanged(kind) {
  const selectId = kind === 'file' ? 'send-file-device-select' : 'send-device-select';
  const addrId = kind === 'file' ? 'send-file-addr' : 'send-addr';
  const peerDeviceId = document.getElementById(selectId).value;
  const savedAddr = getPeerAddress(peerDeviceId);
  if (savedAddr) {
    document.getElementById(addrId).value = savedAddr;
    lastAutoAddr[kind] = savedAddr;
    setStatus('Loaded saved address for selected device', 'ok');
  }
}

// Called by the background auto-discovery loop: refresh the selected device's
// address input when a newer LAN address is found, unless the user edited it.
function autoFillSelectedAddresses() {
  ['text', 'file'].forEach(kind => {
    const selectId = kind === 'file' ? 'send-file-device-select' : 'send-device-select';
    const addrId = kind === 'file' ? 'send-file-addr' : 'send-addr';
    const selectEl = document.getElementById(selectId);
    const addrEl = document.getElementById(addrId);
    if (!selectEl || !addrEl) return;
    const peerId = selectEl.value;
    if (!peerId) return;
    const latest = getPeerAddress(peerId);
    if (!latest) return;
    const current = addrEl.value.trim();
    if (current === '' || current === lastAutoAddr[kind]) {
      addrEl.value = latest;
      lastAutoAddr[kind] = latest;
    }
  });
}

async function chooseFileForSend() {
  try {
    const path = await tauriInvoke('choose_file_path');
    if (path) {
      document.getElementById('send-file-path').value = path;
      setStatus('Selected file', 'ok');
    }
  } catch (err) {
    showMessage('send-msg', 'File picker error: ' + err.message, 'error');
  }
}

async function scanAddressForSend(kind) {
  const selectId = kind === 'file' ? 'send-file-device-select' : 'send-device-select';
  const addrId = kind === 'file' ? 'send-file-addr' : 'send-addr';
  const peerDeviceId = document.getElementById(selectId).value;
  const trustStorePath = getSetting('trustStorePath');
  if (!peerDeviceId) {
    showMessage('send-msg', 'Select a target device first', 'error');
    return;
  }

  setStatus('Scanning LAN for selected device...', 'info');
  try {
    const peers = await tauriInvoke('scan_trusted_mdns', {
      trustStorePath,
      timeoutSeconds: 4
    });
    peers.forEach(peer => setPeerAddress(peer.device_id, peer.address));
    const found = peers.find(peer => peer.device_id === peerDeviceId);
    if (!found) {
      showMessage('send-msg', 'Selected device was not found on LAN.', 'info');
      return;
    }
    document.getElementById(addrId).value = found.address;
    setStatus('Loaded discovered address', 'ok');
    showMessage('send-msg', `Discovered ${found.device_name}: ${found.address}`, 'success');
  } catch (err) {
    showMessage('send-msg', 'LAN scan error: ' + err.message, 'error');
    setStatus('LAN scan failed', 'error');
  }
}

async function sendText() {
  const identityPath = getSetting('identityPath');
  const trustStorePath = getSetting('trustStorePath');
  const historyPath = getSetting('historyPath');
  const peerDeviceId = document.getElementById('send-device-select').value;
  const addr = document.getElementById('send-addr').value.trim();
  const text = document.getElementById('send-text').value.trim();

  if (!peerDeviceId || !addr || !text) {
    showMessage('send-msg', 'Fill in all fields', 'error');
    return;
  }

  try {
    const result = await tauriInvoke('send_encrypted_text', {
      peerAddr: addr, identityPath, peerDeviceId, trustStorePath, historyPath, text
    });
    setPeerAddress(peerDeviceId, addr);
    showMessage('send-msg', result.detail, 'success');
    setStatus('Text sent', 'ok');
  } catch (err) {
    showMessage('send-msg', 'Error: ' + err.message, 'error');
  }
}

async function sendFile() {
  const identityPath = getSetting('identityPath');
  const trustStorePath = getSetting('trustStorePath');
  const historyPath = getSetting('historyPath');
  const peerDeviceId = document.getElementById('send-file-device-select').value;
  const addr = document.getElementById('send-file-addr').value.trim();
  const filePath = document.getElementById('send-file-path').value.trim();

  if (!peerDeviceId || !addr || !filePath) {
    showMessage('send-msg', 'Fill in all fields', 'error');
    return;
  }

  try {
    const result = await tauriInvoke('send_encrypted_file', {
      peerAddr: addr, identityPath, peerDeviceId, trustStorePath, historyPath, filePath
    });
    setPeerAddress(peerDeviceId, addr);
    showMessage('send-msg', result.detail, 'success');
    setStatus('File sent', 'ok');
  } catch (err) {
    showMessage('send-msg', 'Error: ' + err.message, 'error');
  }
}

// Init on load
buildSendTab();
