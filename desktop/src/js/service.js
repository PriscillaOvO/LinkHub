// ── Service Tab (Listener) ──────────────────────────────────────────
function buildServiceTab() {
  const section = document.getElementById('tab-service');
  section.innerHTML = `
    <div class="card">
      <h3>Listener Service</h3>
      <p>Keep LinkHub listening for incoming connections in the background.</p>
      <div class="form-group">
        <label>Listen Address</label>
        <input type="text" id="svc-addr" placeholder="127.0.0.1:8787">
      </div>
      <div class="form-group">
        <label>Receive Directory</label>
        <div class="inline-row">
          <input type="text" id="svc-receive-dir" placeholder="C:\\LinkHub\\inbox">
          <button class="btn btn-secondary" onclick="chooseReceiveDir()">Browse...</button>
        </div>
      </div>
      <div style="display:flex;gap:12px;align-items:center;margin:12px 0">
        <button class="btn btn-primary" id="btn-start-listener" onclick="startListener()">Start Listening</button>
        <button class="btn btn-danger" id="btn-stop-listener" onclick="stopListener()" disabled>Stop</button>
        <span id="svc-running-indicator" style="font-weight:600;color:var(--danger)">&#9679; Stopped</span>
      </div>
      <p class="device-meta" id="svc-mdns-status">mDNS advertise: stopped</p>
    </div>
    <div class="card" id="listener-info-card" style="display:none">
      <h3>Listener Info</h3>
      <div id="listener-info"></div>
    </div>
    <div class="card">
      <h3>Network Hints</h3>
      <p>Use these addresses from another trusted device on the same network.</p>
      <div id="network-hints"><p class="device-meta">Checking...</p></div>
    </div>
    <div id="service-msg"></div>
  `;

  document.getElementById('svc-addr').value = getSetting('listenerAddr');
  document.getElementById('svc-receive-dir').value = getSetting('receiveDir');

  // Poll listener status
  checkListenerStatus();
  checkMdnsStatus();
  renderNetworkHints();
}

async function checkListenerStatus() {
  try {
    const status = await tauriInvoke('listener_status');
    updateListenerUI(status);
  } catch (_) {}
}

async function checkMdnsStatus() {
  const el = document.getElementById('svc-mdns-status');
  if (!el) return;
  try {
    const status = await tauriInvoke('mdns_advertise_status');
    el.textContent = status.running
      ? `mDNS advertise: ${status.service_name}`
      : 'mDNS advertise: stopped';
  } catch (_) {}
}

async function renderNetworkHints() {
  const container = document.getElementById('network-hints');
  if (!container) return;
  const addr = document.getElementById('svc-addr')?.value.trim() || '0.0.0.0:8787';
  const port = Number(addr.split(':').pop()) || 8787;
  try {
    const hints = await tauriInvoke('local_network_hints', { port });
    if (!hints.length) {
      container.innerHTML = '<p class="device-meta">No local network address detected.</p>';
      return;
    }
    container.innerHTML = hints.map(h => `
      <div class="hint-row">
        <span>${escHtml(h.label)}</span>
        <code>${escHtml(h.address)}</code>
      </div>
    `).join('');
  } catch (err) {
    container.innerHTML = `<p class="device-meta">Unable to read network hints: ${escHtml(err.message)}</p>`;
  }
}

async function chooseReceiveDir() {
  try {
    const path = await tauriInvoke('choose_folder_path');
    if (path) {
      document.getElementById('svc-receive-dir').value = path;
      setSetting('receiveDir', path);
      setStatus('Selected receive directory', 'ok');
    }
  } catch (err) {
    showMessage('service-msg', 'Folder picker error: ' + err.message, 'error');
  }
}

function updateListenerUI(status) {
  const btnStart = document.getElementById('btn-start-listener');
  const btnStop = document.getElementById('btn-stop-listener');
  const indicator = document.getElementById('svc-running-indicator');
  const infoCard = document.getElementById('listener-info-card');
  if (!btnStart) return;

  if (status.running) {
    btnStart.disabled = true;
    btnStop.disabled = false;
    if (indicator) {
      indicator.innerHTML = '&#9679; Running';
      indicator.style.color = 'var(--success)';
    }
    if (infoCard) {
      infoCard.style.display = 'block';
      document.getElementById('listener-info').innerHTML = `
        <p>Listening on: <strong>${escHtml(status.bind_addr)}</strong></p>
        <p>Identity: <code>${escHtml(getSetting('identityPath'))}</code></p>
        <p>Trust Store: <code>${escHtml(getSetting('trustStorePath'))}</code></p>
      `;
    }
  } else {
    btnStart.disabled = false;
    btnStop.disabled = true;
    if (indicator) {
      indicator.innerHTML = '&#9679; Stopped';
      indicator.style.color = 'var(--danger)';
    }
    if (infoCard) {
      if (status.error) {
        infoCard.style.display = 'block';
        document.getElementById('listener-info').innerHTML = `
          <p><strong>Last error:</strong> ${escHtml(status.error)}</p>
          <p>Last address: <code>${escHtml(status.bind_addr || getSetting('listenerAddr'))}</code></p>
        `;
        setStatus('Listener error: ' + status.error, 'error');
      } else {
        infoCard.style.display = 'none';
      }
    }
  }
}

async function startListener() {
  const addr = document.getElementById('svc-addr').value.trim() || '127.0.0.1:8787';
  const receiveDir = document.getElementById('svc-receive-dir').value.trim() || 'C:\\LinkHub\\inbox';
  const identityPath = getSetting('identityPath');
  const trustStorePath = getSetting('trustStorePath');

  if (!identityPath) {
    showMessage('service-msg', 'No identity configured. Go to Pair tab first.', 'error');
    return;
  }

  setSetting('listenerAddr', addr);
  setSetting('receiveDir', receiveDir);

  try {
    const result = await tauriInvoke('start_listener', {
      bindAddr: addr, identityPath, trustStorePath, receiveDir
    });
    const port = Number(addr.split(':').pop()) || 8787;
    try {
      await tauriInvoke('start_mdns_advertise', { identityPath, port });
      await checkMdnsStatus();
    } catch (mdnsErr) {
      showMessage('service-msg', 'Listener started, but mDNS advertise failed: ' + mdnsErr.message, 'error');
    }
    updateListenerUI(result);
    setStatus('Listener running on ' + addr, 'ok');
    showMessage('service-msg', 'Listener started on ' + addr, 'success');
  } catch (err) {
    showMessage('service-msg', 'Error: ' + err.message, 'error');
  }
}

async function stopListener() {
  try {
    const result = await tauriInvoke('stop_listener');
    await tauriInvoke('stop_mdns_advertise');
    await checkMdnsStatus();
    updateListenerUI(result);
    setStatus('Listener stopped', 'ok');
    showMessage('service-msg', 'Listener stopped', 'info');
  } catch (err) {
    showMessage('service-msg', 'Error: ' + err.message, 'error');
  }
}

// Init
buildServiceTab();
setInterval(checkListenerStatus, 5000);  // Poll every 5s
setInterval(checkMdnsStatus, 5000);
