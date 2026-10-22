export function formatMinutes(
  value: number | null | undefined,
  decimals = 1,
): string {
  if (value === null || value === undefined || !Number.isFinite(value)) {
    return '—';
  }
  return `${value.toFixed(decimals)} min`;
}

export function formatInt(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) {
    return '—';
  }
  return Math.round(value).toLocaleString();
}

export function formatPct(value: number, decimals = 0): string {
  return `${value.toFixed(decimals)}%`;
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
