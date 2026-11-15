// 浏览器快速槽的 writer lease；gzip 正文和原读取接口保持兼容。
import { OrderedIndexedDbWrite } from "../../../../packages/libos/browser/ordered-indexeddb-write.ts";
import { CompressedSaveRepository, gzipSaveCodec } from "./save-repository.ts";
import { QUICK_SAVE_KEY, SAVE_DATABASE, SAVE_OBJECT_STORE } from "./save-storage-keys.ts";

const LEASE_KEY = QUICK_SAVE_KEY + ":writer";
type WriterRecord = { readonly lease: string; readonly seq: string };

export function chooseCheckpointWrite(previous: unknown, lease: string, sequence: bigint, gzip: string, allowInitialClaim = false) {
  if (previous === undefined) {
    if (!allowInitialClaim) return null;
  } else {
    if (previous === null || typeof previous !== "object" || !("lease" in previous) || !("seq" in previous)
      || typeof previous.lease !== "string" || typeof previous.seq !== "string" || !/^(-1|0|[1-9][0-9]*)$/.test(previous.seq)) {
      throw new Error("快速存档的 writer lease 不可读");
    }
    if (previous.lease !== lease || sequence <= BigInt(previous.seq)) return null;
  }
  const next: WriterRecord = { lease, seq: sequence.toString() };
  return [[LEASE_KEY, next], [QUICK_SAVE_KEY, gzip]] as const;
}

export class WorkerCheckpointRepository {
  private readonly storage: OrderedIndexedDbWrite;
  private readonly factory: IDBFactory;
  private readonly lease: string;
  private allowInitialClaim = false;
  constructor(factory: IDBFactory, lease: string) {
    this.factory = factory;
    this.lease = lease;
    this.storage = new OrderedIndexedDbWrite(factory, SAVE_DATABASE, SAVE_OBJECT_STORE);
  }

  async activate(): Promise<void> {
    // 不存在的快速槽在首份实际存档中被原子认领，启动不写空库。
    this.allowInitialClaim = !(await this.factory.databases()).some(database => database.name === SAVE_DATABASE);
    if (this.allowInitialClaim) return;
    const value: WriterRecord = { lease: this.lease, seq: "-1" };
    await this.storage.change(LEASE_KEY, () => [[LEASE_KEY, value]]);
  }

  close(): Promise<void> { return this.storage.close(); }

  async write(sequence: bigint, envelope: unknown): Promise<boolean> {
    const codec = new CompressedSaveRepository({
      getItem: () => { throw new Error("归档副实例只写压缩快速槽"); },
      setItem: (_key, gzip) => this.storage.change(LEASE_KEY, previous => chooseCheckpointWrite(previous, this.lease, sequence, gzip, this.allowInitialClaim)),
    }, QUICK_SAVE_KEY, gzipSaveCodec);
    const applied = await codec.save(envelope);
    if (applied) this.allowInitialClaim = false;
    return applied;
  }
}
