// The phone window's entry (phone.html, ADR-0023 section 4). A separate page
// so the Twilio Voice SDK is bundled here only, never into the main window.
import { applyTheme, readTheme } from "@openbark/ui";
import "../app.css";
import "./phone.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { loadSettings } from "../context";
import { PhoneApp } from "./PhoneApp";

applyTheme(readTheme());
document.documentElement.lang = loadSettings().uiLanguage;

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <PhoneApp />
  </StrictMode>,
);
