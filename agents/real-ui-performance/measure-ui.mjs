// 只读观测正式 UI；设置 fixture 发生于引擎启动前。
import {chromium} from '../../apps/web/node_modules/@playwright/test/index.mjs';
import {writeFile} from 'node:fs/promises';
const [target,output]=process.argv.slice(2);
const baseUrl='http://127.0.0.1:'+Number(process.env.BENCH_PORT||43123);
let browser;
let page, result={target,viewport:{width:390,height:1180},samples:[]};
try{
 browser=await chromium.launch({executablePath:process.env.BENCH_CHROMIUM || '/usr/bin/chromium',headless:true,args:['--no-sandbox','--disable-background-timer-throttling','--disable-renderer-backgrounding']});
 page=await browser.newPage({viewport:result.viewport,deviceScaleFactor:1});
 page.on('request',r=>{if(r.url().endsWith('/api/new')&&r.method()==='POST')result.nativeCreation=JSON.parse(r.postData());});
 const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(()=>{
  window.seedHits=0;const original=crypto.getRandomValues.bind(crypto);window.restoreEntropy=()=>{crypto.getRandomValues=original;};
  crypto.getRandomValues=a=>{if(a instanceof Uint32Array&&a.length===2){a[0]=0;a[1]=42;window.seedHits++;return a;}return original(a);};
  window.findFibers=(selector)=>{const el=selector?document.querySelector(selector):document.querySelector('#root').firstElementChild;const key=Object.keys(el).find(k=>k.startsWith('__reactFiber'));const out=[];let f=el[key];while(f){out.push(f);if(f.alternate)out.push(f.alternate);f=f.return;}return out;};
  window.initialization=[];window.restorePosts=[];
  for(const proto of [Worker.prototype,MessagePort.prototype]){const original=proto.postMessage;window.restorePosts.push(()=>proto.postMessage=original);proto.postMessage=function(value,...args){if(value?.type==='init'||value?.type==='create')window.initialization.push({type:value.type,setup:value.setup,seed:value.seed?.toString(),waitSettings:value.waitSettings});return original.call(this,value,...args);};}
 });
 await page.goto(baseUrl);await page.waitForSelector('#startup-title');
 result.setup=await page.evaluate(()=>{for(const f of window.findFibers()){let h=f.memoizedState;while(h){if(h.memoizedState?.stocks&&h.queue?.dispatch){const d=h.memoizedState;const stocks=['600101','002156'].map(c=>d.stocks.find(s=>s.code===c));if(stocks.some(s=>!s)||stocks[0].exchange!=='Shanghai'||stocks[1].exchange!=='Shenzhen')throw Error('股票 fixture 错误');const setup={...d,stocks,npcs:{...d.npcs,retail_count:64,inst_count:4,hot_count:2},start_date:'2030-01-04',ticks_per_day:60000,auction_ticks:3,closing_auction_ticks:3,runtime_sync_policy:'every_tick'};h.queue.dispatch(setup);return setup;}h=h.next;}}throw Error('SessionSetup hook 缺失');});
 await page.waitForFunction(()=>window.findFibers().some(f=>{let h=f.memoizedState;while(h){if(h.memoizedState?.npcs?.retail_count===64)return true;h=h.next;}return false;}));
 if(target==='native'){await page.locator('input[value="remote"]').check();await page.getByRole('textbox').fill(baseUrl);}
 await page.locator('button[type="submit"]').click();
 if(target==='native'){await page.getByRole('button',{name:'明确创建匿名身份并登录',exact:true}).click();await page.getByLabel('确认创建共享市场并理解全局影响').check();await page.getByRole('button',{name:'创建共享市场',exact:true}).click();}
 await page.waitForSelector('.mobile-market-row',{timeout:35000});
 result.actual=await page.evaluate(()=>{for(const f of window.findFibers('.mobile-market-row')){if(f.memoizedProps?.hostRef?.current){window.host=f.memoizedProps.hostRef.current;window.restoreEntropy();for(const restore of window.restorePosts)restore();return {setup:f.memoizedProps.sessionSetup,baseline:window.host.snapshot(),capabilities:window.host.capabilities,deliveryMode:window.host.getDeliveryMode?.(),initialization:window.initialization,seedHits:window.seedHits};}}throw Error('hostRef 缺失');});
 if(JSON.stringify(result.actual.setup)!==JSON.stringify(result.setup))throw Error('实际 setup 不一致');
 if(result.actual.seedHits!==1)throw Error('新局 seed 命中次数不为一');
 if(result.actual.baseline.day!==0||result.actual.baseline.tick!==0)throw Error('市场不是首日的新会话');
 if(target==='native'){if(result.actual.deliveryMode!=='push')throw Error('Native 未进入 Push 投递模式');if(result.nativeCreation.seed!=='42'||JSON.stringify(result.nativeCreation.setup)!==JSON.stringify(result.setup))throw Error('Native 初始化 fixture 不符');}else{const init=result.actual.initialization.find(m=>m.type==='init'&&m.setup);const create=result.actual.initialization.find(m=>m.type==='create');if(!init||JSON.stringify(init.setup)!==JSON.stringify(result.setup)||create?.seed!=='42')throw Error('WASM 初始化 fixture 不符');}
 result.workers=page.workers().map(w=>w.url());
 await page.locator('.mobile-market-row').first().click();await page.waitForSelector('.mobile-stock-detail');
 const cdp=await page.context().newCDPSession(page);await cdp.send('Performance.enable');
 await page.evaluate(()=>{
  window.metrics=[];const read=window.host.readSpeedMetrics.bind(window.host);window.host.readSpeedMetrics=async()=>{const m=await read();window.metrics.push({at:performance.now(),...m});return m;};
  window.visual={active:false,raf:[],commits:[],longTasks:[]};const root=document.querySelector('.app-root');
  function raf(t){if(window.visual.active)window.visual.raf.push({at:t,tick:Number(root.dataset.gameTick)});requestAnimationFrame(raf);}requestAnimationFrame(raf);
  new MutationObserver(()=>{if(window.visual.active)window.visual.commits.push({at:performance.now(),tick:Number(root.dataset.gameTick)});}).observe(root,{attributes:true,attributeFilter:['data-game-tick']});
  new PerformanceObserver(l=>{if(window.visual.active)window.visual.longTasks.push(...l.getEntries().map(e=>({start:e.startTime,duration:e.duration})));}).observe({type:'longtask'});
 });
 // 1× 预热至 tick 5，此时两端都已通过两拍集合竞价与一拍 PreOpen。
 await page.locator('.mobile-stock-detail select[aria-label="模拟速度"]').selectOption('1');
 if(await page.locator('.mobile-run-toggle--detail').getAttribute('data-state')==='paused')await page.locator('.mobile-run-toggle--detail').click();
 await page.waitForFunction(()=>Number(document.querySelector('.app-root').dataset.gameTick)>=5,{},{timeout:12000});
 for(const speed of ['1','Infinity']){
  await page.locator('.mobile-stock-detail select[aria-label="模拟速度"]').selectOption(speed);
  await page.waitForTimeout(speed==='1'?500:1000);
  const beforeMetrics=await cdp.send('Performance.getMetrics');
  const start=await page.evaluate(()=>{window.visual={active:true,raf:[],commits:[],longTasks:[]};window.sampleStart=performance.now();return {at:window.sampleStart,tick:Number(document.querySelector('.app-root').dataset.gameTick)};});
  await page.waitForTimeout(speed==='1'?10000:5000);
  const end=await page.evaluate(()=>{window.visual.active=false;return {at:performance.now(),tick:Number(document.querySelector('.app-root').dataset.gameTick),visual:window.visual,metrics:window.metrics.filter(m=>m.at>=window.sampleStart)};});
  if(!Number.isSafeInteger(start.tick)||!Number.isSafeInteger(end.tick)||start.tick<5||end.tick<=start.tick||end.tick>=result.setup.ticks_per_day-result.setup.closing_auction_ticks)throw Error('界面游标未在首日的有效采样区间内推进');
  const afterMetrics=await cdp.send('Performance.getMetrics');
  result.samples.push({speed,start,end,beforeMetrics,afterMetrics});
 }
 await page.locator('.mobile-run-toggle--detail').click();
 if(await page.locator('.app-error,.speed-metrics-error').count())throw Error('页面显示运行错误');
 result.errors=errors;result.finalBody=await page.locator('body').innerText();
 await page.screenshot({path:output+'.png'});
 if(errors.length)throw Error(errors.join('\n'));
 console.log(JSON.stringify({target,samples:result.samples.map(s=>({speed:s.speed,uiTicks:s.end.tick-s.start.tick,elapsed:s.end.at-s.start.at,metrics:s.end.metrics}))}));
} catch(e){result.failure=String(e);if(page){result.failureBody=await page.locator('body').innerText();await page.screenshot({path:output+'-failure.png'});}throw e;}
finally{try{await writeFile(output+'.json',JSON.stringify(result,null,2));}finally{if(browser)await browser.close();}}
