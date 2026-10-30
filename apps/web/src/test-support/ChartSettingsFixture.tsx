import type { ComponentType, ReactNode } from "react";
import { Provider } from "react-redux";
import { configureStore } from "@reduxjs/toolkit";
import { chartSettingsReducer } from "../store/chart-settings-slice.ts";

/** Vite 编译 JSX fixture；Node 测试入口保留可直接执行的 .ts。 */
export function ChartSettingsFixture({ children }: { children?: ReactNode }) {
  return <Provider store={configureStore({ reducer: { chartSettings: chartSettingsReducer } })}>{children}</Provider>;
}

/** 为保持 .test.ts 可由 Node 原生执行，使用 Vite 编译的 JSX fixture 包装必需 children。 */
export function RequiredChildrenFixture<P extends { children?: ReactNode }>({ component: Component, props, children }: { component: ComponentType<P>; props: Omit<P, "children">; children?: ReactNode }) {
  return <Component {...(props as P)}>{children}</Component>;
}
