import { render } from "@solidjs/web";
import { App } from "./app";
import { applyAppearance } from "./appearance";
import { loadChrome } from "./layout-sync";
import "./twm.css";

const chrome = loadChrome(globalThis.localStorage);
applyAppearance(document.documentElement, chrome.theme, chrome.mode);

const root = document.getElementById("root");
if (root) render(() => <App />, root);
