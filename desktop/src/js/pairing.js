// ── Pairing Tab ─────────────────────────────────────────────────────

function buildPairingTab() {
  const section = document.getElementById('tab-pairing');
  section.innerHTML = `
    <!-- Settings -->
    <div class="settings-toggle" data-act="toggleSettings">&#9881; 设置</div>
    <div id="pairing-settings" class="settings-panel" style="display:none">
      <div class="form-group">
        <label>身份文件路径</label>
        <input type="text" id="cfg-identity" placeholder="C:\LinkHub\local-identity.secure.txt">
      </div>
      <div class="form-group">
        <label>信任库路径</label>
        <input type="text" id="cfg-trust" placeholder="C:\LinkHub\trust-store.txt">
      </div>
      <button class="btn btn-primary" data-act="savePairingSettings">保存</button>
    </div>

    <div class="pairing-grid">
      <!-- Left: Show my QR -->
      <div class="card">
        <h3>我的设备二维码</h3>
        <div id="identity-init-area">
          <p>首次使用？先创建本机设备身份：</p>
          <div class="form-group">
            <label>设备名称</label>
            <input type="text" id="init-device-name" placeholder="我的 Windows 电脑">
          </div>
          <button class="btn btn-primary" data-act="initMyIdentity">初始化身份</button>
          <div id="identity-status"></div>
        </div>
        <div id="qr-area" style="display:none">
          <p>生成一个二维码，供另一台可信设备扫描。</p>
          <div class="form-group">
            <label>有效期（秒）</label>
            <input type="text" id="qr-ttl" value="120">
          </div>
          <button class="btn btn-primary" data-act="generateMyQr">生成二维码</button>
        </div>
        <div id="qr-display" class="qr-container" style="display:none"></div>
        <div id="qr-info"></div>
        <div id="qr-payload" style="display:none">
          <div class="form-group">
            <label>配对凭据（可复制用于手动交换）</label>
            <textarea id="qr-payload-text" readonly rows="2"></textarea>
          </div>
        </div>
        <div id="qr-msg"></div>
      </div>

      <!-- Right: Scan peer -->
      <div class="card">
        <h3>扫描对端</h3>
        <p>粘贴另一台设备的配对凭据以开始配对。</p>
        <div class="form-group">
          <label>对端的配对凭据</label>
          <textarea id="scan-payload" placeholder="linkhub-pair-v2|..."></textarea>
        </div>
        <button class="btn btn-primary" data-act="inspectPeerPayload">解析</button>
        <div id="peer-info" style="display:none">
          <div id="peer-details"></div>
          <div id="peer-confirmation-code" class="confirmation-code" style="display:none"></div>
          <div class="form-group" id="confirm-group" style="display:none">
            <label>输入上方显示的确认码</label>
            <input type="text" id="confirm-input" placeholder="ABC-DEF">
          </div>
          <button class="btn btn-primary" id="btn-confirm" style="display:none"
                  data-act="confirmPairing">确认配对</button>
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
    `<p class="msg msg-info">身份：<strong>${escHtml(result.local_device_name)}</strong>
     (${escHtml(result.local_device_id)})</p>`;
}

async function initMyIdentity() {
  const name = document.getElementById('init-device-name').value.trim() || '我的 Windows 电脑';
  const ip = getSetting('identityPath');
  if (!ip) {
    showMessage('qr-msg', '请先在「设置」中填写身份文件路径', 'error');
    return;
  }
  try {
    const result = await tauriInvoke('identity_init', { identityPath: ip, deviceName: name });
    showIdentityReady(result);
    setStatus('已创建身份：' + result.local_device_name, 'ok');
  } catch (err) {
    showMessage('qr-msg', '错误：' + err.message, 'error');
  }
}

function toggleSettings() {
  const panel = document.getElementById('pairing-settings');
  panel.style.display = panel.style.display === 'none' ? 'block' : 'none';
}

function savePairingSettings() {
  setSetting('identityPath', document.getElementById('cfg-identity').value);
  setSetting('trustStorePath', document.getElementById('cfg-trust').value);
  showMessage('qr-msg', '设置已保存', 'success');
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
      <p>ID：${result.device_id}</p>
      <p>指纹：${result.fingerprint}</p>
      <p>有效期：${result.ttl_seconds} 秒</p>
    `;
    // Show raw payload
    document.getElementById('qr-payload-text').value = result.payload;
    document.getElementById('qr-payload').style.display = 'block';
    setStatus('二维码已生成', 'ok');
  } catch (err) {
    showMessage('qr-msg', '错误：' + err.message, 'error');
  }
}

async function inspectPeerPayload() {
  const identityPath = getSetting('identityPath');
  const payload = document.getElementById('scan-payload').value.trim();
  if (!payload) {
    showMessage('peer-msg', '请先粘贴配对凭据', 'error');
    return;
  }
  try {
    const result = await tauriInvoke('pairing_inspect', { identityPath, payload });
    document.getElementById('peer-details').innerHTML = `
      <p><strong>${result.device_name}</strong></p>
      <p>ID：${result.device_id}</p>
      <p>指纹：${result.fingerprint}</p>
    `;
    document.getElementById('peer-info').style.display = 'block';
    document.getElementById('peer-confirmation-code').textContent = result.confirmation_code;
    document.getElementById('peer-confirmation-code').style.display = 'block';
    document.getElementById('confirm-group').style.display = 'block';
    document.getElementById('btn-confirm').style.display = 'inline-block';
    // Store payload for later confirm
    document.getElementById('scan-payload').dataset.parsedPayload = payload;
  } catch (err) {
    showMessage('peer-msg', '错误：' + err.message, 'error');
  }
}

async function confirmPairing() {
  const identityPath = getSetting('identityPath');
  const trustStorePath = getSetting('trustStorePath');
  const payload = document.getElementById('scan-payload').dataset.parsedPayload;
  const code = document.getElementById('confirm-input').value.trim();
  if (!code) {
    showMessage('peer-msg', '请输入确认码', 'error');
    return;
  }
  try {
    const result = await tauriInvoke('pairing_confirm', {
      identityPath, payload, confirmationCode: code, trustStorePath
    });
    showMessage('peer-msg',
      `已信任！${result.device_name}（${result.fingerprint}）`, 'success');
    setStatus('设备已配对：' + result.device_name, 'ok');
    // Reset peer panel
    document.getElementById('peer-info').style.display = 'none';
    document.getElementById('confirm-input').value = '';
  } catch (err) {
    showMessage('peer-msg', '错误：' + err.message, 'error');
  }
}

// Init pairing tab on load
buildPairingTab();
