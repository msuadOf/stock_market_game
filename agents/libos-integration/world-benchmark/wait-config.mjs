export function parseFineWaitUs(raw) {
  if (raw === undefined) return 500;
  if (typeof raw !== 'string' || !/^[1-9][0-9]*$/.test(raw)) throw Error('fine wait 必须是正整数微秒');
  const value = Number(raw);
  if (!Number.isSafeInteger(value) || value > 1000000) throw Error('fine wait 超出诊断范围');
  return value;
}

export function initializeDiagnosticWait(glue, fineWaitUs) {
  glue.init_wasm_wait(500, 50000, fineWaitUs, 500);
}
