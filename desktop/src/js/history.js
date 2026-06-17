// ── History Tab ─────────────────────────────────────────────────────
function buildHistoryTab() {
  const section = document.getElementById( 'tab-history' );
  section.innerHTML = `
    <div class="card">
      <div style="display:flex;justify-content:space-between;align-items:center">
        <h3>传输历史</h3>
        <button class="btn btn-secondary" data-act="clearHistory">清空全部</button>
      </div>
      <div id="history-list">
        <p class="device-meta">暂无传输记录。</p>
      </div>
    </div>
    <div id="history-msg"></div>
  `;
}

async function renderHistoryTab() {
  if (!document.getElementById('history-list')) buildHistoryTab();

  const historyPath = getSetting('historyPath');
  try {
    const result = await tauriInvoke('get_history', { historyPath });
    const entries = result.entries || [];

    if (entries.length === 0) {
      document.getElementById('history-list').innerHTML =
        '<p class="device-meta">暂无传输记录。</p>';
      return;
    }

    // Sort newest first
    entries.sort((a, b) => parseInt(b.timestamp) - parseInt(a.timestamp));

    const rows = entries.map(e => {
      const dirIcon = e.direction === 'sent' ? '&#8593;' : '&#8595;';
      const dirColor = e.direction === 'sent' ? 'var(--accent)' : 'var(--success)';
      const dirLabel = e.direction === 'sent' ? '发送' : '接收';
      const kindLabel = e.kind === 'file' ? '[文件]' : '[文本]';
      const status = e.status || 'success';
      const statusColor = status === 'success' ? 'var(--success)' : 'var(--danger)';
      const statusLabel = status === 'success' ? '成功' : status;
      const time = new Date(parseInt(e.timestamp) * 1000).toLocaleString();
      return `<tr>
        <td style="color:${dirColor};font-weight:600">${dirIcon} ${dirLabel}</td>
        <td>${escHtml(kindLabel)}</td>
        <td>${escHtml(e.peer_device_name)}</td>
        <td style="font-size:0.8rem;max-width:200px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${escHtml(e.content_preview)}</td>
        <td style="color:${statusColor};font-weight:600">${escHtml(statusLabel)}</td>
        <td style="font-size:0.75rem;color:var(--text-secondary)">${time}</td>
      </tr>`;
    }).join('');

    document.getElementById('history-list').innerHTML = `
      <table class="history-table">
        <thead><tr>
          <th>方向</th><th>类型</th><th>对端</th><th>内容</th><th>状态</th><th>时间</th>
        </tr></thead>
        <tbody>${rows}</tbody>
      </table>
    `;
  } catch (err) {
    showMessage('history-msg', '错误：' + err.message, 'error');
  }
}

async function clearHistory() {
  const historyPath = getSetting('historyPath');
  try {
    await tauriInvoke('clear_history', { historyPath });
    renderHistoryTab();
    showMessage('history-msg', '历史已清空', 'info');
  } catch (err) {
    showMessage('history-msg', '错误：' + err.message, 'error');
  }
}

// Init
buildHistoryTab();
