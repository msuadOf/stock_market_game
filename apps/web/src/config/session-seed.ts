export type SeedEntropy = Pick<Crypto, "getRandomValues">;

export function createNewSessionSeed(entropy: SeedEntropy | null | undefined = globalThis.crypto): bigint {
  if (!entropy || typeof entropy.getRandomValues !== "function") {
    throw new Error("无法生成新局 seed：运行环境未提供 crypto.getRandomValues，请检查安全上下文与浏览器支持");
  }
  const words = new Uint32Array(2);
  try {
    entropy.getRandomValues(words);
  } catch (cause) {
    throw new Error(`无法生成新局 seed：crypto.getRandomValues 失败：${cause instanceof Error ? cause.message : String(cause)}`, { cause });
  }
  return (BigInt(words[0]!) << 32n) | BigInt(words[1]!);
}
