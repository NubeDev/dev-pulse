import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import "./globals.css";
import { App } from "./app.js";

const root = document.getElementById("root");
if (!root) throw new Error("missing #root in index.html");

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
