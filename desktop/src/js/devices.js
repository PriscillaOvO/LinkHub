// ── Devices Tab ─────────────────────────────────────────────────────

function buildDevicesTab() {
  const section = document.getElementById('tab-devices');
  section.innerHTML = `
    <div class="card">
      <h3>本机设备</h3>
      <div id="local-status">
        <p class="device-meta">点击「加载状态」以显示设备信息。</p>
      </div>
    </div>
    <div class="card">
      <h3>可信设备</h3>
      <div id="trusted-list">
        <p class="device-meta">加载状态后即可查看可信设备。</p>
      </div>
    </div>
    <div style="display:flex;gap:12px;align-items:center;margin:12px 0">
      <button class="btn btn-primary" data-act="renderDevicesTab">加载状态</button>
      <button class="btn btn-secondary" data-act="scanTrustedMdns">扫描局域网</button>
    </div>
    <p class="device-meta">局域网地址每数秒自动发现刷新，无需手动扫描。</p>
    <div id="devices-msg"></div>
  `;
}

// Called by the background auto-discovery loop to keep presence current.
function refreshDevicePresence() {
  if (document.getElementById('local-status')) renderDevicesTab();
}

async function renderDevicesTab() {
  // Rebuild if empty
  if (!document.getElementById('local-status')) {
    buildDevicesTab();
  }

  const identityPath = getSetting('identityPath');
  const trustStorePath = getSetting('trustStorePath');
  if (!identityPath) {
    document.getElementById('local-status').innerHTML =
      '<p class="device-meta">请先在「配对 → 设置」中填写身份文件路径。</p>';
    return;
  }

  try {
    const result = await tauriInvoke('get_local_status', {
      identityPath, trustStorePath
    });

    document.getElementById('local-status').innerHTML = `
      <p><strong>${result.local_device_name}</strong></p>
      <p>ID：<code>${result.local_device_id}</code></p>
      <p>指纹：<code>${result.local_fingerprint}</code></p>
    `;

    if (result.trusted_devices.length === 0) {
      document.getElementById('trusted-list').innerHTML =
        '<p class="device-meta">暂无可信设备。请到「配对」页添加。</p>';
    } else {
      const items = result.trusted_devices.map(d => deviceListItem(d)).join('');
      document.getElementById('trusted-list').innerHTML =
        `<ul class="device-list">${items}</ul>`;
    }
    setStatus(`共 ${result.trusted_devices.length} 台可信设备`, 'ok');
  } catch (err) {
    showMessage('devices-msg', '错误：' + err.message, 'error');
  }
}

function deviceListItem(device) {
  const presence = peerPresence(device.device_id);
  const address = presence.address || '地址未知';
  const statusDot = presence.online ? '&#128994;' : '&#9898;';
  const statusTitle = presence.online ? '在线' : '离线';
  return `
        <li class="device-item">
          <div class="device-main">
            <div class="device-title-row">
              <span class="device-name">${escHtml(device.device_name)}</span>
              <span class="device-state" title="${statusTitle}">${statusDot}</span>
            </div>
            <div class="device-meta">ID：<code>${escHtml(device.device_id)}</code></div>
            <div class="device-meta">指纹：<code>${escHtml(device.fingerprint)}</code></div>
            <div class="device-meta">地址：<code>${escHtml(address)}</code></div>
            <div class="device-meta">${escHtml(presenceLabel(device.device_id))}</div>
            <div class="device-actions">
              <button class="btn btn-secondary btn-small" data-act="copyDeviceField" data-a0="${escHtml(device.device_id)}" data-a1="设备 ID">复制 ID</button>
              <button class="btn btn-secondary btn-small" data-act="copyDeviceAddress" data-a0="${escHtml(device.device_id)}">复制地址</button>
              <button class="btn btn-secondary btn-small" data-act="scanSingleDevice" data-a0="${escHtml(device.device_id)}">刷新</button>
              <button class="btn btn-primary btn-small" data-act="selectPeerForSend" data-a0="${escHtml(device.device_id)}" data-a1="text">发送文本</button>
              <button class="btn btn-secondary btn-small" data-act="selectPeerForSend" data-a0="${escHtml(device.device_id)}" data-a1="file">发送文件</button>
            </div>
          </div>
        </li>
      `;
}

async function scanTrustedMdns() {
  const trustStorePath = getSetting('trustStorePath');
  if (!trustStorePath) {
    showMessage('devices-msg', '尚未配置信任库路径。', 'error');
    return;
  }

  setStatus('正在局域网扫描可信设备…', 'info');
  try {
    const peers = await tauriInvoke('scan_trusted_mdns', {
      trustStorePath,
      timeoutSeconds: 4
    });

    const now = Date.now();
    peers.forEach(peer => {
      setPeerAddress(peer.device_id, peer.address);
      setPeerLastSeen(peer.device_id, now);
    });

    if (peers.length === 0) {
      showMessage('devices-msg', '局域网未发现可信的 LinkHub 设备。', 'info');
    } else {
      showMessage('devices-msg', `发现 ${peers.length} 个可信设备地址。`, 'success');
    }
    await renderDevicesTab();
  } catch (err) {
    showMessage('devices-msg', '局域网扫描出错：' + err.message, 'error');
    setStatus('局域网扫描失败', 'error');
  }
}

async function scanSingleDevice(deviceId) {
  const trustStorePath = getSetting('trustStorePath');
  if (!trustStorePath) {
    showMessage('devices-msg', '尚未配置信任库路径。', 'error');
    return;
  }

  setStatus('正在局域网扫描所选设备…', 'info');
  try {
    const peers = await tauriInvoke('scan_trusted_mdns', {
      trustStorePath,
      timeoutSeconds: 4
    });
    const now = Date.now();
    peers.forEach(peer => {
      setPeerAddress(peer.device_id, peer.address);
      setPeerLastSeen(peer.device_id, now);
    });
    const found = peers.find(peer => peer.device_id === deviceId);
    if (!found) {
      showMessage('devices-msg', '局域网未发现所选设备。', 'info');
      await renderDevicesTab();
      return;
    }
    showMessage('devices-msg', `已更新 ${found.device_name}：${found.address}`, 'success');
    setStatus('设备地址已更新', 'ok');
    await renderDevicesTab();
  } catch (err) {
    showMessage('devices-msg', '局域网扫描出错：' + err.message, 'error');
    setStatus('局域网扫描失败', 'error');
  }
}

async function copyDeviceField(text, label) {
  await copyText(text);
  showMessage('devices-msg', `${label} 已复制`, 'success');
}

async function copyDeviceAddress(deviceId) {
  const address = getPeerAddress(deviceId);
  if (!address) {
    showMessage('devices-msg', '该设备暂无缓存地址。', 'info');
    return;
  }
  await copyText(address);
  showMessage('devices-msg', '地址已复制', 'success');
}

async function copyText(text) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const input = document.createElement('textarea');
  input.value = text;
  input.style.position = 'fixed';
  input.style.opacity = '0';
  document.body.appendChild(input);
  input.select();
  document.execCommand('copy');
  input.remove();
}

function escHtml(s) {
  const div = document.createElement('div');
  div.textContent = s;
  return div.innerHTML;
}

function escJs(s) {
  return String(s).replace(/\\/g, '\\\\').replace(/'/g, "\\'");
}

// Init on load
buildDevicesTab();
