// ── Service Tab (Listener) ──────────────────────────────────────────
function buildServiceTab() {
  const section = document.getElementById('tab-service');
  section.innerHTML = `
    <div class="card">
      <h3>监听服务</h3>
      <p>让 LinkHub 在后台持续监听来自其他设备的连接。</p>
      <div class="form-group">
        <label>监听地址</label>
        <input type="text" id="svc-addr" placeholder="127.0.0.1:8787">
      </div>
      <div class="form-group">
        <label>接收目录</label>
        <div class="inline-row">
          <input type="text" id="svc-receive-dir" placeholder="C:\\LinkHub\\inbox">
          <button class="btn btn-secondary" data-act="chooseReceiveDir">浏览…</button>
        </div>
      </div>
      <div style="display:flex;gap:12px;align-items:center;margin:12px 0">
        <button class="btn btn-primary" id="btn-start-listener" data-act="startListener">开始监听</button>
        <button class="btn btn-danger" id="btn-stop-listener" data-act="stopListener" disabled>停止</button>
        <span id="svc-running-indicator" style="font-weight:600;color:var(--danger)">&#9679; 已停止</span>
      </div>
      <p class="device-meta" id="svc-mdns-status">mDNS 广播：已停止</p>
    </div>
    <div class="card" id="listener-info-card" style="display:none">
      <h3>监听信息</h3>
      <div id="listener-info"></div>
    </div>
    <div class="card">
      <h3>网络地址提示</h3>
      <p>在同一网络的另一台可信设备上使用以下地址。</p>
      <div id="network-hints"><p class="device-meta">正在检测…</p></div>
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
      ? `mDNS 广播：${status.service_name}`
      : 'mDNS 广播：已停止';
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
      container.innerHTML = '<p class="device-meta">未检测到本地网络地址。</p>';
      return;
    }
    container.innerHTML = hints.map(h => `
      <div class="hint-row">
        <span>${escHtml(h.label)}</span>
        <code>${escHtml(h.address)}</code>
      </div>
    `).join('');
  } catch (err) {
    container.innerHTML = `<p class="device-meta">无法读取网络地址提示：${escHtml(err.message)}</p>`;
  }
}

async function chooseReceiveDir() {
  try {
    const path = await tauriInvoke('choose_folder_path');
    if (path) {
      document.getElementById('svc-receive-dir').value = path;
      setSetting('receiveDir', path);
      setStatus('已选择接收目录', 'ok');
    }
  } catch (err) {
    showMessage('service-msg', '目录选择出错：' + err.message, 'error');
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
      indicator.innerHTML = '&#9679; 运行中';
      indicator.style.color = 'var(--success)';
    }
    if (infoCard) {
      infoCard.style.display = 'block';
      document.getElementById('listener-info').innerHTML = `
        <p>监听地址：<strong>${escHtml(status.bind_addr)}</strong></p>
        <p>身份文件：<code>${escHtml(getSetting('identityPath'))}</code></p>
        <p>信任库：<code>${escHtml(getSetting('trustStorePath'))}</code></p>
      `;
    }
  } else {
    btnStart.disabled = false;
    btnStop.disabled = true;
    if (indicator) {
      indicator.innerHTML = '&#9679; 已停止';
      indicator.style.color = 'var(--danger)';
    }
    if (infoCard) {
      if (status.error) {
        infoCard.style.display = 'block';
        document.getElementById('listener-info').innerHTML = `
          <p><strong>上次错误：</strong> ${escHtml(status.error)}</p>
          <p>上次地址：<code>${escHtml(status.bind_addr || getSetting('listenerAddr'))}</code></p>
        `;
        setStatus('监听出错：' + status.error, 'error');
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
    showMessage('service-msg', '尚未配置身份。请先到「配对」页。', 'error');
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
      showMessage('service-msg', '监听已启动，但 mDNS 广播失败：' + mdnsErr.message, 'error');
    }
    updateListenerUI(result);
    setStatus('监听运行于 ' + addr, 'ok');
    showMessage('service-msg', '监听已启动于 ' + addr, 'success');
  } catch (err) {
    showMessage('service-msg', '错误：' + err.message, 'error');
  }
}

async function stopListener() {
  try {
    const result = await tauriInvoke('stop_listener');
    await tauriInvoke('stop_mdns_advertise');
    await checkMdnsStatus();
    updateListenerUI(result);
    setStatus('监听已停止', 'ok');
    showMessage('service-msg', '监听已停止', 'info');
  } catch (err) {
    showMessage('service-msg', '错误：' + err.message, 'error');
  }
}

// Init
buildServiceTab();
setInterval(checkListenerStatus, 5000);  // Poll every 5s
setInterval(checkMdnsStatus, 5000);
