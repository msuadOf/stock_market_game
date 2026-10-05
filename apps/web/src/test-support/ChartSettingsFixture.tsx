import type { ReactNode } from "react";
import { Provider } from "react-redux";
import { configureStore } from "@reduxjs/toolkit";
import { chartSettingsReducer } from "../store/chart-settings-slice.ts";

/** Vite 编译 JSX fixture；Node 测试入口保留可直接执行的 .ts。 */
export function ChartSettingsFixture({ children }: { children?: ReactNode }) {
  return <Provider store={configureStore({ reducer: { chartSettings: chartSettingsReducer } })}>{children}</Provider>;
}
