(() => {
  const API_BASE = window.API_BASE_URL || location.origin;
  const BASE_PATH = '/api/products';

  const $serverInfo = document.getElementById('server-info');
  const $apiBase = document.getElementById('api-base');
  const $tbody = document.getElementById('products-body');
  const $metrics = document.getElementById('metrics');
  const $form = document.getElementById('create-form');
  const $refresh = document.getElementById('refresh-btn');

  $apiBase.textContent = API_BASE + BASE_PATH;

  function timeFetch(label, url, init) {
    const t0 = performance.now();
    return fetch(url, init).then(async (res) => {
      const t1 = performance.now();
      captureMetric(label, t1 - t0, res.status);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const ct = res.headers.get('content-type') || '';
      if (ct.includes('application/json')) return res.json();
      return res.text();
    });
  }

  const METRICS = [];
  function captureMetric(name, ms, status) {
    METRICS.push({ name, ms: Math.round(ms), status, ts: Date.now() });
    $metrics.textContent = JSON.stringify(METRICS.slice(-50), null, 2);
  }

  async function getStatus() {
    try {
      const data = await timeFetch('status', API_BASE + '/status');
      $serverInfo.textContent = `Connected • model=${data.model || 'N/A'} • base_path=${data.base_path || '/api/products'}`;
    } catch (e) {
      $serverInfo.textContent = 'Status failed.';
    }
  }

  async function loadProducts() {
    $tbody.innerHTML = '<tr><td colspan="3" class="muted">Loading…</td></tr>';
    try {
      const items = await timeFetch('list', API_BASE + BASE_PATH);
      if (!Array.isArray(items)) throw new Error('Invalid JSON');
      if (items.length === 0) {
        $tbody.innerHTML = '<tr><td colspan="3" class="muted">No products.</td></tr>';
        return;
      }
      $tbody.innerHTML = items.map(p => `
        <tr>
          <td class="id">${escapeHtml(p.id)}</td>
          <td>${escapeHtml(p.name)}</td>
          <td>${Number(p.price).toFixed(2)}</td>
        </tr>
      `).join('');
    } catch (e) {
      $tbody.innerHTML = `<tr><td colspan="3" class="error">Load failed: ${escapeHtml(e.message)}</td></tr>`;
    }
  }

  $form.addEventListener('submit', async (ev) => {
    ev.preventDefault();
    const name = document.getElementById('name').value.trim();
    const price = parseFloat(document.getElementById('price').value);
    if (!name || !(price > 0)) return;
    try {
      await timeFetch('create', API_BASE + BASE_PATH, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ name, price })
      });
      await loadProducts();
      $form.reset();
    } catch (e) {
      alert('Create failed: ' + e.message);
    }
  });

  $refresh.addEventListener('click', loadProducts);

  function escapeHtml(s) {
    return String(s).replace(/[&<>"']/g, c => ({
      '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'
    }[c]));
  }

  // Initial
  getStatus().then(loadProducts);
})();
