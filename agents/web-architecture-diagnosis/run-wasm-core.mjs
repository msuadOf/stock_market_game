// 顺序独立浏览器实例，同一诊断 WASM 仅更改 wait quantum。
import {createRequire} from 'node:module';import {writeFile} from 'node:fs/promises';
const require=createRequire(new URL('../../apps/web/package.json',import.meta.url));const {createServer}=await import(require.resolve('vite'));const {chromium}=require('@playwright/test');
let server;const results=[];
try {
 server=await createServer({root:'apps/web',configFile:'apps/web/vite.config.ts',server:{host:'127.0.0.1',port:4198,strictPort:true},plugins:[{name:'core-bare',configureServer(s){s.middlewares.use('/core-bare',(_q,r)=>{r.setHeader('Content-Type','text/html');r.end('<!doctype html><html><script type="module" src="/@fs/Users/asuad/workplace/stock_market_game/agents/web-architecture-diagnosis/wasm-stage/browser/main.ts"></script></html>');});}}]});await server.listen();
 for(const sleepUs of [500,50,500]) {
  const browser=await chromium.launch({headless:true,executablePath:'/Users/asuad/Library/Caches/ms-playwright/chromium-1243/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing'});
  try{const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(String(e)));await page.goto(`http://127.0.0.1:4198/core-bare?sleepUs=${sleepUs}`);await page.waitForFunction(()=>window.results?.result||window.results?.failure,{timeout:80000});const data=await page.evaluate(()=>window.results);results.push({sleepUs,...data,errors});await writeFile('agents/web-architecture-diagnosis/wasm-core-results.json',JSON.stringify(results,null,2));if(data.failure||errors.length)throw Error(JSON.stringify(data.failure||errors));}finally{await browser.close();}
 }
}finally{await server?.close();}
