export interface ObserverCut<T> {
  readonly ordinal: string;
  readonly value: T;
}

type MaybePromise<T> = T | Promise<T>;

export function observerOrdinal(value: unknown): string {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]{0,19})$/.test(value)
    || BigInt(value) > 18_446_744_073_709_551_615n) throw new Error("无效的观察流发布序号");
  return value;
}

export function canInstallObserverCut(ordinal: string, floor: string | null): boolean {
  const requested = BigInt(observerOrdinal(ordinal));
  return floor === null || requested >= BigInt(observerOrdinal(floor));
}

export function isNewObserverDelivery(ordinal: string, floor: string | null): boolean {
  const next = BigInt(observerOrdinal(ordinal));
  return floor === null || next > BigInt(observerOrdinal(floor));
}

export async function runObserverResync<T>(port: {
  assertCurrent(): void;
  hold(): MaybePromise<void>;
  capture(attempt: number): MaybePromise<ObserverCut<T>>;
  floor(): string | null;
  canUse?(cut: ObserverCut<T>): boolean;
  fence(cut: ObserverCut<T>): MaybePromise<boolean>;
  install(cut: ObserverCut<T>): MaybePromise<void>;
  release(): MaybePromise<void | boolean>;
}): Promise<void> {
  port.assertCurrent();
  await port.hold();
  for (let attempt = 0; attempt < 4; attempt++) {
    port.assertCurrent();
    const cut = await port.capture(attempt);
    port.assertCurrent();
    if (!canInstallObserverCut(cut.ordinal, port.floor()) || port.canUse?.(cut) === false) continue;
    if (!(await port.fence(cut))) continue;
    port.assertCurrent();
    await port.install(cut);
    port.assertCurrent();
    if ((await port.release()) !== false) return;
    port.assertCurrent();
    await port.hold();
  }
  throw new Error("观察流连续四次基线已过期，请重新连接观察者");
}
