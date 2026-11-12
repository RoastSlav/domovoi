import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import "./styles/fonts.css";
import "./styles/global.css";
import { applyTheme, readTheme } from "./design/theme";

// The inline script in index.html already set the theme; this syncs the toolbar colour.
applyTheme(readTheme());

const root = document.getElementById("root");
if (!root) {
  throw new Error("missing #root element");
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
