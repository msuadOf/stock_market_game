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
      const progress = self.postMessage.bind(self);
      progress('Blob 已启动，准备 import');
      const originalAdd = self.addEventListener.bind(self);
      self.addEventListener = (type, callback) => {
        if(type === 'message') {
          listener = callback;
          progress('已捕获 listener 绑定：' + callback.toString().slice(0, 100));
        } else originalAdd(type, callback);
      };
      try { await import('${origin}/src/host/wasm-worker.ts'); }
      catch(error) { progress('import 失败：' + String(error)); throw error; }
      progress('import 已完成');
      if (${enabled}) {
        const nativeReportError = self.reportError.bind(self);
        self.reportError = error => { progress('调用 native reportError'); nativeReportError(error); };
      } else {
        self.reportError = undefined;
        const nativeQueue = self.queueMicrotask.bind(self);
        self.queueMicrotask = callback => {
          progress('排入 noAPI microtask');
          nativeQueue(() => { progress('执行 noAPI microtask'); callback(); });
        };
      }
      let posts = 0;
      self.postMessage = () => { progress('postMessage 抛错：' + (++posts)); throw new Error('real port unavailable'); };
      const request = Object.defineProperty({type:'speedMetrics',requestId:49},'generation',{get(){throw {code:'STEP_FATAL',message:'真实 Worker 原始故障'};}});
      progress('准备实际调用 listener，类型：' + typeof listener);
      listener({data:request});
      progress('listener 同步调用已返回');`;
    const url = URL.createObjectURL(new Blob([source], { type: 'text/javascript' }));
    const worker = new Worker(url, { type: 'module' });
    try {
      return await new Promise((resolve, reject) => {
        const progress = [];
        worker.onmessage = (event) => { progress.push(event.data); };
        const timer = setTimeout(() => reject(new Error(`Worker error event timeout：${JSON.stringify({ reportErrorEnabled: enabled, progress })}`)), 5000);
        worker.onerror = (event) => {
          clearTimeout(timer);
          event.preventDefault();
          progress.push('parent 捕获 Worker error');
          resolve({ reportErrorEnabled: enabled, message: event.message, progress });
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
