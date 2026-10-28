import { LyricLine, LyricWord } from "./tauri";

/**
 * Búsqueda binaria O(log n) para encontrar el índice de la línea activa en `lines`.
 *
 * @param lines Arreglo de líneas ordenadas ascendentemente por `time_ms`.
 * @param positionSeconds Posición actual de reproducción en segundos.
 * @param offsetMs Ajuste manual o embebido del usuario en milisegundos (positivo = letra antes).
 * @returns Índice de la línea activa (0 .. lines.length - 1) o -1 si aún no ha comenzado la primera línea.
 */
export function findActiveLineIndex(
  lines: LyricLine[],
  positionSeconds: number,
  offsetMs: number = 0
): number {
  if (!lines || lines.length === 0) return -1;

  const currentMs = Math.max(0, Math.floor(positionSeconds * 1000) + offsetMs);

  if (currentMs < lines[0].time_ms) {
    return -1;
  }

  let low = 0;
  let high = lines.length - 1;
  let best = -1;

  while (low <= high) {
    const mid = (low + high) >> 1;
    if (lines[mid].time_ms <= currentMs) {
      best = mid;
      low = mid + 1;
    } else {
      high = mid - 1;
    }
  }

  return best;
}

/**
 * Calcula el progreso de una palabra con animación continua de llenado [0.0 .. 1.0].
 */
export function getWordProgress(word: LyricWord, currentMs: number): number {
  if (currentMs < word.time_ms) return 0;
  if (!word.end_ms || word.end_ms <= word.time_ms) {
    return currentMs >= word.time_ms ? 1 : 0;
  }
  if (currentMs >= word.end_ms) return 1;

  const progress = (currentMs - word.time_ms) / (word.end_ms - word.time_ms);
  return Math.min(1, Math.max(0, progress));
}

/**
 * Formatea segundos a mm:ss (ej: 125.4 -> "02:05").
 */
export function formatLyricTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "00:00";
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins.toString().padStart(2, "0")}:${secs.toString().padStart(2, "0")}`;
}
