// 从产品配置导出本次测量的实际 setup。
import {createRequire} from 'node:module';
import {writeFile} from 'node:fs/promises';
const require=createRequire(new URL('../../apps/web/package.json',import.meta.url));
const {createServer}=await import(require.resolve('vite'));
const server=await createServer({root:'apps/web',configFile:'apps/web/vite.config.ts',server:{middlewareMode:true}});
try {const {DEFAULT_SETUP}=await server.ssrLoadModule('/src/config/defaults.ts');await writeFile('agents/web-architecture-diagnosis/default-setup.json',JSON.stringify(DEFAULT_SETUP));} finally {await server.close();}
