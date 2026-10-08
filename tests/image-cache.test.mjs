import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("../src/images.ts", import.meta.url), "utf8");

test("image cache covers the complete notification surface and refreshes recency", () => {
  assert.match(source, /const MAX_CACHED_IMAGES = 320;/);
  assert.match(
    source,
    /if \(existing\) \{\s*cache\.delete\(url\);\s*cache\.set\(url, existing\);\s*return existing;/,
  );
});
