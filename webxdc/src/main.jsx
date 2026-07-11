import { render } from "preact";
import { App } from "./App.jsx";
import "@xterm/xterm/css/xterm.css";
import "./style.css";

const root = document.getElementById("app");
if (root) {
  render(<App />, root);
}
