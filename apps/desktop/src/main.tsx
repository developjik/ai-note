import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { installBridge } from "./tauriBridge";

installBridge(window);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
