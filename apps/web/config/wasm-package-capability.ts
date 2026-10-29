import { lstat } from 'node:fs/promises'
import { createRequire } from 'node:module'
import path from 'node:path'
import type { Plugin } from 'vite'
const require = createRequire(import.meta.url)
const configuredModule = process.env.STOCK_GAME_WASM_METADATA_MODULE
if (configuredModule !== undefined && !path.isAbsolute(configuredModule)) {
  throw new Error('STOCK_GAME_WASM_METADATA_MODULE must be an absolute module path')
}
const { WASM_PACKAGE_METADATA_FILENAMES, readWasmPackageMetadata } = require(configuredModule ?? '../../../scripts/wasm-package-capability.mjs') as {
  readonly WASM_PACKAGE_METADATA_FILENAMES: readonly string[]
  readWasmPackageMetadata(directory: string): Promise<{ provenanceBytes: Buffer; capabilityBytes: Buffer }>
}

export function wasmPackageCapabilityPlugin(): Plugin {
  let packageDirectory: string
  let basePath = '/'

  const loadVerifiedBytes = async () => {
    const metadata = await readWasmPackageMetadata(packageDirectory)
    return new Map([
      [WASM_PACKAGE_METADATA_FILENAMES[0], metadata.provenanceBytes],
      [WASM_PACKAGE_METADATA_FILENAMES[1], metadata.capabilityBytes],
    ])
  }

  return {
    name: 'wasm-package-capability-metadata',
    configResolved(resolved) {
      const configuredPackage = process.env.STOCK_GAME_WASM_PKG
      packageDirectory = path.resolve(configuredPackage ?? path.join(resolved.root, 'wasm-pkg'))
      basePath = new URL(resolved.base, 'http://vite.local').pathname
    },
    async generateBundle() {
      const files = await loadVerifiedBytes()
      for (const [fileName, source] of files) this.emitFile({ type: 'asset', fileName, source })
    },
    configureServer(server) {
      server.middlewares.use((request, response, next) => {
        let pathname: string
        try { pathname = new URL(request.url ?? '/', 'http://vite.local').pathname } catch (error) {
          const detail = error instanceof Error ? error.message : String(error)
          next(new Error(`WASM metadata request URL is malformed: ${detail}`, { cause: error }))
          return
        }
        const fileName = WASM_PACKAGE_METADATA_FILENAMES.find((name) => pathname === `${basePath}${name}`)
        if (fileName === undefined) { next(); return }
        void (async () => {
          const filename = path.join(packageDirectory, fileName)
          const stat = await lstat(filename)
          if (!stat.isFile() || stat.isSymbolicLink()) throw new Error(`WASM metadata must be a regular package file: ${filename}`)
          const files = await loadVerifiedBytes()
          const bytes = files.get(fileName)
          if (bytes === undefined) throw new Error(`WASM metadata allowlist lost ${fileName}`)
          response.statusCode = 200
          response.setHeader('Content-Type', 'application/json; charset=utf-8')
          response.setHeader('Cache-Control', 'no-cache')
          response.end(bytes)
        })().catch(next)
      })
    },
  }
}
