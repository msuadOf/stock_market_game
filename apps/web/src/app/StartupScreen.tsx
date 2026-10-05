import type { FormEvent, ReactNode } from "react";
import type { StartupMode } from "../host/startup-policy.ts";

interface StartupScreenProps {
  readonly mode: StartupMode;
  readonly remoteAddress: string;
  readonly error: string | null;
  readonly developmentHint: boolean;
  readonly onModeChange: (mode: StartupMode) => void;
  readonly onAddressChange: (address: string) => void;
  readonly onStart: () => void;
  readonly creationSettings?: ReactNode;
}

export function StartupScreen({ mode, remoteAddress, error, developmentHint, onModeChange, onAddressChange, onStart, creationSettings }: StartupScreenProps) {
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    onStart();
  }

  return <main className="app-error" aria-labelledby="startup-title">
    <h1 id="startup-title">股票模拟游戏 · 启动选择</h1>
    <p>尚未连接游戏引擎。本地确认后才建立宿主并读取一次日终快速存档；远程先登录并选择公开共享市场，身份凭据与游戏存档独立。模式和地址不写入游戏存档。</p>
    <form noValidate onSubmit={submit}>
      <fieldset>
        <legend>引擎运行位置</legend>
        <label><input type="radio" name="startup-mode" value="local" checked={mode === "local"} onChange={() => onModeChange("local")} />本地</label>
        <label><input type="radio" name="startup-mode" value="remote" checked={mode === "remote"} onChange={() => onModeChange("remote")} />远程 Server</label>
      </fieldset>
      <p id="startup-help">本地：桌面使用原生引擎，浏览器使用 WASM 多线程 Worker。远程：使用填写的 HTTP(S) 地址，WebSocket 自动派生；跨源需 Server 允许访问，HTTPS 页面连接 HTTP Server 可能被浏览器阻止。</p>
      {mode === "remote" && <label>
        Server HTTP(S) 地址
        <input type="text" inputMode="url" autoComplete="off" spellCheck={false}
          value={remoteAddress} onChange={(event) => onAddressChange(event.currentTarget.value)}
          aria-describedby="startup-help" aria-invalid={error !== null} required />
      </label>}
      {developmentHint && <p>开发环境变量仅作为初值提示，尚未建立连接；仍需确认启动。</p>}
      {mode === "local" && creationSettings}
      {error !== null && <pre role="alert" aria-live="assertive">{error}</pre>}
      <p>启动后不支持无缝切换宿主；失败或取消加载可返回重新选择，不会再次读取快速槽。不会自动切换或忽略坏档。</p>
      <button type="submit">启动游戏</button>
    </form>
  </main>;
}
