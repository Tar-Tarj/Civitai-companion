import type { AppSnapshot } from "./types";

export type BuzzKind = "blue" | "yellow" | "green";

export interface VisibleBuzzBalance {
  kind: BuzzKind;
  value: number;
}

export function completedSyncChanged(
  previousLastSuccess: string | null,
  currentLastSuccess: string | null,
): boolean {
  return currentLastSuccess !== null && currentLastSuccess !== previousLastSuccess;
}

const exactFormatter = new Intl.NumberFormat("en-US", { maximumFractionDigits: 1 });
const COMPACT_UNITS: ReadonlyArray<[number, string]> = [[1e9, "b"], [1e6, "m"], [1e3, "k"]];

export function buzzKindLabel(kind: BuzzKind): string {
  return kind === "blue" ? "Blue" : kind === "yellow" ? "Yellow" : "Green";
}

export function formatExactBuzz(value: number | null): string {
  return value === null || !Number.isFinite(value) ? "\u2014" : exactFormatter.format(value);
}

export function formatCompactBuzz(value: number): string {
  if (!Number.isFinite(value)) return "\u2014";
  for (const [threshold, suffix] of COMPACT_UNITS) {
    if (Math.abs(value) < threshold * 0.9995) continue;
    const scaled = value / threshold;
    const digits = Math.abs(scaled) >= 99.95 ? 0 : 1;
    const rounded = Number(scaled.toFixed(digits));
    if (Math.abs(rounded) >= 1) return `${rounded}${suffix}`;
  }
  return exactFormatter.format(value);
}

/** Font sizes tried, largest first, until the header Buzz pill fits. */
export const BUZZ_FONT_SIZES: ReadonlyArray<number> = [13, 12, 11, 10, 9];
/** Grid gap between the profile column and the stats column in .account-summary. */
export const BUZZ_HEADER_COLUMN_GAP = 8;
/** Minimum width reserved for the avatar and username column in .account-summary. */
export const BUZZ_HEADER_MIN_PROFILE_WIDTH = 120;

export function fitBuzzFontSize(available: number, widthAt: (fontPx: number) => number): number {
  let chosen = BUZZ_FONT_SIZES[0] ?? 13;
  for (const size of BUZZ_FONT_SIZES) {
    chosen = size;
    if (widthAt(size) <= available) break;
  }
  return chosen;
}

export function buzzBalanceTooltip(balances: ReadonlyArray<VisibleBuzzBalance>): string {
  const parts = balances.map(({ kind, value }) => `${buzzKindLabel(kind)} ${formatExactBuzz(value)}`);
  return [...parts, "Buzz transactions"].join(" \u00b7 ");
}

export function visibleBuzzBalances(buzz: AppSnapshot["buzz"]): VisibleBuzzBalance[] {
  const balances: ReadonlyArray<[BuzzKind, number | null]> = [
    ["blue", buzz.blue],
    ["yellow", buzz.yellow],
    ["green", buzz.green],
  ];
  return balances
    .filter((entry): entry is [BuzzKind, number] => (
      typeof entry[1] === "number" && Number.isFinite(entry[1]) && entry[1] > 0
    ))
    .map(([kind, value]) => ({ kind, value }));
}
