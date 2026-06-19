// ── Send Tab ────────────────────────────────────────────────────────

let cachedPeers = [];
let pendingSendSelection = null;
let webrtcReceiverPoll = null;
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

    <div class="card">
      <h3>跨网络传输 (WebRTC)</h3>
      <p class="hint">两台已配对设备在不同网络间，经信令服务器打洞 / 中继端到端加密传输。需先运行 signaling-server，且双方都在线。</p>
      <div class="form-group">
        <label>信令服务器</label>
        <input type="text" id="wrtc-signaling" placeholder="ws://127.0.0.1:9000" value="ws://127.0.0.1:9000">
      </div>
      <div class="form-group">
        <label>STUN / TURN URL（可选，逗号或换行分隔）</label>
        <input type="text" id="wrtc-ice" placeholder="stun:stun.l.google.com:19302, turn:turn.example.com:3478">
      </div>
      <div class="form-group">
        <label>TURN 凭证与中继（可选）</label>
        <div class="inline-row">
          <input type="text" id="wrtc-turn-user" placeholder="TURN 用户名">
          <input type="text" id="wrtc-turn-cred" placeholder="TURN 凭证">
          <label class="inline-check"><input type="checkbox" id="wrtc-relay-only"> 仅走中继</label>
        </div>
      </div>
      <div class="form-group">
        <label>目标设备</label>
        <select id="wrtc-device-select">
          <option value="">-- 请先加载设备列表 --</option>
        </select>
      </div>
      <div class="form-group">
        <label>文件路径</label>
        <div class="inline-row">
          <input type="text" id="wrtc-file-path" placeholder="C:\\path\\to\\file">
          <button class="btn btn-secondary" data-act="chooseFileForWebrtc">浏览…</button>
        </div>
      </div>
      <div class="inline-row">
        <button class="btn btn-primary" data-act="webrtcSendFile">跨网络发送文件</button>
        <button class="btn btn-secondary" id="wrtc-start-receiver" data-act="webrtcStartReceiver">开始接收</button>
        <button class="btn btn-secondary" id="wrtc-stop-receiver" data-act="webrtcStopReceiver">停止接收</button>
        <button class="btn btn-secondary" data-act="previewConnectionPlan">查看连接路径</button>
      </div>
      <div id="wrtc-receiver-status" class="hint">接收未启动</div>
      <div id="wrtc-plan" class="hint"></div>
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
    const wrtcSelect = document.getElementById('wrtc-device-select');
    if (wrtcSelect) wrtcSelect.innerHTML = opts;
    applyPendingSendSelection();
    refreshWebrtcReceiverStatus();
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

// ── Cross-network (WebRTC) ─────────────────────────────────────────

async function chooseFileForWebrtc() {
  try {
    const path = await tauriInvoke('choose_file_path');
    if (path) {
      document.getElementById('wrtc-file-path').value = path;
      setStatus('已选择文件', 'ok');
    }
  } catch (err) {
    showMessage('send-msg', '文件选择出错：' + err.message, 'error');
  }
}

// Parse the shared ICE/TURN inputs into the args the Rust commands expect.
function webrtcIceArgs() {
  const ice = document.getElementById('wrtc-ice').value.trim();
  const iceUrls = ice ? ice.split(/[\n,]+/).map(s => s.trim()).filter(Boolean) : [];
  return {
    iceUrls,
    turnUsername: document.getElementById('wrtc-turn-user').value.trim() || null,
    turnCredential: document.getElementById('wrtc-turn-cred').value.trim() || null,
    relayOnly: document.getElementById('wrtc-relay-only').checked,
  };
}

async function webrtcSendFile() {
  const identityPath = getSetting('identityPath');
  const trustStorePath = getSetting('trustStorePath');
  const historyPath = getSetting('historyPath');
  const signalingUrl = document.getElementById('wrtc-signaling').value.trim();
  const peerDeviceId = document.getElementById('wrtc-device-select').value;
  const filePath = document.getElementById('wrtc-file-path').value.trim();
  if (!signalingUrl || !peerDeviceId || !filePath) {
    showMessage('send-msg', '请填写信令服务器、目标设备和文件路径', 'error');
    return;
  }
  const ice = webrtcIceArgs();
  setStatus('正在建立跨网络连接并发送…', 'info');
  try {
    const result = await tauriInvoke('webrtc_send_file', {
      signalingUrl, identityPath, peerDeviceId, trustStorePath, historyPath, filePath,
      iceUrls: ice.iceUrls, turnUsername: ice.turnUsername,
      turnCredential: ice.turnCredential, relayOnly: ice.relayOnly,
    });
    showMessage('send-msg', result.detail, 'success');
    setStatus('跨网络发送完成', 'ok');
  } catch (err) {
    showMessage('send-msg', '跨网络发送错误：' + err.message, 'error');
    setStatus('跨网络发送失败', 'error');
  }
}

