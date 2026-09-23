import { parseSaveSlot } from "./save-schema.ts";
import type { StrictSaveEnvelope } from "./schema/root.ts";
import { parseSaveJson } from "./save-schema.ts";

interface KeyValueStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export interface SaveCompressionCodec {
  encode(text: string): Promise<string>
  decode(text: string): Promise<string>
}

export interface SaveRepository {
  save(slot: unknown): void;
  load(): StrictSaveEnvelope | null;
}

export class LocalStorageSaveRepository implements SaveRepository {
  private readonly storage: KeyValueStorage;
  private readonly key: string;

  constructor(storage: KeyValueStorage, key = "stock-game-save") {
    this.storage = storage;
    this.key = key;
  }

  save(slot: unknown): void {
    try {
      this.storage.setItem(this.key, JSON.stringify(parseSaveSlot(slot)));
    } catch (error) {
      throw new Error(`写入浏览器存档失败：${error instanceof Error ? error.message : String(error)}`);
    }
  }

  load(): StrictSaveEnvelope | null {
    let raw: string | null;
    try {
      raw = this.storage.getItem(this.key);
    } catch (error) {
      throw new Error(`读取浏览器存档失败：${error instanceof Error ? error.message : String(error)}`);
    }
    return raw === null ? null : parseSaveJson(raw);
  }
}

async function readBytes(stream: ReadableStream<Uint8Array>): Promise<Uint8Array> {
  const reader = stream.getReader()
  const chunks: Uint8Array[] = []
  let size = 0
  while (true) {
    const result = await reader.read()
    if (result.done) break
    chunks.push(result.value)
    size += result.value.length
  }
  const joined = new Uint8Array(size)
  let offset = 0
  for (const chunk of chunks) {
    joined.set(chunk, offset)
    offset += chunk.length
  }
  return joined
}

function toBase64(bytes: Uint8Array): string {
  let binary = ""
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return btoa(binary)
}

function fromBase64(value: string): Uint8Array {
  const binary = atob(value)
  return Uint8Array.from(binary, (character) => character.charCodeAt(0))
}

export const gzipSaveCodec: SaveCompressionCodec = {
  async encode(text) {
    const input = new Blob([text]).stream().pipeThrough(new CompressionStream("gzip"))
    return toBase64(await readBytes(input))
  },
  async decode(text) {
    const bytes = fromBase64(text)
    const copy = new Uint8Array(bytes.byteLength)
    copy.set(bytes)
    const input = new Blob([copy]).stream().pipeThrough(new DecompressionStream("gzip"))
    return new TextDecoder().decode(await readBytes(input))
  },
}

export class CompressedLocalStorageSaveRepository {
  private readonly storage: KeyValueStorage
  private readonly key: string
  private readonly codec: SaveCompressionCodec

  constructor(storage: KeyValueStorage, key = "stock-game-save", codec = gzipSaveCodec) {
    this.storage = storage
    this.key = key
    this.codec = codec
  }

  async save(slot: unknown): Promise<void> {
    try {
      const compressed = await this.codec.encode(JSON.stringify(parseSaveSlot(slot)))
      this.storage.setItem(this.key, `gzip:${compressed}`)
    } catch (error) {
      throw new Error(`写入浏览器存档失败：${error instanceof Error ? error.message : String(error)}`)
    }
  }

  async load(): Promise<StrictSaveEnvelope | null> {
    let raw: string | null
    try {
      raw = this.storage.getItem(this.key)
    } catch (error) {
      throw new Error(`读取浏览器存档失败：${error instanceof Error ? error.message : String(error)}`)
    }
    if (raw === null) return null
    const json = raw.startsWith("gzip:") ? await this.codec.decode(raw.slice("gzip:".length)) : raw
    return parseSaveJson(json)
  }
}
