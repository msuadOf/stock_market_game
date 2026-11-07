import { defineConfig } from '../../../apps/web/node_modules/vite/dist/node/index.js';
import path from 'node:path';
export default defineConfig({
  root: process.cwd(),
  resolve: {alias: [{find: /^env$/, replacement: path.resolve('apps/web/src/host/engine-clock-env.ts')}]},
  server: {host:'127.0.0.1', headers:{'Cross-Origin-Opener-Policy':'same-origin','Cross-Origin-Embedder-Policy':'require-corp'}},
});
