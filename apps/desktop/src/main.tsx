import { applyTheme, readTheme } from "@openbark/ui";
import "./app.css";
import "./redesign.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";

applyTheme(readTheme());

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
