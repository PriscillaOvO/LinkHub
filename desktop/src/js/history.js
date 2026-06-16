// ── History Tab ─────────────────────────────────────────────────────
function buildHistoryTab() {
  const section = document.getElementById( 'tab-history' );
  section.innerHTML = `
    <div class="card">
      <div style="display:flex;justify-content:space-between;align-items:center">
        <h3>Transmission History</h3>
        <button class="btn btn-secondary" onclick="clearHistory()">Clear All</button>
      </div>
      <div id="history-list">
        <p class="device-meta">No transmissions recorded yet.</p>
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
        '<p class="device-meta">No transmissions recorded yet.</p>';
      return;
    }

    // Sort newest first
    entries.sort((a, b) => parseInt(b.timestamp) - parseInt(a.timestamp));

    const rows = entries.map(e => {
      const dirIcon = e.direction === 'sent' ? '&#8593;' : '&#8595;';
      const dirColor = e.direction === 'sent' ? 'var(--accent)' : 'var(--success)';
      const kindLabel = e.kind === 'file' ? '[FILE]' : '[TEXT]';
      const status = e.status || 'success';
      const statusColor = status === 'success' ? 'var(--success)' : 'var(--danger)';
      const time = new Date(parseInt(e.timestamp) * 1000).toLocaleString();
      return `<tr>
        <td style="color:${dirColor};font-weight:600">${dirIcon} ${e.direction.toUpperCase()}</td>
        <td>${escHtml(kindLabel)}</td>
        <td>${escHtml(e.peer_device_name)}</td>
        <td style="font-size:0.8rem;max-width:200px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${escHtml(e.content_preview)}</td>
        <td style="color:${statusColor};font-weight:600">${escHtml(status.toUpperCase())}</td>
        <td style="font-size:0.75rem;color:var(--text-secondary)">${time}</td>
      </tr>`;
    }).join('');

    document.getElementById('history-list').innerHTML = `
      <table class="history-table">
        <thead><tr>
          <th>Dir</th><th>Type</th><th>Peer</th><th>Content</th><th>Status</th><th>Time</th>
        </tr></thead>
        <tbody>${rows}</tbody>
      </table>
    `;
  } catch (err) {
    showMessage('history-msg', 'Error: ' + err.message, 'error');
  }
}

async function clearHistory() {
  const historyPath = getSetting('historyPath');
  try {
    await tauriInvoke('clear_history', { historyPath });
    renderHistoryTab();
    showMessage('history-msg', 'History cleared', 'info');
  } catch (err) {
    showMessage('history-msg', 'Error: ' + err.message, 'error');
  }
}

// Init
buildHistoryTab();
