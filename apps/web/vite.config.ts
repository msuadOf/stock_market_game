import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vite.dev/config/
//
// SharedArrayBuffer 支持（wasm-bindgen-rayon 多核需要）：
// 浏览器要求 Cross-Origin-Opener-Policy + Cross-Origin-Embedder-Policy 头
// 才会启用 SharedArrayBuffer。dev 和 preview 都必须设。
const crossOriginIsolationHeaders = {
  'Cross-Origin-Opener-Policy': 'same-origin',
  'Cross-Origin-Embedder-Policy': 'require-corp',
}

export default defineConfig({
  plugins: [react()],
  server: {
    headers: crossOriginIsolationHeaders,
  },
  preview: {
    headers: crossOriginIsolationHeaders,
  },
  // WASM 文件不压缩（wasm-pack 已优化）
  assetsInclude: ['**/*.wasm'],
  optimizeDeps: {
    // 排除 wasm-pkg（由 Worker 动态 import，不经 Vite 预打包）
    exclude: ['wasm-pkg'],
  },
})
