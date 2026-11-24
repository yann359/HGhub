import "../src/index.css";
import "../src/ios.css";
import { createRoot } from "react-dom/client";
import { StrictMode } from "react";
import App from "../src/App";
import { AppProvider } from "../src/store";


// Replace only Tauri's IPC transport. Rust dispatcher, QuickJS, plugin network,
// validation, media sessions and the React page remain the production code.
const openedUrls: string[] = [];
const windowCommands: string[] = [];
Object.assign(window, { __HPP_TEST_WINDOW_COMMANDS__: windowCommands });
Object.assign(window, { __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} }, __HPP_TEST_OPENED_URLS__: openedUrls, __TAURI_INTERNALS__: { async invoke(cmd: string, args: unknown) {
  if (cmd === "pick_plugin_files") return (window as unknown as { __HPP_TEST_FILE_PATHS__?: string[] }).__HPP_TEST_FILE_PATHS__ || [];
  if (cmd === "open_url") { openedUrls.push((args as { url: string }).url); return; }
  if (cmd === "app_boot") return { strict_security: true };
  if (cmd === "set_orientation") {
    windowCommands.push(`orientation:${(args as { landscape: boolean }).landscape}`);
    return;
  }
  if (cmd === "security_set") return (args as { strict: boolean }).strict;
  if (cmd.startsWith("plugin:window|")) {
    windowCommands.push(cmd);
    if (cmd.endsWith("is_maximized") || cmd.endsWith("is_fullscreen")) return false;
    return;
  }
  if (cmd === "plugin:app|registerListener" || cmd === "plugin:app|remove_listener") return;
  if (cmd === "plugin:event|listen") return 1;
  if (cmd === "plugin:event|unlisten") return;
  if (cmd !== "hpp_manage") throw new Error(`Unexpected command ${cmd}`);
  const response = await fetch("/__hpp_test_ipc", { method: "POST", body: JSON.stringify(args) });
  const result = await response.json(); if (!result.ok) throw result.error; return result.value;
}, transformCallback() { return 1; }, unregisterCallback() {}, metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } } } });
createRoot(document.querySelector("#root")!).render(<StrictMode><AppProvider><App/></AppProvider></StrictMode>);
