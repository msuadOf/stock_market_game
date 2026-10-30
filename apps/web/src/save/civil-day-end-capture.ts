/** 日终候选先完成后才允许刷新其派生的活动委托视图。 */
export function captureCivilDayEndCandidate<T>(
  candidate: Promise<T>,
  isCurrent: () => boolean,
  onCaptured: () => void,
): Promise<T> {
  return candidate.then((value) => {
    if (isCurrent()) onCaptured();
    return value;
  });
}
