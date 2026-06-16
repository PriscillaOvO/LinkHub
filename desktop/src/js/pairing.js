// ── Pairing Tab ─────────────────────────────────────────────────────

function buildPairingTab() {
  const section = document.getElementById('tab-pairing');
  section.innerHTML = `
    <!-- Settings -->
    <div class="settings-toggle" onclick="toggleSettings()">&#9881; Settings</div>
    <div id="pairing-settings" class="settings-panel" style="display:none">
      <div class="form-group">
        <label>Identity path</label>
        <input type="text" id="cfg-identity" placeholder="C:\LinkHub\local-identity.secure.txt">
      </div>
      <div class="form-group">
        <label>Trust Store path</label>
        <input type="text" id="cfg-trust" placeholder="C:\LinkHub\trust-store.txt">
      </div>
      <button class="btn btn-primary" onclick="savePairingSettings()">Save</button>
    </div>

    <div class="pairing-grid">
      <!-- Left: Show my QR -->
      <div class="card">
        <h3>My Device QR</h3>
        <div id="identity-init-area">
          <p>First time? Create your device identity:</p>
          <div class="form-group">
            <label>Device Name</label>
            <input type="text" id="init-device-name" placeholder="My Windows PC">
          </div>
          <button class="btn btn-primary" onclick="initMyIdentity()">Initialize Identity</button>
          <div id="identity-status"></div>
        </div>
        <div id="qr-area" style="display:none">
          <p>Generate a QR code that another trusted device can scan.</p>
          <div class="form-group">
            <label>TTL (seconds)</label>
            <input type="text" id="qr-ttl" value="120">
          </div>
          <button class="btn btn-primary" onclick="generateMyQr()">Generate QR</button>
        </div>
        <div id="qr-display" class="qr-container" style="display:none"></div>
        <div id="qr-info"></div>
        <div id="qr-payload" style="display:none">
          <div class="form-group">
            <label>Pairing payload (copy for manual exchange)</label>
            <textarea id="qr-payload-text" readonly rows="2"></textarea>
          </div>
        </div>
        <div id="qr-msg"></div>
      </div>

      <!-- Right: Scan peer -->
      <div class="card">
        <h3>Scan Peer</h3>
        <p>Paste the pairing payload from another device to begin pairing.</p>
        <div class="form-group">
          <label>Peer's pairing payload</label>
          <textarea id="scan-payload" placeholder="linkhub-pair-v1|..."></textarea>
        </div>
        <button class="btn btn-primary" onclick="inspectPeerPayload()">Inspect</button>
        <div id="peer-info" style="display:none">
          <div id="peer-details"></div>
          <div id="peer-confirmation-code" class="confirmation-code" style="display:none"></div>
          <div class="form-group" id="confirm-group" style="display:none">
            <label>Type the confirmation code shown above</label>
            <input type="text" id="confirm-input" placeholder="ABC-DEF">
          </div>
          <button class="btn btn-primary" id="btn-confirm" style="display:none"
                  onclick="confirmPairing()">Confirm Pairing</button>
        </div>
        <div id="peer-msg"></div>
      </div>
    </div>
  `;

  // Load saved settings
  document.getElementById('cfg-identity').value = getSetting('identityPath');
  document.getElementById('cfg-trust').value = getSetting('trustStorePath');

  // Auto-check if identity exists — show QR area if so
  checkExistingIdentity();
}

async function checkExistingIdentity() {
  const ip = getSetting('identityPath');
  if (!ip) return;
  try {
    const result = await tauriInvoke('identity_load', { identityPath: ip });
    showIdentityReady(result);
  } catch (_) {
    // No identity yet — keep init area visible
  }
}

function showIdentityReady(result) {
  document.getElementById('identity-init-area').style.display = 'none';
  document.getElementById('qr-area').style.display = 'block';
  document.getElementById('identity-status').innerHTML =
    `<p class="msg msg-info">Identity: <strong>${escHtml(result.local_device_name)}</strong>
     (${escHtml(result.local_device_id)})</p>`;
}

