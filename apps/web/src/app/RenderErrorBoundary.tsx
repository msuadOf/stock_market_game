import { Component, type ReactNode } from "react";
import type { HostFailure } from "../host/host-update.ts";
import { FatalHostError } from "./HostStatusViews.tsx";

type RenderErrorState = { failure: HostFailure | null };

function renderFailureMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error === null || typeof error !== "object") return "React 界面出现非 Error 异常，请复制详情反馈。";
  try {
    const descriptor = Object.getOwnPropertyDescriptor(error, "message");
    if (descriptor === undefined) return "React 界面出现非 Error 异常或异常没有自身 message 字段，请复制详情反馈。";
    if (!Object.hasOwn(descriptor, "value")) return "React 界面异常的 message 是访问器，未执行读取，请复制详情反馈。";
    return typeof descriptor.value === "string" ? descriptor.value : "React 界面异常的 message 不是字符串，请复制详情反馈。";
  } catch {
    return "React 界面异常的 message 描述符读取失败，请复制详情反馈。";
  }
}

export class RenderErrorBoundary extends Component<{ children: ReactNode }, RenderErrorState> {
  state: RenderErrorState = { failure: null };

  static getDerivedStateFromError(error: unknown): RenderErrorState {
    return { failure: {
      code: "UI_RENDER_FAILED",
      where: "render-app.React",
      message: renderFailureMessage(error),
      cause: error,
      recoverable: false,
      recoveryActions: ["复制错误反馈并附上操作步骤", "必要时刷新，从最近日终档重新开始"],
    } };
  }

  render() {
    return this.state.failure === null ? this.props.children : <FatalHostError
      error={this.state.failure}
      title="界面无法继续显示"
      description="React 界面渲染或布局失败。请复制真实错误详情反馈；刷新不保证解决原因，且会丢失未保存的日内进度。"
      onRetry={() => window.location.reload()}
    />;
  }
}
