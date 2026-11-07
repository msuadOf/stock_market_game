import { createFacadeChildBootstrap } from '../../../apps/web/src/host/facade-child-bootstrap.ts';
import { bindFacadeThreadEnvironment, installFacadeSlotLayout, getArenaView, requireExitIntent } from '../../../apps/web/src/host/engine-clock-env.ts';
const bootstrap = createFacadeChildBootstrap({
  environment: {bindFacadeThreadEnvironment, installFacadeSlotLayout, getArenaView, requireExitIntent},
  async loadGlue() { return await import('./pkg/web_wasm.js'); },
  postControl(message, input) { input.controlPort.postMessage(message); },
  postValidationFailure(message) { self.postMessage(message); },
  reportError(error) { queueMicrotask(() => { throw error; }); },
  close() { self.close(); },
});
self.onmessage = event => { void bootstrap.handleMessage(event.data); };
