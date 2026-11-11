import { lstat, readFile } from 'node:fs/promises'
import { realpathSync } from 'node:fs'
import { createHash } from 'node:crypto'
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
  readWasmPackageMetadata(directory: string): Promise<{ provenanceBytes: Buffer; capabilityBytes: Buffer; provenance: { wasmSha256: string; glueSha256: string } }>
}

export function isExpectedGlueModule(id: string, packageDirectory: string): boolean {
  const normalized = (value: string) => value.replace(/\\/g, '/')
  return normalized(id.split('?', 1)[0]) === `${normalized(packageDirectory).replace(/\/$/, '')}/web_wasm.js`
}

export function wasmPackageGlueCapturePlugin(consumedGlueHashes: Set<string>): Plugin {
  let packageDirectory: string
  return {
    name: 'wasm-package-glue-capture',
    configResolved(resolved) { packageDirectory = realpathSync(path.resolve(resolved.root, 'wasm-pkg')) },
    async load(id) {
      if (!isExpectedGlueModule(id, packageDirectory)) return null
      const bytes = await readFile(path.join(packageDirectory, 'web_wasm.js'))
      consumedGlueHashes.add(createHash('sha256').update(bytes).digest('hex'))
      return bytes.toString('utf8')
    },
  }
}

export function wasmPackageCapabilityPlugin(consumedGlueHashes = new Set<string>()): Plugin {
  let packageDirectory: string
  let basePath = '/'
  const digest = (bytes: Uint8Array) => createHash('sha256').update(bytes).digest('hex')

  const loadVerifiedBytes = async () => {
    const metadata = await readWasmPackageMetadata(packageDirectory)
    const files = new Map([
      [WASM_PACKAGE_METADATA_FILENAMES[0], metadata.provenanceBytes],
      [WASM_PACKAGE_METADATA_FILENAMES[1], metadata.capabilityBytes],
    ])
    return { files, provenance: metadata.provenance }
  }

  return {
    name: 'wasm-package-capability-metadata',
    configResolved(resolved) {
      const configuredPackage = process.env.STOCK_GAME_WASM_PKG
      const importedPackage = path.resolve(resolved.root, 'wasm-pkg')
      if (configuredPackage !== undefined && path.resolve(configuredPackage) !== importedPackage) {
        throw new Error('WASM metadata package must match the actual import package')
      }
      packageDirectory = realpathSync(importedPackage)
      basePath = new URL(resolved.base, 'http://vite.local').pathname
    },
    buildStart() { consumedGlueHashes.clear() },
    async load(id) {
      if (!isExpectedGlueModule(id, packageDirectory)) return null
      const bytes = await readFile(path.join(packageDirectory, 'web_wasm.js'))
      consumedGlueHashes.add(digest(bytes))
      return bytes.toString('utf8')
    },
    async generateBundle(_options, bundle) {
      const { files, provenance } = await loadVerifiedBytes()
      if (consumedGlueHashes.size !== 1 || !consumedGlueHashes.has(provenance.glueSha256)) throw new Error('consumed glue SHA-256 does not match package metadata')
      const wasmAssets = Object.values(bundle).filter((entry) => entry.type === 'asset' && /(?:^|\/)web_wasm_bg(?:-[^/]*)?\.wasm$/.test(entry.fileName))
      if (wasmAssets.length !== 1) throw new Error(`bundle must contain exactly one game WASM asset; found ${wasmAssets.length}`)
      const wasm = wasmAssets[0]
      if (wasm.type !== 'asset' || typeof wasm.source === 'string' || digest(wasm.source) !== provenance.wasmSha256) throw new Error('bundle WASM SHA-256 does not match package metadata')
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
          const { files } = await loadVerifiedBytes()
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
