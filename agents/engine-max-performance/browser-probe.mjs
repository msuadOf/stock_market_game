// 无头浏览器通过 Vite 启动真实页面，记录默认场景的 max 吞吐。
import { createRequire } from 'node:module';
import { writeFile } from 'node:fs/promises';
const require = createRequire(new URL('../../apps/web/package.json', import.meta.url));
const { chromium } = require('@playwright/test');
const browser = await chromium.launch({ executablePath: '/home/baiyifan/.cache/ms-playwright/chromium-1243/chrome-linux64/chrome', headless: true, args: ['--no-sandbox', '--disable-background-timer-throttling'] });
const page = await browser.newPage({viewport:{width:1440,height:1000}});
const errors = [];
page.on('pageerror', error => errors.push(String(error)));
page.on('console', msg => { if (msg.type() === 'error') errors.push(msg.text()); });
await page.addInitScript(() => Object.defineProperty(navigator, 'hardwareConcurrency', {get:()=>16}));
try {
  await page.goto('http://127.0.0.1:5187');
  await page.waitForSelector('#startup-title');
  await page.locator('input[name="startup-mode"][value="local"]').check();
  await page.locator('button[type="submit"]').click();
  await page.waitForSelector('.app-root', {timeout:60000});
  const result = await page.evaluate(async () => {
    const { createWorkerHost } = await import('/src/host/worker-host.ts');
    const { DEFAULT_SETUP } = await import('/src/config/defaults.ts');
    const host = await createWorkerHost(DEFAULT_SETUP, 42n, {threadCount:16});
    const errors = [];
    let committedTicks = 0;
    await host.setSpeed(Infinity);
    await host.start(update => { if (update.type === 'protocol' && update.update && 'TickBatch' in update.update) committedTicks += update.update.TickBatch.frames.length; return true; }, error => errors.push(error));
    const warmup = performance.now();
    while (committedTicks < 100) {
      if (errors.length > 0) throw new Error(JSON.stringify(errors));
      if (performance.now() - warmup > 120000) throw new Error('市场 warmup 超时');
      await new Promise(resolve => setTimeout(resolve,100));
    }
    const beforeTick = committedTicks;
    const start = performance.now();
    await new Promise(resolve => setTimeout(resolve,10000));
    const metrics = await host.readSpeedMetrics();
    const afterTick = committedTicks;
    const wall_ms = performance.now()-start;
    await host.stop();
    await host.dispose();
    return {metrics,beforeTick,afterTick,wall_ms,speed_x:(afterTick-beforeTick)*1000/wall_ms,errors,isolated:crossOriginIsolated};
  });
  await writeFile(new URL(`./${process.argv[2] ?? 'baseline-browser'}.json`, import.meta.url), JSON.stringify({result,errors},null,2));
  console.log(JSON.stringify({result,errors}));
} catch (error) {
  console.error(error,errors);
  await page.screenshot({path:new URL('./browser-failure.png',import.meta.url).pathname});
  throw error;
} finally {
  await browser.close();
}
