import assert from "node:assert/strict";
import test from "node:test";

const {
  BUZZ_FONT_SIZES,
  BUZZ_HEADER_COLUMN_GAP,
  BUZZ_HEADER_MIN_PROFILE_WIDTH,
  buzzBalanceTooltip,
  completedSyncChanged,
  fitBuzzFontSize,
  formatCompactBuzz,
  formatExactBuzz,
  visibleBuzzBalances,
} = await import("../src/buzz.ts");

test("Buzz cache refreshes only after a newly completed sync", () => {
  assert.equal(completedSyncChanged(null, null), false);
  assert.equal(completedSyncChanged("2026-09-13T12:00:00.000Z", "2026-09-13T12:00:00.000Z"), false);
  assert.equal(completedSyncChanged(null, "2026-09-13T12:00:00.000Z"), true);
  assert.equal(completedSyncChanged("2026-09-13T12:00:00.000Z", "2026-09-13T12:05:00.000Z"), true);
});

test("only positive account Buzz balances are visible", () => {
  assert.deepEqual(
    visibleBuzzBalances({ blue: 12, yellow: 0, green: null }),
    [{ kind: "blue", value: 12 }],
  );
  assert.deepEqual(
    visibleBuzzBalances({ blue: 12, yellow: 7, green: 3 }),
    [
      { kind: "blue", value: 12 },
      { kind: "yellow", value: 7 },
      { kind: "green", value: 3 },
    ],
  );
  assert.deepEqual(visibleBuzzBalances({ blue: 0, yellow: null, green: 0 }), []);
});

test("header Buzz counters use a compact k/m/b scale with one decimal", () => {
  assert.equal(formatCompactBuzz(42.5), "42.5");
  assert.equal(formatCompactBuzz(999), "999");
  assert.equal(formatCompactBuzz(5512), "5.5k");
  assert.equal(formatCompactBuzz(17340), "17.3k");
  assert.equal(formatCompactBuzz(17000), "17k");
  assert.equal(formatCompactBuzz(250000), "250k");
  assert.equal(formatCompactBuzz(999950), "1m");
  assert.equal(formatCompactBuzz(1234567.5), "1.2m");
  assert.equal(formatCompactBuzz(99999999.5), "100m");
  assert.equal(formatCompactBuzz(2500000000), "2.5b");
});

test("header Buzz pill steps its font down until the pill fits, never below the smallest step", () => {
  assert.deepEqual([...BUZZ_FONT_SIZES], [13, 12, 11, 10, 9]);
  const widthAt = (fontPx) => fontPx * 16;
  assert.equal(fitBuzzFontSize(300, widthAt), 13);
  assert.equal(fitBuzzFontSize(200, widthAt), 12);
  assert.equal(fitBuzzFontSize(165, widthAt), 10);
  assert.equal(fitBuzzFontSize(50, widthAt), 9);
  const applied = [];
  fitBuzzFontSize(0, (fontPx) => { applied.push(fontPx); return 1000; });
  assert.deepEqual(applied, [13, 12, 11, 10, 9]);
  assert.equal(BUZZ_HEADER_COLUMN_GAP, 8);
  assert.equal(BUZZ_HEADER_MIN_PROFILE_WIDTH, 120);
});

test("exact Buzz values stay available in the tooltip and ledger", () => {
  assert.equal(formatExactBuzz(17340), "17,340");
  assert.equal(formatExactBuzz(1234567.5), "1,234,567.5");
  assert.equal(formatExactBuzz(null), "\u2014");
  assert.equal(
    buzzBalanceTooltip([{ kind: "blue", value: 5512 }, { kind: "yellow", value: 17340 }]),
    "Blue 5,512 \u00b7 Yellow 17,340 \u00b7 Buzz transactions",
  );
});
