import { createEngineControlOwner } from '../../../apps/web/src/host/engine-control-owner.ts';
const observations = new BroadcastChannel('wasm-full-host-fixture');
function owned(url: URL, name: string): Worker {
  const worker = new Worker(url, { type: 'module', name });
  const terminate = worker.terminate.bind(worker);
  worker.terminate = () => { observations.postMessage({ type: 'termination', name }); terminate(); };
  worker.addEventListener('error', event => observations.postMessage({ type: 'nativeError', name, message: event.message }));
  return worker;
}
createEngineControlOwner({
  createEngineWorker: name => owned(new URL('./full-host-engine.ts', import.meta.url), name),
  createChildWorker: name => owned(new URL('./child.ts', import.meta.url), name),
  createIngressWorker() { throw new Error('fixture 不创建 ingress'); },
  createChannel: () => new MessageChannel(),
  postMessage(value, transfer) { observations.postMessage(value); self.postMessage(value, { transfer }); },
  addMessageListener: listener => self.addEventListener('message', listener),
  reportError(error) { self.reportError(error); },
});
