import { createEngineControlOwner } from '../../../apps/web/src/host/engine-control-owner.ts';
function owned(url: URL, name: string): Worker {
  const worker = new Worker(url,{type:'module',name});
  const terminate = worker.terminate.bind(worker);
  worker.terminate = () => { self.postMessage({type:'fixtureTermination', name}); terminate(); };
  worker.addEventListener('error', event => self.postMessage({type:'fixtureNativeError', name, message:event.message}));
  return worker;
}
createEngineControlOwner({
  createEngineWorker:name => owned(new URL('./owner-engine.ts',import.meta.url),name),
  createChildWorker:name => owned(new URL('./child.ts',import.meta.url),name),
  createIngressWorker() { throw new Error('fixture 不创建 ingress'); },
  createChannel:() => new MessageChannel(),
  postMessage:(value,transfer) => self.postMessage(value,{transfer}),
  addMessageListener:listener => self.addEventListener('message',listener),
  reportError:error => { const details = error instanceof AggregateError ? error.errors.map(item => String(item)) : [String(error)]; self.postMessage({type:'fixtureReporterError',details}); },
});
