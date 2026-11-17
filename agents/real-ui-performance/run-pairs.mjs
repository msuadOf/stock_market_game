// 真实前端批次：同一 CPU 亲和集内串行，逐个独立数据库与浏览器。
import {spawn,execFileSync} from 'node:child_process';
import {readFile,readdir,mkdir,open,writeFile} from 'node:fs/promises';
import {terminateTree} from '../../scripts/run-with-deadline.mjs';
const root=process.cwd(),out=root+'/agents/real-ui-performance/run-'+Date.now();await mkdir(out);
const web=root+'/apps/web/dist';
await writeFile(out+'/environment.json',JSON.stringify({date:new Date().toISOString(),source:execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim(),affinity:execFileSync('taskset',['-pc',String(process.pid)],{encoding:'utf8'}),browser:execFileSync(process.env.BENCH_CHROMIUM || 'chromium',['--version'],{encoding:'utf8'}),cpuinfo:await readFile('/proc/cpuinfo','utf8'),web},null,2));
console.log('原始输出目录',out);
for(let pair=Number(process.env.BENCH_FIRST_PAIR||1);pair<=Number(process.env.BENCH_LAST_PAIR||3);pair++)for(const target of pair%2?['native','wasm']:['wasm','native']){
 const port=Number(process.env.BENCH_BASE_PORT||43240)+pair*2+(target==='wasm'?1:0);
 const baseUrl='http://127.0.0.1:'+port;
 const base=out+'/'+pair+'-'+target;console.log('开始',pair,target);
 const log=await open(base+'-server.log','w');
 const server=spawn(root+'/target/release/server',['--services','all','--web-root',web,'--bind','127.0.0.1:'+port,'--database',base+'.sqlite','--new-market'],{stdio:['ignore',log.fd,log.fd]});
 let child,serverError;server.on('error',e=>{serverError=e;});const usage=[];let samplePending=Promise.resolve();
 async function sample(){const rows=[];for(const pid of (await readdir('/proc')).filter(s=>/^\d+$/.test(s))){try{const stat=await readFile('/proc/'+pid+'/stat','utf8');const i=stat.lastIndexOf(')'),p=stat.slice(i+2).split(' ');rows.push({pid:Number(pid),comm:stat.slice(stat.indexOf('(')+1,i),ppid:Number(p[1]),utime:Number(p[11]),stime:Number(p[12]),threads:Number(p[17]),rssPages:Number(p[21])});}catch(e){if(e.code!=='ENOENT'&&e.code!=='ESRCH')throw e;}}usage.push({at:Date.now(),rows});}
 let sampler;
 try{
  let ready=false;for(let i=0;i<100;i++){if(serverError)throw serverError;try{if((await fetch(baseUrl)).ok){ready=true;break;}}catch(e){if(i===99)throw e;}await new Promise(r=>setTimeout(r,100));}if(!ready)throw Error('服务未就绪');
  sampler=setInterval(()=>{samplePending=samplePending.then(sample).catch(e=>console.error('进程采样失败',e));},500);
  await new Promise((resolve,reject)=>{child=spawn(process.execPath,[root+'/agents/real-ui-performance/measure-ui.mjs',target,base],{stdio:'inherit',env:{...process.env,BENCH_PORT:String(port)}});child.on('error',reject);child.on('exit',c=>c===0?resolve():reject(Error('probe exit '+c)));});
 }finally{if(sampler)clearInterval(sampler);await samplePending;const failures=[];for(const owned of [child,server]){if(owned&&owned.exitCode===null&&owned.signalCode===null){try{await terminateTree(owned);}catch(e){failures.push(e);}}}try{await log.close();}catch(e){failures.push(e);}await writeFile(base+'-processes.json',JSON.stringify(usage));if(failures.length)throw new AggregateError(failures,'清理自有进程失败');}
}
console.log('批次完成',out);
