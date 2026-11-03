import { Button } from "@blueprintjs/core";

/** 真实运行中的返回控件；独立模块避免仅测该控件时装载整个游戏模块图。 */
export function ReturnToStartupControl({ onSelectHost, stopSession, error }: { onSelectHost: (stopSession: () => Promise<void>) => Promise<void>; stopSession: () => Promise<void>; error: string | null }) {
  return <section aria-label="返回启动选择">
    <p>返回启动选择将结束当前内存会话，未保存的日内进度会丢失；不会重读快速槽，也不会无缝迁移。</p>
    <Button onClick={() => { void onSelectHost(stopSession); }}>返回启动选择</Button>
    {error !== null && <p role="alert">{error}</p>}
  </section>;
}
