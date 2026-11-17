export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

export function framesToMs(frames: number, sampleRate: number): number {
  return sampleRate > 0 ? (frames / sampleRate) * 1000 : 0;
}

export function formatMs(frames: number, sampleRate: number): string {
  const ms = framesToMs(frames, sampleRate);
  return ms >= 100 ? `${ms.toFixed(0)} ms` : `${ms.toFixed(1)} ms`;
}

export function formatSeconds(frames: number, sampleRate: number): string {
  return sampleRate > 0 ? `${(frames / sampleRate).toFixed(1)} s` : "—";
}

/** Level in dBFS for display: no decimals, floor shown as "−∞". */
export function formatDb(db: number | null | undefined): string {
  if (db === null || db === undefined || !Number.isFinite(db) || db <= -90) return "−∞ dB";
  const r = Math.round(db);
  return `${r < 0 ? "−" : ""}${Math.abs(r)} dB`;
}

/** Linear track gain (0..2) as dB text. */
export function formatGain(gain: number): string {
  if (gain <= 0.0005) return "−∞ dB";
  const db = 20 * Math.log10(gain);
  const r = Math.round(db * 10) / 10;
  return `${r > 0 ? "+" : r < 0 ? "−" : ""}${Math.abs(r).toFixed(1)} dB`;
}

export const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