async function initMyIdentity() {
  const name = document.getElementById('init-device-name').value.trim() || 'My Windows PC';
  const ip = getSetting('identityPath');
  if (!ip) {
    showMessage('qr-msg', 'Set identity path in Settings first', 'error');
    return;
  }
  try {
    const result = await tauriInvoke('identity_init', { identityPath: ip, deviceName: name });
    showIdentityReady(result);
    setStatus('Identity created: ' + result.local_device_name, 'ok');
  } catch (err) {
    showMessage('qr-msg', 'Error: ' + err.message, 'error');
  }
}

function toggleSettings() {
  const panel = document.getElementById('pairing-settings');
  panel.style.display = panel.style.display === 'none' ? 'block' : 'none';
}

function savePairingSettings() {
  setSetting('identityPath', document.getElementById('cfg-identity').value);
  setSetting('trustStorePath', document.getElementById('cfg-trust').value);
  showMessage('qr-msg', 'Settings saved', 'success');
}

async function generateMyQr() {
  const identityPath = getSetting('identityPath');
  const ttl = parseInt(document.getElementById('qr-ttl').value) || 120;
  try {
    const result = await tauriInvoke('pairing_generate_qr', {
      identityPath, ttlSeconds: ttl
    });
    // Show QR SVG
    const qrDisplay = document.getElementById('qr-display');
    qrDisplay.innerHTML = result.qr_svg;
    qrDisplay.style.display = 'flex';
    // Show info
    document.getElementById('qr-info').innerHTML = `
      <p><strong>${result.device_name}</strong></p>
      <p>ID: ${result.device_id}</p>
      <p>Fingerprint: ${result.fingerprint}</p>
      <p>TTL: ${result.ttl_seconds}s</p>
    `;
    // Show raw payload
    document.getElementById('qr-payload-text').value = result.payload;
    document.getElementById('qr-payload').style.display = 'block';
    setStatus('QR code generated', 'ok');
  } catch (err) {
    showMessage('qr-msg', 'Error: ' + err.message, 'error');
  }
}

async function inspectPeerPayload() {
  const identityPath = getSetting('identityPath');
  const payload = document.getElementById('scan-payload').value.trim();
  if (!payload) {
    showMessage('peer-msg', 'Paste a pairing payload first', 'error');
    return;
  }
  try {
    const result = await tauriInvoke('pairing_inspect', { identityPath, payload });
    document.getElementById('peer-details').innerHTML = `
      <p><strong>${result.device_name}</strong></p>
      <p>ID: ${result.device_id}</p>
      <p>Fingerprint: ${result.fingerprint}</p>
    `;
    document.getElementById('peer-info').style.display = 'block';
    document.getElementById('peer-confirmation-code').textContent = result.confirmation_code;
    document.getElementById('peer-confirmation-code').style.display = 'block';
    document.getElementById('confirm-group').style.display = 'block';
    document.getElementById('btn-confirm').style.display = 'inline-block';
    // Store payload for later confirm
    document.getElementById('scan-payload').dataset.parsedPayload = payload;
  } catch (err) {
    showMessage('peer-msg', 'Error: ' + err.message, 'error');
  }
}

async function confirmPairing() {
  const identityPath = getSetting('identityPath');
  const trustStorePath = getSetting('trustStorePath');
  const payload = document.getElementById('scan-payload').dataset.parsedPayload;
  const code = document.getElementById('confirm-input').value.trim();
  if (!code) {
    showMessage('peer-msg', 'Enter the confirmation code', 'error');
    return;
  }
  try {
    const result = await tauriInvoke('pairing_confirm', {
      identityPath, payload, confirmationCode: code, trustStorePath
    });
    showMessage('peer-msg',
      `Trusted! ${result.device_name} (${result.fingerprint})`, 'success');
    setStatus('Device paired: ' + result.device_name, 'ok');
    // Reset peer panel
    document.getElementById('peer-info').style.display = 'none';
    document.getElementById('confirm-input').value = '';
  } catch (err) {
    showMessage('peer-msg', 'Error: ' + err.message, 'error');
  }
}

// Init pairing tab on load
buildPairingTab();
