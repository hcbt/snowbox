import { render } from "@solidjs/web";
import { App } from "./app";
import "./twm.css";

const root = document.getElementById("root");
if (root) render(() => <App />, root);
