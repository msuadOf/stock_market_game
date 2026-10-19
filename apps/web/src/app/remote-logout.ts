interface LogoutPorts {
  revoke(): Promise<void>;
  forget(): Promise<void>;
  disconnect(): Promise<void>;
}

export async function logoutRemoteIdentity(ports: LogoutPorts): Promise<void> {
  await ports.revoke();
  let storageFailure: unknown;
  try { await ports.forget(); } catch (failure) { storageFailure = failure; }
  try { await ports.disconnect(); } catch (failure) {
    if (storageFailure !== undefined) throw new AggregateError([storageFailure, failure], "Server 登录已撤销，但登录存储清理与断开连接均失败");
    throw failure;
  }
  if (storageFailure !== undefined) throw new Error(`已退出 Server 登录并断开连接，但登录存储清理失败：${storageFailure instanceof Error ? storageFailure.message : String(storageFailure)}；请检查浏览器 IndexedDB 后清除失效 credential。`);
}
