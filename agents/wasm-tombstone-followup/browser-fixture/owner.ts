const result = {events: [] as unknown[]};
Object.assign(window,{result});
const worker = new Worker(new URL('./owner-worker.ts',import.meta.url),{type:'module'});
const data = new MessageChannel();
worker.onmessage = ({data:message}) => {
  result.events.push(message);
  if (message.type === 'failurePending') worker.postMessage({type:'beginShutdown',instanceId:1,deadline:performance.timeOrigin + performance.now() + 300});
  if (message.type === 'terminal') { worker.terminate(); data.port1.close(); result.events.push({type:'fixtureOwnerTerminated'}); }
};
worker.onerror = event => result.events.push({type:'fixtureOwnerError',message:event.message});
worker.postMessage({type:'start',instanceId:1,runtimeOverride:true,dataPort:data.port2},[data.port2]);
