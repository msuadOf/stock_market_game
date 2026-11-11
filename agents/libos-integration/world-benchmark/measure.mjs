export function validateBusinessHash(businessHash) {
  if (typeof businessHash !== 'string' || !/^(0|[1-9][0-9]*)$/.test(businessHash)
      || BigInt(businessHash) > 18446744073709551615n) {
    throw Error('业务状态摘要必须是精确的 u64 十进制字符串');
  }
  return businessHash;
}

export function measureGame(game, now, hooks = {}) {
  const warmStart = now();
  const warm = Object.fromEntries(game.diagnostic_game_steps(1000));
  const warmMs = now() - warmStart;
  hooks.onSampleStart?.();
  const start = now();
  const sample = Object.fromEntries(game.diagnostic_game_steps(3000));
  const elapsedMs = now() - start;
  hooks.onSampleEnd?.();
  const businessHash = validateBusinessHash(game.diagnostic_game_business_hash());
  return {
    measurementPath:'core-diagnostic',
    warm,
    warmMs,
    warmupTicks:1000,
    sample,
    elapsedMs,
    measuredTicks:3000,
    timingBoundary:'diagnostic_game_steps(3000) 调用前至返回后；不包含创建、恢复、UI、存档或 shutdown',
    businessHash,
  };
}
