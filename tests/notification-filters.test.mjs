import assert from "node:assert/strict";
import test from "node:test";

const filters = await import("../src/notification-filters.ts");

function notification(type, category = "Other", read = false) {
  return { type, category, read };
}

test("followers never leak into submission and bounty updates", () => {
  const follower = notification("followed-by", "Update");
  assert.equal(filters.notificationMatchesFilter(follower, "followers"), true);
  assert.equal(filters.notificationMatchesFilter(follower, "updates"), false);
});

test("model updates have an exclusive filter", () => {
  for (const item of [
    notification("new-model-version", "Update"),
    notification("new-model-from-following", "Update"),
    notification("early-access-complete", "Update"),
    notification("old-draft", "System"),
  ]) {
    assert.equal(filters.notificationMatchesFilter(item, "models"), true);
    assert.equal(filters.notificationMatchesFilter(item, "updates"), false);
  }
});

test("submission and bounty updates remain in their filter", () => {
  assert.equal(filters.notificationMatchesFilter(notification("collection-item-accepted", "Update"), "updates"), true);
  assert.equal(filters.notificationMatchesFilter(notification("bounty-awarded", "Bounty"), "updates"), true);
  assert.equal(filters.notificationMatchesFilter(notification("featured-image", "System"), "updates"), true);
});
