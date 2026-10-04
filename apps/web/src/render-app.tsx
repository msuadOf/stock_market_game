import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Provider } from "react-redux";
import { store } from "./store/store";
import App from "./App.tsx";
import { RenderErrorBoundary } from "./app/RenderErrorBoundary.tsx";

export function renderApp(root: HTMLElement): void {
  createRoot(root).render(
    <StrictMode>
      <RenderErrorBoundary>
        <Provider store={store}>
          <App />
        </Provider>
      </RenderErrorBoundary>
    </StrictMode>,
  );
}
