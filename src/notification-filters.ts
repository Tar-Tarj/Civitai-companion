import type { Filter, NotificationItem } from "./types";

const SUBMISSION_UPDATE_TYPES = new Set([
  "featured-image",
  "image-featured",
  "beggars-board-rejected",
  "beggars-board-expired",
]);

const MODEL_UPDATE_TYPES = new Set([
  "early-access-complete",
  "old-draft",
]);

export function isModelUpdateNotification(item: Pick<NotificationItem, "type">): boolean {
  const kind = item.type.toLowerCase();
  return kind.startsWith("new-model-") || MODEL_UPDATE_TYPES.has(kind);
}

export function isSubmissionUpdateNotification(
  item: Pick<NotificationItem, "type" | "category">,
): boolean {
  const kind = item.type.toLowerCase();
  const category = item.category.toLowerCase();
  return kind !== "followed-by"
    && !isModelUpdateNotification(item)
    && (category === "update" || category === "bounty" || SUBMISSION_UPDATE_TYPES.has(kind));
}

export function notificationMatchesFilter(
  item: Pick<NotificationItem, "type" | "category">,
  filter: Filter,
): boolean {
  const kind = item.type.toLowerCase();
  const category = item.category.toLowerCase();
  if (filter === "all") return true;
  if (filter === "comments") return kind.includes("comment") || category === "comment";
  if (filter === "followers") return kind === "followed-by";
  if (filter === "milestones") return kind.includes("reaction") || category === "milestone";
  if (filter === "tips") return kind.includes("tip");
  if (filter === "models") return isModelUpdateNotification(item);
  if (filter === "updates") return isSubmissionUpdateNotification(item);
  return false;
}
