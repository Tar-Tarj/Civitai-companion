import "./styles.css";
import { backend } from "./bridge";
import { CompanionApp } from "./app";
import { preloadSounds } from "./audio";

async function main(): Promise<void> {
  const root = document.querySelector<HTMLElement>("#app");
  if (!root) throw new Error("Application root is missing");

  document.addEventListener("dragover", (event) => event.preventDefault());
  document.addEventListener("drop", (event) => event.preventDefault());
  preloadSounds();

  try {
    const snapshot = await backend.snapshot();
    await new CompanionApp(root, snapshot).start();
  } catch {
    const message = document.createElement("div");
    message.className = "fatal-error";
    message.textContent = "Civitai Companion could not initialize. Restart the app and try again.";
    root.replaceChildren(message);
  }
}

void main();
