// Frontend benchmark E2E via Playwright
// - Can start the RaftStone server automatically when START_SERVER=1
// - Otherwise assumes a server is already running at BASE_URL

const { test, expect } = require('@playwright/test');
const cp = require('child_process');
const http = require('http');

const PORT = process.env.PORT || '18090';
const BASE = process.env.BASE_URL || `http://127.0.0.1:${PORT}`;
const RS_STATIC_DIR = process.env.RS_STATIC_DIR || 'tests/frontend_benchmark/dist';

let child = null;

async function waitForStatus(url, timeoutMs = 15000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const ok = await new Promise((resolve) => {
        const req = http.get(url, (res) => {
          res.resume();
          resolve(res.statusCode === 200);
        });
        req.on('error', () => resolve(false));
      });
      if (ok) return true;
    } catch { /* ignore */ }
    await new Promise(r => setTimeout(r, 250));
  }
  return false;
}

test.beforeAll(async () => {
  if (process.env.START_SERVER === '1') {
    console.log(`Starting RaftStone server on :${PORT} with RS_STATIC_DIR=${RS_STATIC_DIR}`);
    child = cp.spawn('cargo', [
      'run', '--release', '-p', 'raft_replication_demo', '--bin', 'http_firewall_declarative', '--', '--port', PORT
    ], {
      env: { ...process.env, RS_STATIC_DIR },
      stdio: 'inherit'
    });
    const ok = await waitForStatus(`${BASE}/status`, 30000);
    if (!ok) throw new Error('Server did not become ready');
  }
});

test.afterAll(async () => {
  if (child) {
    try { child.kill('SIGKILL'); } catch {}
  }
});

test('homepage and CRUD flow', async ({ page }) => {
  await page.goto(`${BASE}/`);
  await expect(page.locator('h1')).toHaveText(/Frontend Benchmark/i);
  await expect(page.locator('#api-base')).toContainText('/api/products');
  // Initial load already triggers one GET /api/products via main.js
  // Avoid hitting per-ip QPS=2 by spacing subsequent calls across seconds
  await page.waitForTimeout(1200);

  const unique = `Demo ${Date.now()}`;
  await page.fill('#name', unique);
  await page.fill('#price', '9.99');
  await page.click('#create-form button[type="submit"]');

  // Model-level QPS limit may throttle immediate re-reads, wait to cross window
  await page.waitForTimeout(1500);
  await page.click('#refresh-btn');

  await expect(page.locator('#products-body tr')).toContainText(unique);

  // Metrics should have captured at least one entry
  const metricsText = await page.textContent('#metrics');
  expect(metricsText).toMatch(/\"name\":/);
});
