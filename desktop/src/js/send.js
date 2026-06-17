// ── Send Tab ────────────────────────────────────────────────────────

let cachedPeers = [];
let pendingSendSelection = null;
// Tracks the last address auto-filled per kind, so background discovery can
// refresh it without clobbering a value the user typed manually.
let lastAutoAddr = { text: '', file: '' };

function buildSendTab() {
  const section = document.getElementById('tab-send');
  section.innerHTML = `
    <div class="card">
      <h3>发送文本</h3>
      <div class="form-group">
        <label>目标设备</label>
        <select id="send-device-select" data-change="onSendPeerChanged" data-a0="text">
          <option value="">-- 请先加载设备列表 --</option>
        </select>
      </div>
      <div class="form-group">
        <label>目标地址（IP:端口）</label>
        <div class="inline-row">
          <input type="text" id="send-addr" placeholder="127.0.0.1:8787">
          <button class="btn btn-secondary" data-act="scanAddressForSend" data-a0="text">扫描局域网</button>
        </div>
      </div>
      <div class="form-group">
        <label>消息内容</label>
        <textarea id="send-text" placeholder="输入要发送的消息…"></textarea>
      </div>
      <button class="btn btn-primary" data-act="sendText">发送文本</button>
    </div>

    <div class="card">
      <h3>发送文件</h3>
      <div class="form-group">
        <label>目标设备</label>
        <select id="send-file-device-select" data-change="onSendPeerChanged" data-a0="file">
          <option value="">-- 请先加载设备列表 --</option>
        </select>
      </div>
      <div class="form-group">
        <label>目标地址（IP:端口）</label>
        <div class="inline-row">
          <input type="text" id="send-file-addr" placeholder="127.0.0.1:8787">
          <button class="btn btn-secondary" data-act="scanAddressForSend" data-a0="file">扫描局域网</button>
        </div>
      </div>
      <div class="form-group">
        <label>文件路径</label>
        <div class="inline-row">
          <input type="text" id="send-file-path" placeholder="C:\\path\\to\\file.txt">
          <button class="btn btn-secondary" data-act="chooseFileForSend">浏览…</button>
        </div>
      </div>
      <button class="btn btn-primary" data-act="sendFile">发送文件</button>
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

    const opts = '<option value="">-- 选择设备 --</option>' +
      cachedPeers.map(d =>
        `<option value="${escHtml(d.device_id)}">${escHtml(d.device_name)} (${escHtml(d.device_id)})</option>`
      ).join('');

    document.getElementById('send-device-select').innerHTML = opts;
    document.getElementById('send-file-device-select').innerHTML = opts;
    applyPendingSendSelection();
  } catch (err) {
    showMessage('send-msg', '加载设备失败：' + err.message, 'error');
  }
}

function selectPeerForSend(peerDeviceId, kind = 'text') {
  pendingSendSelection = { peerDeviceId, kind };
  document.querySelectorAll('.tab-bar .tab').forEach(btn => {
    const active = btn.dataset.tab === 'send';
    btn.classList.toggle('active', active);
  });
  document.querySelectorAll('.tab-content').forEach(section => {
    section.classList.toggle('active', section.id === 'tab-send');
  });
  renderSendTab();
}

function applyPendingSendSelection() {
  if (!pendingSendSelection) return;
  const { peerDeviceId, kind } = pendingSendSelection;
  const selectId = kind === 'file' ? 'send-file-device-select' : 'send-device-select';
  const selectEl = document.getElementById(selectId);
  if (!selectEl) return;
  selectEl.value = peerDeviceId;
  onSendPeerChanged(kind);
  pendingSendSelection = null;
  setStatus('已选择发送目标设备', 'ok');
}

function onSendPeerChanged(kind) {
  const selectId = kind === 'file' ? 'send-file-device-select' : 'send-device-select';
  const addrId = kind === 'file' ? 'send-file-addr' : 'send-addr';
  const peerDeviceId = document.getElementById(selectId).value;
  const savedAddr = getPeerAddress(peerDeviceId);
  if (savedAddr) {
    document.getElementById(addrId).value = savedAddr;
    lastAutoAddr[kind] = savedAddr;
    setStatus('已载入所选设备的已保存地址', 'ok');
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
      setStatus('已选择文件', 'ok');
    }
  } catch (err) {
    showMessage('send-msg', '文件选择出错：' + err.message, 'error');
  }
}

async function scanAddressForSend(kind) {
  const selectId = kind === 'file' ? 'send-file-device-select' : 'send-device-select';
  const addrId = kind === 'file' ? 'send-file-addr' : 'send-addr';
  const peerDeviceId = document.getElementById(selectId).value;
  const trustStorePath = getSetting('trustStorePath');
  if (!peerDeviceId) {
    showMessage('send-msg', '请先选择目标设备', 'error');
    return;
  }

  setStatus('正在局域网扫描所选设备…', 'info');
  try {
    const peers = await tauriInvoke('scan_trusted_mdns', {
      trustStorePath,
      timeoutSeconds: 4
    });
    peers.forEach(peer => setPeerAddress(peer.device_id, peer.address));
    const found = peers.find(peer => peer.device_id === peerDeviceId);
    if (!found) {
      showMessage('send-msg', '局域网未发现所选设备。', 'info');
      return;
    }
    document.getElementById(addrId).value = found.address;
    setStatus('已载入发现的地址', 'ok');
    showMessage('send-msg', `已发现 ${found.device_name}：${found.address}`, 'success');
  } catch (err) {
    showMessage('send-msg', '局域网扫描出错：' + err.message, 'error');
    setStatus('局域网扫描失败', 'error');
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
    showMessage('send-msg', '请填写所有字段', 'error');
    return;
  }

  try {
    const result = await tauriInvoke('send_encrypted_text', {
      peerAddr: addr, identityPath, peerDeviceId, trustStorePath, historyPath, text
    });
    setPeerAddress(peerDeviceId, addr);
    showMessage('send-msg', result.detail, 'success');
    setStatus('文本已发送', 'ok');
  } catch (err) {
    showMessage('send-msg', '错误：' + err.message, 'error');
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
    showMessage('send-msg', '请填写所有字段', 'error');
    return;
  }

  try {
    const result = await tauriInvoke('send_encrypted_file', {
      peerAddr: addr, identityPath, peerDeviceId, trustStorePath, historyPath, filePath
    });
    setPeerAddress(peerDeviceId, addr);
    showMessage('send-msg', result.detail, 'success');
    setStatus('文件已发送', 'ok');
  } catch (err) {
    showMessage('send-msg', '错误：' + err.message, 'error');
  }
}

// Init on load
buildSendTab();
