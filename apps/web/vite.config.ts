import { defineConfig, type PluginOption } from 'vite'
import react from '@vitejs/plugin-react'

// SharedArrayBuffer 支持（wasm-bindgen-rayon 多核需要）：
// COOP/COEP 必须在**每个**响应头（含 HTML 入口）。
// Vite 的 server.headers 对 HTML transform 响应不生效 → 用 plugin 注入。
function crossOriginIsolation(): PluginOption {
  return {
    name: 'cross-origin-isolation',
    configureServer(server) {
      server.middlewares.use((_req, res, next) => {
        res.setHeader('Cross-Origin-Opener-Policy', 'same-origin')
        res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp')
        next()
      })
    },
    configurePreviewServer(server) {
      server.middlewares.use((_req, res, next) => {
        res.setHeader('Cross-Origin-Opener-Policy', 'same-origin')
        res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp')
        next()
      })
    },
  }
}

export default defineConfig({
  plugins: [react(), crossOriginIsolation()],
  assetsInclude: ['**/*.wasm'],
  optimizeDeps: {
    exclude: ['wasm-pkg'],
  },
})
