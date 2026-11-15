//! 从已提交的快速槽读取；授权文件副本不是快速槽的来源。
type QuickArchiveRead<T> = {
  readonly fence?: () => Promise<void>;
  readonly readAuthority?: () => Promise<unknown>;
  readonly readLegacy: () => Promise<T | null>;
  readonly validate: (slot: unknown) => T;
};

export async function readQuickArchive<T>(ports: QuickArchiveRead<T>): Promise<T | null> {
  await ports.fence?.();
  if (ports.readAuthority !== undefined) {
    const current = await ports.readAuthority();
    if (current !== null) return ports.validate(current);
  }
  return ports.readLegacy();
}
