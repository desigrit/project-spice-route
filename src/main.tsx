import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles.css";
import "./interface.css";
import "./macos.css";
import "./workspace.css";

const isMac = /Macintosh|Mac OS X/.test(navigator.userAgent);
document.documentElement.dataset.platform = isMac ? "macos" : "windows";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
