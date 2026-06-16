// ── Devices Tab ─────────────────────────────────────────────────────

function buildDevicesTab() {
  const section = document.getElementById('tab-devices');
  section.innerHTML = `
    <div class="card">
      <h3>This Device</h3>
      <div id="local-status">
        <p class="device-meta">Click "Load Status" to show device info.</p>
      </div>
    </div>
    <div class="card">
      <h3>Trusted Devices</h3>
      <div id="trusted-list">
        <p class="device-meta">Load your status to see trusted devices.</p>
      </div>
    </div>
    <div style="display:flex;gap:12px;align-items:center;margin:12px 0">
      <button class="btn btn-primary" onclick="renderDevicesTab()">Load Status</button>
      <button class="btn btn-secondary" onclick="scanTrustedMdns()">Scan LAN</button>
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
      '<p class="device-meta">Set identity path in Pair → Settings first.</p>';
    return;
  }

  try {
    const result = await tauriInvoke('get_local_status', {
      identityPath, trustStorePath
    });

    document.getElementById('local-status').innerHTML = `
      <p><strong>${result.local_device_name}</strong></p>
      <p>ID: <code>${result.local_device_id}</code></p>
      <p>Fingerprint: <code>${result.local_fingerprint}</code></p>
    `;

    if (result.trusted_devices.length === 0) {
      document.getElementById('trusted-list').innerHTML =
        '<p class="device-meta">No trusted devices yet. Go to Pair tab to add one.</p>';
    } else {
      const items = result.trusted_devices.map(d => deviceListItem(d)).join('');
      document.getElementById('trusted-list').innerHTML =
        `<ul class="device-list">${items}</ul>`;
    }
    setStatus(`${result.trusted_devices.length} trusted device(s)`, 'ok');
  } catch (err) {
    showMessage('devices-msg', 'Error: ' + err.message, 'error');
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
            <div class="device-meta">ID: <code>${escHtml(device.device_id)}</code></div>
            <div class="device-meta">Fingerprint: <code>${escHtml(device.fingerprint)}</code></div>
            <div class="device-meta">Address: <code>${escHtml(address)}</code></div>
            <div class="device-meta">${escHtml(presenceLabel(device.device_id))}</div>
            <div class="device-actions">
              <button class="btn btn-secondary btn-small" onclick="copyDeviceField('${escJs(device.device_id)}', 'Device ID')">Copy ID</button>
              <button class="btn btn-secondary btn-small" onclick="copyDeviceAddress('${escJs(device.device_id)}')">Copy Address</button>
              <button class="btn btn-secondary btn-small" onclick="scanSingleDevice('${escJs(device.device_id)}')">Refresh</button>
              <button class="btn btn-primary btn-small" onclick="selectPeerForSend('${escJs(device.device_id)}', 'text')">Send Text</button>
              <button class="btn btn-secondary btn-small" onclick="selectPeerForSend('${escJs(device.device_id)}', 'file')">Send File</button>
            </div>
          </div>
        </li>
      `;
}

async function scanTrustedMdns() {
  const trustStorePath = getSetting('trustStorePath');
  if (!trustStorePath) {
    showMessage('devices-msg', 'Trust store path is not configured.', 'error');
    return;
  }

  setStatus('Scanning LAN for trusted devices...', 'info');
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
      showMessage('devices-msg', 'No trusted LinkHub devices found on LAN.', 'info');
    } else {
      showMessage('devices-msg', `Found ${peers.length} trusted device address(es).`, 'success');
    }
    await renderDevicesTab();
  } catch (err) {
    showMessage('devices-msg', 'LAN scan error: ' + err.message, 'error');
    setStatus('LAN scan failed', 'error');
  }
}

async function scanSingleDevice(deviceId) {
  const trustStorePath = getSetting('trustStorePath');
  if (!trustStorePath) {
    showMessage('devices-msg', 'Trust store path is not configured.', 'error');
    return;
  }

  setStatus('Scanning LAN for selected device...', 'info');
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
      showMessage('devices-msg', 'Selected device was not found on LAN.', 'info');
      await renderDevicesTab();
      return;
    }
    showMessage('devices-msg', `Updated ${found.device_name}: ${found.address}`, 'success');
    setStatus('Device address updated', 'ok');
    await renderDevicesTab();
  } catch (err) {
    showMessage('devices-msg', 'LAN scan error: ' + err.message, 'error');
    setStatus('LAN scan failed', 'error');
  }
}

async function copyDeviceField(text, label) {
  await copyText(text);
  showMessage('devices-msg', `${label} copied`, 'success');
}

async function copyDeviceAddress(deviceId) {
  const address = getPeerAddress(deviceId);
  if (!address) {
    showMessage('devices-msg', 'No cached address for this device.', 'info');
    return;
  }
  await copyText(address);
  showMessage('devices-msg', 'Address copied', 'success');
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