async function webrtcReceiveOnce() {
  return webrtcStartReceiver();
}

function webrtcReceiverArgs() {
  const identityPath = getSetting('identityPath');
  const trustStorePath = getSetting('trustStorePath');
  const receiveDir = getSetting('receiveDir');
  const signalingUrl = document.getElementById('wrtc-signaling').value.trim();
  const ice = webrtcIceArgs();
  return {
    signalingUrl,
    identityPath,
    trustStorePath,
    receiveDir,
    iceUrls: ice.iceUrls,
    turnUsername: ice.turnUsername,
    turnCredential: ice.turnCredential,
    relayOnly: ice.relayOnly,
  };
}

function renderWebrtcReceiverStatus(status) {
  const el = document.getElementById('wrtc-receiver-status');
  const startBtn = document.getElementById('wrtc-start-receiver');
  const stopBtn = document.getElementById('wrtc-stop-receiver');
  if (!el) return;
  const running = Boolean(status && status.running);
  const stopping = Boolean(status && status.stopping);
  const completed = status && Number.isFinite(status.completed_sessions) ? status.completed_sessions : 0;
  const error = status && status.error ? `，最近错误：${status.error}` : '';
  el.textContent = stopping
    ? `接收正在停止，已完成 ${completed} 次${error}`
    : running
      ? `接收运行中，已完成 ${completed} 次${error}`
      : `接收未启动，已完成 ${completed} 次${error}`;
  if (startBtn) startBtn.disabled = running;
  if (stopBtn) stopBtn.disabled = !running;
}

async function refreshWebrtcReceiverStatus() {
  try {
    const status = await tauriInvoke('webrtc_receiver_status');
    renderWebrtcReceiverStatus(status);
  } catch (_) {
    renderWebrtcReceiverStatus({ running: false, stopping: false, completed_sessions: 0, error: '' });
  }
}

function ensureWebrtcReceiverPoll() {
  if (webrtcReceiverPoll) return;
  webrtcReceiverPoll = setInterval(refreshWebrtcReceiverStatus, 3000);
}

async function webrtcStartReceiver() {
  const args = webrtcReceiverArgs();
  if (!args.signalingUrl) {
    showMessage('send-msg', '请填写信令服务器', 'error');
    return;
  }
  setStatus('正在启动跨网络接收…', 'info');
  try {
    const status = await tauriInvoke('webrtc_start_receiver', args);
    renderWebrtcReceiverStatus(status);
    ensureWebrtcReceiverPoll();
    showMessage('send-msg', '跨网络接收已启动', 'success');
    setStatus('跨网络接收运行中', 'ok');
  } catch (err) {
    showMessage('send-msg', '启动跨网络接收错误：' + err.message, 'error');
    setStatus('跨网络接收启动失败', 'error');
  }
}

async function webrtcStopReceiver() {
  setStatus('正在停止跨网络接收…', 'info');
  try {
    const status = await tauriInvoke('webrtc_stop_receiver');
    renderWebrtcReceiverStatus(status);
    showMessage('send-msg', status.stopping ? '跨网络接收正在停止' : '跨网络接收已停止', 'success');
    setStatus(status.stopping ? '跨网络接收正在停止' : '跨网络接收已停止', 'ok');
  } catch (err) {
    showMessage('send-msg', '停止跨网络接收错误：' + err.message, 'error');
    setStatus('跨网络接收停止失败', 'error');
  }
}

// Show which transport path a transfer would take (LAN 直连 → 打洞 → 中继).
async function previewConnectionPlan() {
  const peerDeviceId = document.getElementById('wrtc-device-select').value;
  const lanAddr = peerDeviceId ? (getPeerAddress(peerDeviceId) || '') : '';
  const signalingAvailable = document.getElementById('wrtc-signaling').value.trim() !== '';
  const ice = webrtcIceArgs();
  const relayAvailable = ice.iceUrls.some(u => u.startsWith('turn:') || u.startsWith('turns:'));
  try {
    const paths = await tauriInvoke('connection_plan', { lanAddr, signalingAvailable, relayAvailable });
    const el = document.getElementById('wrtc-plan');
    if (!paths.length) {
      el.textContent = '当前无可达路径（请填写信令服务器，或先在局域网扫描到对端地址）。';
      return;
    }
    el.innerHTML = '连接尝试顺序：' + paths.map((p, i) =>
      `${i + 1}. ${escHtml(p.label)}${p.detail ? ' (' + escHtml(p.detail) + ')' : ''}`
    ).join('  →  ');
  } catch (err) {
    showMessage('send-msg', '获取连接路径出错：' + err.message, 'error');
  }
}

// Init on load
buildSendTab();
refreshWebrtcReceiverStatus();
