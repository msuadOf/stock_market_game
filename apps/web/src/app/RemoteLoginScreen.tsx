import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";
import type { SessionSetup } from "../types/engine.ts";
import { RemoteAuthClient, type RemoteLogin } from "../host/remote-auth.ts";
import type { RemoteMarketContext } from "../host/remote-market-context.ts";
import { IndexedDbCredentialStore } from "../auth/credential-store.ts";
import { parseSessionSeed } from "../config/seed-draft.ts";
import { parseCompanySystemConfig } from "../save/schema/company/system-config.ts";

interface RemoteLoginScreenProps {
  readonly baseUrl: string;
  readonly setup: SessionSetup;
  readonly seed: string;
  readonly companySystemDraft: string;
  readonly creationSettings?: ReactNode;
  readonly onConnected: (token: string, context: RemoteMarketContext) => void;
  readonly onBack: () => void;
}

export function RemoteLoginScreen({ baseUrl, setup, seed, companySystemDraft, creationSettings, onConnected, onBack }: RemoteLoginScreenProps) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [remember, setRemember] = useState(false);
  const [login, setLogin] = useState<RemoteLogin | null>(null);
  const [markets, setMarkets] = useState<RemoteMarketContext[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmedRejoin, setConfirmedRejoin] = useState<string | null>(null);
  const [confirmCreate, setConfirmCreate] = useState(false);
  const [confirmClear, setConfirmClear] = useState(false);
  const credentialStore = useRef<IndexedDbCredentialStore | null>(null);
  const epoch = useRef(0);
  const active = useRef(false);
  const restoreSavedLogin = useRef(restore);
  restoreSavedLogin.current = restore;

  useEffect(() => {
    epoch.current += 1;
    setLogin(null);
    setMarkets(null);
    setError(null);
    setBusy(false);
    setPassword("");
    setConfirmedRejoin(null);
    setConfirmCreate(false);
    setConfirmClear(false);
    active.current = false;
    void restoreSavedLogin.current(false);
    return () => { epoch.current += 1; credentialStore.current?.cancelPending(); };
  }, [baseUrl]);

  function storage(): IndexedDbCredentialStore {
    if (credentialStore.current === null) credentialStore.current = new IndexedDbCredentialStore();
    return credentialStore.current;
  }

  async function run(operation: (client: RemoteAuthClient, isCurrent: () => boolean) => Promise<void>) {
    if (active.current) return;
    active.current = true;
    setBusy(true);
    setError(null);
    const operationEpoch = epoch.current;
    const isCurrent = () => epoch.current === operationEpoch;
    try { await operation(new RemoteAuthClient(baseUrl), isCurrent); }
    catch (failure) {
      if (isCurrent()) setError(failure instanceof Error ? failure.message : "remote-login 发生未知错误，请反馈");
    } finally {
      if (isCurrent()) { active.current = false; setBusy(false); setPassword(""); }
    }
  }

  async function authenticate(kind: "login" | "register" | "guest") {
    await run(async (client, isCurrent) => {
      const authenticated = kind === "login" ? await client.login(username, password) : kind === "register" ? await client.register(username, password) : await client.guest();
      if (!isCurrent()) return;
      setLogin(authenticated);
      if (remember) {
        await storage().save({ ...authenticated, server: client.baseUrl }, isCurrent);
        if (!isCurrent()) return;
      }
      const available = await client.markets(authenticated.token);
      if (isCurrent()) setMarkets(available);
    });
  }

  function submit(event: FormEvent<HTMLFormElement>) { event.preventDefault(); void authenticate("login"); }

  function restore(required = true) {
    void run(async (client, isCurrent) => {
      const stored = await storage().load(client.baseUrl);
      if (!isCurrent()) return;
      if (stored === null) {
        if (required) throw new Error("此 Server 没有记住的登录；请明确选择账号登录或创建匿名身份");
        return;
      }
      const subject = await client.me(stored.token);
      if (!isCurrent()) return;
      if (subject.subject_id !== stored.subject.subject_id) throw new Error("已记住的 credential 与主体不一致，请明确清除并反馈 remote-login");
      setLogin({ subject, token: stored.token });
      setRemember(true);
      const available = await client.markets(stored.token);
      if (isCurrent()) setMarkets(available);
    });
  }

  function refresh() {
    if (login === null) return;
    void run(async (client, isCurrent) => {
      const available = await client.markets(login.token);
      if (isCurrent()) { setMarkets(available); setConfirmedRejoin(null); }
    });
  }

  function join(context: RemoteMarketContext) {
    if (login === null) return;
    void run(async (client, isCurrent) => {
      const joined = await client.join(login.token, context, confirmedRejoin === context.session_id);
      if (!isCurrent()) return;
      if (joined.member === null || joined.needs_rejoin) throw new Error("Server 未确认当前主体的资金账户，请刷新市场列表并反馈 remote-login");
      onConnected(login.token, joined);
    });
  }

  function create() {
    if (login === null || !confirmCreate) return;
    void run(async (client, isCurrent) => {
      parseSessionSeed(seed);
      const config = parseCompanySystemConfig(JSON.parse(companySystemDraft));
      const created = await client.create(login.token, { ...setup, company_system: config }, seed);
      if (!isCurrent()) return;
      if (created.member === null || created.needs_rejoin || !created.can_control) throw new Error("Server 未返回创建者的资金账户与市场控制能力，请刷新市场列表并反馈 remote-login");
      onConnected(login.token, created);
    });
  }

  function logout() {
    if (login === null) return;
    void run(async (client, isCurrent) => {
      await client.logout(login.token);
      if (!isCurrent()) return;
      setLogin(null);
      setMarkets(null);
      if (remember) await storage().remove(client.baseUrl, isCurrent);
    });
  }

  return <main className="app-error" aria-labelledby="remote-login-title">
    <h1 id="remote-login-title">远程 Server · 登录与选择市场</h1>
    <p>远程交易模拟和日终存档由 Server 负责；登录不发放资金，不加载或创建市场。</p>
    <p>HTTP 不提供传输保密，公网请使用 HTTPS。记住登录只把 Bearer credential 保存到此浏览器独立的 IndexedDB 登录库，按 Server 隔离，不保存密码或市场状态；同源脚本仍能访问，请勿在不可信设备使用。</p>
    {login === null ? <>
      <form onSubmit={submit}>
        <fieldset disabled={busy}>
          <legend>账号登录／注册</legend>
          <label>用户名<input autoComplete="username" value={username} onChange={(event) => setUsername(event.currentTarget.value)} maxLength={64} required /></label>
          <label>密码<input type="password" autoComplete="current-password" value={password} onChange={(event) => setPassword(event.currentTarget.value)} required /></label>
          <p>用户名：1—64 个 ASCII 字母、数字、下划线或短横线；密码：8—1024 个 UTF-8 字节。</p>
          <label><input type="checkbox" checked={remember} onChange={(event) => setRemember(event.currentTarget.checked)} />记住登录</label>
          <p>不勾选则此次 credential 仅留在内存；已有记住的 credential 需使用下方按钮显式清除。</p>
          <button type="submit">登录</button>
          <button type="button" onClick={() => void authenticate("register")}>注册并登录</button>
        </fieldset>
      </form>
      <p>匿名身份与账号一样拥有独立持仓。匿名 credential 丢失或退出后无法恢复该身份及持仓；不记住登录时关闭页面即丢失，创建新匿名身份不会恢复原资产。</p>
      <button type="button" disabled={busy} onClick={() => void authenticate("guest")}>明确创建匿名身份并登录</button>
      <button type="button" disabled={busy} onClick={() => restore()}>重新读取已记住的登录</button>
    </> : <>
      <p>已登录：{login.subject.username === null ? "匿名主体" : login.subject.username}。公开市场无需邀请码；加入已有市场不会为您暗建独立市场。</p>
      {login.subject.username === null && <p>匿名 credential 丢失或退出后无法恢复身份及持仓，请谨慎操作。</p>}
      <button type="button" disabled={busy} onClick={refresh}>刷新市场列表</button>
      {markets === null ? <p>市场列表尚未成功读取，请点击刷新；不会自动创建新市场。</p> : markets.length === 0 ? <>
        <p>Server 当前没有活动市场。创建会建立整个共享市场并授予创建者市场控制能力与本人交易账户，不是只创建个人视图；之后的设置、暂停、重置和加载存档会影响所有玩家。</p>
        {creationSettings}
        <label><input type="checkbox" disabled={busy} checked={confirmCreate} onChange={(event) => setConfirmCreate(event.currentTarget.checked)} />确认创建共享市场并理解全局影响</label>
        <button type="button" disabled={busy || !confirmCreate} onClick={create}>创建共享市场</button>
      </> : <ul>{markets.map((context) => <li key={context.session_id}>
        <p>市场 {context.session_id} · generation {context.generation} · {context.can_control ? "拥有市场控制能力" : "仅本人交易能力（加入后）"}</p>
        {context.needs_rejoin && <>
          <p>此身份在当前市场没有资金账户，可能首次加入或未出现在加载的日终存档中。必须本人确认加入／重新加入，才按当前市场设置发放一次初始资金；不会恢复旧余额或持仓。已有控制能力保持独立。</p>
          <label><input type="checkbox" disabled={busy} checked={confirmedRejoin === context.session_id} onChange={(event) => setConfirmedRejoin(event.currentTarget.checked ? context.session_id : null)} />确认重新加入此市场</label>
        </>}
        <button type="button" disabled={busy || (context.needs_rejoin && confirmedRejoin !== context.session_id)} onClick={() => join(context)}>{context.member !== null ? "进入已有账户" : context.needs_rejoin ? "确认重新加入并进入" : "加入公开市场"}</button>
        {context.can_control && context.member === null && <button type="button" disabled={busy} onClick={() => onConnected(login.token, context)}>仅进入公共市场／控制（不创建资金账户）</button>}
      </li>)}</ul>}
      <button type="button" disabled={busy} onClick={logout}>退出当前登录（撤销 credential，不删除持仓）</button>
    </>}
    <p>清除已记住的 credential 后，匿名身份在关闭页面时将无法恢复；此操作不撤销 Server 登录，也不删除市场持仓。</p>
    <label><input type="checkbox" disabled={busy} checked={confirmClear} onChange={(event) => setConfirmClear(event.currentTarget.checked)} />确认理解匿名身份无法恢复的风险并清除记住的凭据</label>
    <button type="button" disabled={busy || !confirmClear} onClick={() => void run(async (client, isCurrent) => { await storage().remove(client.baseUrl, isCurrent); if (isCurrent()) { setConfirmClear(false); setRemember(false); } })}>清除此 Server 已记住的 credential（不撤销 Server 登录）</button>
    {busy && <p role="status">正在联系 Server 或登录存储，请稍候。</p>}
    {error !== null && <pre role="alert" aria-live="assertive">{error}</pre>}
    <button type="button" onClick={() => { epoch.current += 1; credentialStore.current?.cancelPending(); onBack(); }}>返回启动选择（不撤销 Server 登录）</button>
  </main>;
}
