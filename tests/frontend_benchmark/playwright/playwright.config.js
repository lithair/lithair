// @ts-check
const { defineConfig } = require('@playwright/test');

module.exports = defineConfig({
  testDir: __dirname,
  timeout: 60_000,
  retries: 0,
  reporter: 'list',
  use: {
    headless: true,
    trace: 'off',
  },
});
