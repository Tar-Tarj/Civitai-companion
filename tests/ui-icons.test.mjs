import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const css = await readFile(new URL("../src/styles.css", import.meta.url), "utf8");
const app = await readFile(new URL("../src/app.ts", import.meta.url), "utf8");

test("notification Buzz bolts use same icon boxes as neighboring notification icons", () => {
  assert.match(css, /\.filter-button \.icon-bolt \{ width: 17px; height: 17px; \}/);
  assert.match(css, /\.notification-icon\.icon-bolt \{ width: 21px; height: 21px;/);
});

test("compact Buzz counter and transaction tab use their intended bolt sizes", () => {
  assert.match(css, /\.buzz-line \.icon-bolt \{ width: 10px; height: 14px; \}/);
  assert.match(css, /\.buzz-account-tab \.icon-bolt \{ width: 12px; height: 17px; \}/);
});

test("requested notification icon colors and external tip outline are preserved", () => {
  assert.match(css, /\.notification-icon\.icon-collection \{ color: #99C218; \}/);
  assert.match(css, /\.notification-icon\.icon-comment \{ color: #BDBDBD; \}/);
  assert.match(css, /\.thumbnail-frame\.tip-thumbnail \{ outline: 2px solid #f59f00; outline-offset: 2px; \}/);
  assert.match(app, /kind === "collection-update"\) return "icon-collection"/);
  assert.match(app, /kind\.endsWith\("-pending"\) \|\| kind\.endsWith\("-resolved"\)/);
  assert.match(app, /label: "Miscellaneous"/);
});

test("custom titlebar and account summary match the compact mockup behavior", () => {
  assert.match(app, /renderTitleBar/);
  assert.match(app, /data-tauri-drag-region/);
  assert.match(app, /appIcon/);
  assert.match(app, /button\(`profile-avatar \$\{this\.syncIndicatorClass\(\)\}`/);
  assert.match(app, /"profile-name-link"/);
  assert.match(app, /"Go to profile →"/);
  assert.match(app, /element\("div", "stat-card follower-card"\)/);
  assert.match(app, /"buzz-separator"/);
  assert.match(css, /\.profile-avatar\.ready \{ outline-color: #4ade80; \}/);
  assert.match(css, /\.profile-avatar\.pending \{ outline-color: #facc15; animation: profile-sync-pulse/);
  assert.match(css, /\.profile-avatar\.failed \{ outline-color: #f87171; \}/);
  assert.match(css, /\.shell \{[\s\S]*border: 1px solid #34343a;[\s\S]*border-radius: 8px;/);
  assert.match(css, /\.profile-copy \{ grid-column: 2; grid-row: 1; width: 100%; min-width: 0;/);
  assert.match(css, /\.profile-name-link \{ position: relative; width: 100%;/);
  assert.match(css, /\.follower-card \{[^}]*flex-direction: row;[^}]*white-space: nowrap;/);
  assert.match(css, /\.profile-card, \.compact-stats \{ display: contents; \}/);
});

test("About uses the app icon and linked creator profile", () => {
  assert.match(app, /element\("img", "about-mark"\)/);
  assert.match(app, /https:\/\/civitai\.red\/user\/Tar_Tarj/);
  assert.match(app, /this\.snapshot\.account\.profileImageUrl/);
  assert.match(css, /\.about-credit-avatar \{/);
});

test("status toasts are removed and confirmations stay inside the themed UI", () => {
  assert.doesNotMatch(app, /window\.confirm/);
  assert.doesNotMatch(app, /"Synchronized"/);
  assert.doesNotMatch(css, /\.toast/);
  assert.match(app, /element\("dialog", "app-dialog"\)/);
  assert.match(css, /\.app-dialog \{ width: min\(360px, calc\(100vw - 28px\)\)/);
});

test("preference export confirms the exact backend path in the themed dialog", () => {
  assert.match(app, /const path = await backend\.exportPreferences\(\)/);
  assert.match(app, /this\.dialog\("Preferences exported", `Saved to:\\n\$\{path\}`, "OK"\)/);
  assert.doesNotMatch(app, /URL\.createObjectURL/);
  assert.match(css, /\.app-dialog p \{[^}]*white-space: pre-wrap;/);
});

test("header keeps one avatar size, compact pills and a shorter titlebar at every width", () => {
  assert.match(css, /\.titlebar \{\n  height: 36px;\n  flex: 0 0 36px;/);
  assert.match(css, /\.profile-avatar \{ position: relative; width: 54px; height: 54px;/);
  assert.match(css, /\.account-summary \{ display: grid; grid-template-columns: 54px minmax\(0, 1fr\) max-content; grid-template-rows: auto auto;/);
  assert.match(css, /\.profile-avatar \{ grid-column: 1; grid-row: 1 \/ 3; \}/);
  assert.match(css, /\.buzz-stack \{ --buzz-font: 13px; grid-column: 2 \/ 4; grid-row: 2; justify-self: end;/);
  assert.match(css, /\.buzz-stack \{ --buzz-font: 13px; grid-column: 2 \/ 4; grid-row: 2; justify-self: end; width: max-content; max-width: none; flex: 0 0 auto; height: 28px;/);
  assert.match(css, /\.follower-card \{ grid-column: 3; grid-row: 1; width: max-content; height: 28px;/);
  assert.doesNotMatch(css, /@media \(max-width: 455px\)/);
  assert.match(app, /formatCompactBuzz\(balance\.value\)/);
  assert.match(app, /buzz\.title = buzzBalanceTooltip\(balances\)/);
  assert.match(app, /view\.append\(this\.renderBuzzBalance\(\)\)/);
  assert.match(css, /\.buzz-balance \{ display: flex;/);
});

test("header Buzz numbers are never truncated: the pill keeps natural width and fits by font size", () => {
  assert.match(css, /\.buzz-line \{ flex: 0 0 auto;/);
  assert.match(css, /\.buzz-line strong \{ color: #f4f4f5 !important; font-size: var\(--buzz-font, 13px\); font-variant-numeric: tabular-nums; white-space: nowrap; \}/);
  assert.doesNotMatch(css, /\.buzz-line strong \{[^}]*text-overflow: ellipsis/);
  assert.doesNotMatch(css, /\.buzz-line strong \{[^}]*overflow: hidden/);
  assert.match(app, /private fitBuzzStack\(\): void/);
  assert.match(app, /this\.restoreScrollPosition\(\);\n    this\.fitBuzzStack\(\);/);
  assert.match(app, /window\.addEventListener\("resize", \(\) => this\.fitBuzzStack\(\)\)/);
  assert.match(app, /stack\.style\.setProperty\("--buzz-font", `\$\{fontPx\}px`\)/);
});

test("Creator Pulse surface and navigation are fully removed", () => {
  assert.doesNotMatch(app, /Creator Pulse/);
  assert.doesNotMatch(app, /renderPulse/);
  assert.doesNotMatch(app, /generatePulse/);
});
