import { defineConfig, type PluginOption } from 'vite'
import react from '@vitejs/plugin-react'
import { createRequire } from 'node:module'

// Rolldown 无法解析 Popper 的 ESM 星号转导出；使用其官方 main 入口，避免修改依赖文件。
const blueprintRequire = createRequire(createRequire(import.meta.url).resolve('@blueprintjs/core'))

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
  resolve: {
    alias: [{ find: /^@popperjs\/core$/, replacement: blueprintRequire.resolve('@popperjs/core') }],
  },
  build: {
    // AG Grid 的不可再分核心模块约 532 kB（gzip 约 147 kB）；其余依赖均按组拆分。
    chunkSizeWarningLimit: 550,
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            {
              name: 'grid-vendor',
              test: /node_modules[\\/](?:ag-grid-community|ag-grid-react)[\\/]/,
              priority: 30,
              maxSize: 450_000,
            },
            {
              name: 'blueprint-vendor',
              test: /node_modules[\\/]@blueprintjs[\\/]/,
              priority: 25,
              maxSize: 450_000,
            },
            {
              name: 'chart-vendor',
              test: /node_modules[\\/]lightweight-charts[\\/]/,
              priority: 20,
            },
            {
              name: 'react-vendor',
              test: /node_modules[\\/](?:react|react-dom|react-redux|@reduxjs)[\\/]/,
              priority: 15,
            },
            {
              name: 'vendor',
              test: /node_modules[\\/]/,
              priority: 10,
              maxSize: 450_000,
            },
          ],
        },
      },
    },
  },
  optimizeDeps: {
    exclude: ['wasm-pkg'],
  },
})
