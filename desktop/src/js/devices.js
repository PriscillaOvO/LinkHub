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
    <div id="devices-msg"></div>
  `;
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
      const items = result.trusted_devices.map(d => `
        <li class="device-item">
          <div>
            <div class="device-name">${escHtml(d.device_name)}</div>
            <div class="device-meta">${escHtml(d.device_id)} &middot; ${escHtml(d.fingerprint)}</div>
            <div class="device-meta">Address: ${escHtml(getPeerAddress(d.device_id) || 'not saved')}</div>
          </div>
          <span>&#9786;</span>
        </li>
      `).join('');
      document.getElementById('trusted-list').innerHTML =
        `<ul class="device-list">${items}</ul>`;
    }
    setStatus(`${result.trusted_devices.length} trusted device(s)`, 'ok');
  } catch (err) {
    showMessage('devices-msg', 'Error: ' + err.message, 'error');
  }
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

    peers.forEach(peer => setPeerAddress(peer.device_id, peer.address));

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

function escHtml(s) {
  const div = document.createElement('div');
  div.textContent = s;
  return div.innerHTML;
}

// Init on load
buildDevicesTab();
