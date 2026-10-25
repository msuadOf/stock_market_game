import { createServer } from '../../../apps/web/node_modules/vite/dist/node/index.js';
import { chromium } from '../../../apps/web/node_modules/@playwright/test/index.mjs';
const startedAt = performance.now();
const stage = (name) => console.log(JSON.stringify({ 阶段: name, 已用毫秒: Math.round(performance.now() - startedAt) }));
stage("创建 Vite server");
const server = await createServer({
  root: new URL('../../../apps/web/', import.meta.url).pathname,
  configFile: false,
  server: { host: '127.0.0.1', port: 0 },
  optimizeDeps: { noDiscovery: true },
});
let browser;
let browserServer;
try {
  await server.listen();
  stage("启动 Chromium");
  browserServer = await chromium.launchServer({ headless: true, args: ['--no-sandbox'], timeout: 5000 });
  stage("连接 Chromium");
  browser = await chromium.connect(browserServer.wsEndpoint());
  const page = await browser.newPage();
  stage("加载同源页面");
  const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  await page.goto(`${origin}/src/host/wasm-failure.ts`);
  stage("并发检查两个实际 Worker");
  const results = await page.evaluate(async (origin) => Promise.all([true, false].map(async (enabled) => {
    const source = `let listener;
      const originalAdd = self.addEventListener.bind(self);
      self.addEventListener = (type, callback) => { if(type === 'message') listener = callback; else originalAdd(type, callback); };
      const {postFailure} = await import('${origin}/src/host/wasm-worker.ts');
      if (!${enabled}) self.reportError = undefined;
      self.postMessage = () => { throw new Error('real port unavailable'); };
      const request = Object.defineProperty({type:'speedMetrics',requestId:49},'generation',{get(){throw {code:'STEP_FATAL',message:'真实 Worker 原始故障'};}});
      listener({data:request});`;
    const url = URL.createObjectURL(new Blob([source], { type: 'text/javascript' }));
    const worker = new Worker(url, { type: 'module' });
    try {
      return await new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error('Worker error event timeout')), 5000);
        worker.onerror = (event) => {
          clearTimeout(timer);
          event.preventDefault();
          resolve({ reportErrorEnabled: enabled, message: event.message });
        };
      });
    } finally { worker.terminate(); URL.revokeObjectURL(url); }
  })), origin);
  for (const result of results) {
    if (!/STEP_FATAL.*真实 Worker 原始故障.*real port unavailable.*real port unavailable/.test(result.message)) {
      throw new Error(`unexpected Worker error: ${JSON.stringify(result)}`);
    }
  }
  console.log(JSON.stringify({ browser: await browser.version(), results }));
} finally {
  stage("终止 Chromium");
  await browserServer?.kill();
  await browser?.close();
  server.httpServer.closeAllConnections();
  await server.close();
  stage("清理完成");
}
