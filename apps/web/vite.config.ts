import { defineConfig, type PluginOption } from 'vite'
import react from '@vitejs/plugin-react'
import { createRequire } from 'node:module'

// Rolldown 无法解析 Popper 的 ESM 星号转导出；使用其官方 main 入口，避免修改依赖文件。
const blueprintRequire = createRequire(createRequire(import.meta.url).resolve('@blueprintjs/core'))

// 批 9 A10：COOP/COEP 头插件随 wasm-bindgen-rayon 多核退役删除
//（串行 wasm 不需要 SharedArrayBuffer 跨源隔离）。

export default defineConfig({
  plugins: [react()],
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
