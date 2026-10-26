import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

// Bundle the real TypeScript modules; replace only desktop/network/DOM boundaries.
// No live account or WebView is required to exercise the request lifecycle.
async function loadModule(file, backend, { mockImages = false } = {}) {
  const result = await build({
    entryPoints: [fileURLToPath(new URL(`../src/${file}`, import.meta.url))],
    bundle: true,
    write: false,
    format: "cjs",
    platform: "node",
    loader: { ".png": "dataurl" },
    define: { "import.meta.env.VITE_TEST_PANEL": '"0"' },
    plugins: [{
      name: "desktop-boundaries",
      setup(builder) {
        builder.onLoad({ filter: /[/\\]bridge\.ts$/ }, () => ({
          contents: "export const backend = globalThis.testBackend;",
          loader: "js",
        }));
        builder.onLoad({ filter: /[/\\]audio\.ts$/ }, () => ({
          contents: "export async function playSound() {}",
          loader: "js",
        }));
        if (mockImages) builder.onLoad({ filter: /[/\\]images\.ts$/ }, () => ({
          contents: "export function clearImageCache() { globalThis.imageClears++; } export function loadCivitaiImage() {}",
          loader: "js",
        }));
        builder.onResolve({ filter: /^@tauri-apps\/api\/window$/ }, () => ({
          path: "window", namespace: "desktop",
        }));
        builder.onLoad({ filter: /.*/, namespace: "desktop" }, () => ({
          contents: "export function getCurrentWindow() { return {}; }",
          loader: "js",
        }));
      },
    }],
  });
  const revoked = [];
  const sandbox = {
    module: { exports: {} },
    testBackend: backend,
    imageClears: 0,
    window: { addEventListener() {}, atob: value => Buffer.from(value, "base64").toString("binary") },
    Blob,
    URL: class extends URL {
      static createObjectURL() { return "blob:test"; }
      static revokeObjectURL(url) { revoked.push(url); }
    },
  };
  vm.runInNewContext(result.outputFiles[0].text, sandbox);
  return { ...sandbox.module.exports, sandbox, revoked };
}

const snapshot = (generation = 0, userId = 1, configured = true, lastSuccess = "first") => ({
  accountGeneration: generation,
  credentialConfigured: configured,
  account: { userId },
  sync: { lastSuccess },
});

async function appWith(backend = {}) {
  const module = await loadModule("app.ts", backend, { mockImages: true });
  const app = new module.CompanionApp({}, snapshot());
  app.render = () => {};
  return { app, module };
}

function deferred() {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
}

test("removal and account replacement discard cached transactions and images", async () => {
  const { app, module } = await appWith();
  app.buzzPages.blue.items = [{ description: "private A transaction" }];
  app.buzzPages.blue.loaded = true;
  app.acceptSnapshot(snapshot(1, null, false, null));
  assert.equal(app.buzzPages.blue.items.length, 0);
  assert.equal(app.buzzPages.blue.loaded, false);
  assert.equal(module.sandbox.imageClears, 1);

  app.acceptSnapshot(snapshot(2, 2));
  assert.equal(app.buzzPages.blue.items.length, 0);
  assert.equal(module.sandbox.imageClears, 2);
});

test("old command responses cannot restore an earlier account generation", async () => {
  const { app } = await appWith();
  app.acceptSnapshot(snapshot(1, null, false, null));
  await app.runNow(async () => snapshot(0, 1));
  assert.equal(app.snapshot.accountGeneration, 1);
  assert.equal(app.snapshot.credentialConfigured, false);
});

test("failed first-page load retries once on user action without an automatic loop", async () => {
  let calls = 0;
  const { app } = await appWith({ buzzTransactions: async () => {
    calls++;
    if (calls === 1) throw "temporary failure";
    return { transactions: [{ description: "new data" }], nextCursor: null };
  } });
  app.view = "buzz";
  await app.loadBuzzTransactions(false);
  await Promise.resolve();
  assert.equal(calls, 1);
  assert.equal(app.buzzPages.blue.error, "temporary failure");
  await app.loadBuzzTransactions(false);
  assert.equal(calls, 2);
  assert.equal(app.buzzPages.blue.error, "");
  assert.equal(app.buzzPages.blue.items[0].description, "new data");
});

test("an old in-flight response cannot overwrite or unlock a new-account request", async () => {
  const old = deferred();
  const current = deferred();
  let calls = 0;
  const { app } = await appWith({ buzzTransactions: () => ++calls === 1 ? old.promise : current.promise });
  const first = app.loadBuzzTransactions(false);
  app.acceptSnapshot(snapshot(1, 2));
  const second = app.loadBuzzTransactions(false);
  old.resolve({ transactions: [{ description: "A" }], nextCursor: null });
  await first;
  assert.equal(app.buzzPages.blue.items.length, 0);
  assert.equal(app.buzzPages.blue.loading, true);
  current.resolve({ transactions: [{ description: "B" }], nextCursor: null });
  await second;
  assert.equal(app.buzzPages.blue.items[0].description, "B");
  assert.equal(app.buzzPages.blue.loading, false);
});

test("a failed new-account request cannot fall back to old-account rows", async () => {
  const { app } = await appWith({ buzzTransactions: async () => { throw "offline"; } });
  app.buzzPages.blue.items = [{ description: "A" }];
  app.buzzPages.blue.loaded = true;
  app.acceptSnapshot(snapshot(1, 2));
  await app.loadBuzzTransactions(false);
  assert.equal(app.buzzPages.blue.items.length, 0);
  assert.equal(app.buzzPages.blue.error, "offline");
});

test("removing a credential prevents new ledger requests", async () => {
  let calls = 0;
  const { app } = await appWith({ buzzTransactions: async () => { calls++; } });
  app.acceptSnapshot(snapshot(1, null, false, null));
  await app.loadBuzzTransactions(false);
  assert.equal(calls, 0);
});

test("image-cache clearing discards pending media and revokes existing blobs", async () => {
  const pending = deferred();
  let calls = 0;
  const images = await loadModule("images.ts", { fetchImage: () => ++calls === 1
    ? Promise.resolve({ data: "eA==", mimeType: "image/png" }) : pending.promise });
  const first = { isConnected: true, addEventListener() {} };
  images.loadCivitaiImage(first, "https://image.civitai.com/one", () => {});
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(first.src, "blob:test");
  const second = { isConnected: true, addEventListener() {} };
  images.loadCivitaiImage(second, "https://image.civitai.com/two", () => {});
  images.clearImageCache();
  pending.resolve({ data: "eA==", mimeType: "image/png" });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(second.src, undefined);
  assert.ok(images.revoked.includes("blob:test"));
});
