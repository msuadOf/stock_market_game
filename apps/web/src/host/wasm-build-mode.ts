export function shouldLoadDiagnosticsWasm(devBuild: boolean, diagnosticsFlag: string | undefined): boolean {
  return devBuild && diagnosticsFlag === "1";
}
