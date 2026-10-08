export function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

export function button(className: string, label: string, action: () => void): HTMLButtonElement {
  const node = element("button", className, label);
  node.type = "button";
  node.addEventListener("click", action);
  return node;
}

export function iconButton(label: string, glyph: string, action: () => void): HTMLButtonElement {
  const node = button("icon-button", glyph, action);
  node.setAttribute("aria-label", label);
  node.title = label;
  return node;
}

export function formatNumber(value: number | null, fractionDigits = 0): string {
  return value === null
    ? "—"
    : new Intl.NumberFormat(undefined, { maximumFractionDigits: fractionDigits }).format(value);
}

export function formatDate(value: string | null): string {
  if (!value) return "Never";
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? "Unknown"
    : new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

export function relativeTime(value: string | null, suffix = ""): string {
  if (!value) return "";
  const timestamp = Date.parse(value);
  if (Number.isNaN(timestamp)) return "";
  const seconds = Math.max(0, Math.round((Date.now() - timestamp) / 1000));
  let age = "now";
  if (seconds >= 86400) age = `${Math.max(1, Math.floor(seconds / 86400))}d`;
  else if (seconds >= 3600) age = `${Math.max(1, Math.floor(seconds / 3600))}h`;
  else if (seconds >= 60) age = `${Math.max(1, Math.floor(seconds / 60))}m`;
  return `${age}${suffix}`;
}

export function isAllowedImageUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === "https:" && url.hostname === "image.civitai.com";
  } catch {
    return false;
  }
}
