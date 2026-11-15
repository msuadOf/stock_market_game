/** 可跨 Worker 传送的结构化错误，不解释任何业务错误码。 */
export type HostFailure = {
  readonly code: string;
  readonly where: string;
  readonly message: string;
  readonly cause?: unknown;
  readonly context?: unknown;
  readonly recoverable?: boolean | null;
  readonly recoveryActions?: readonly string[] | null;
};

export type ParseHostFailure = (value: unknown, transportWhere?: string) => HostFailure;
