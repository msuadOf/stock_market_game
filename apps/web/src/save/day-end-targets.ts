export type DayEndTarget = {
  readonly label: string;
  readonly write: (slot: unknown, isCurrent: () => boolean) => Promise<void | boolean>;
};

export async function writeDayEndTargets(slot: unknown, isCurrent: () => boolean, targets: readonly DayEndTarget[]): Promise<boolean> {
  if (targets.length === 0) throw new Error("没有日终输出目标");
  const results = await Promise.allSettled(targets.map((target) => Promise.resolve().then(() => target.write(slot, isCurrent))));
  const errors: unknown[] = [];
  let committed = false;
  const status = results.map((result, index) => {
    const label = targets[index].label;
    if (result.status === "fulfilled") {
      if (result.value === false) return `${label}已取消（旧会话或目标）`;
      committed = true;
      return `${label}已更新`;
    }
    errors.push(result.reason);
    return `${label}更新失败：${result.reason instanceof Error ? result.reason.message : String(result.reason)}`;
  });
  if (errors.length !== 0) throw new AggregateError(errors, status.join("；"), { cause: errors[0] });
  return committed;
}
