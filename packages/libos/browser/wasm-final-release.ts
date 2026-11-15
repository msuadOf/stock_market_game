/** released 只由真实终结器产生：全组终止、最后观测不可再 grow 的 Memory、清除全部强引用。 */
export type WasmFinalRelease = Readonly<{
  kind: "released";
  finalBytes: number;
  growthStopped: true;
  referencesDropped: true;
}> | Readonly<{ kind: "not-allocated" }>;
