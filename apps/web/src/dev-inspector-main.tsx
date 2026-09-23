import { createRoot } from "react-dom/client";
import "./index.css";

if (!import.meta.env.DEV) {
  throw new Error("NPC 决策诊断仅可在开发构建中运行");
}

const { NpcDecisionInspector } = await import("./dev/NpcDecisionInspector.tsx");

createRoot(document.getElementById("root") ?? document.body).render(<NpcDecisionInspector />);
