export async function queryPrivateHistory<Host extends object, Result>(readContext: () => { host: Host | null; generation: string | null; account: string | null }, query: (host: Host) => Promise<Result>): Promise<Result> {
  const context = readContext();
  if (context.host === null) throw new Error("游戏宿主未就绪，不能查询本人交割历史");
  if (context.account === null) throw new Error("当前市场没有本人资金账户，不能查询私有交割历史");
  if (context.generation === null) throw new Error("市场基线尚未就绪，不能查询本人交割历史");
  const assertCurrent = () => {
    const current = readContext();
    if (current.host !== context.host || current.generation !== context.generation || current.account !== context.account) throw new Error("本人交割历史响应属于已切换的宿主、市场或账户");
  };
  let result: Result;
  try { result = await query(context.host); }
  catch (error) { assertCurrent(); throw error; }
  assertCurrent();
  return result;
}
