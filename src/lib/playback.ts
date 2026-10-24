/**
 * Utilidades de reproducción, independientes de React y de Tauri.
 *
 * El backend ya filtra valores no finitos, pero el frontend nunca debe
 * confiar en ello: un `duration_seconds` en 0, `NaN` o `Infinity` rompería
 * el slider (max inválido, thumb fuera de rango) y el texto de tiempo.
 */

/** Redondea un número al rango [min, max]; devuelve `fallback` si no es finito. */
export function clamp(value: number, min: number, max: number, fallback = min): number {
  if (!Number.isFinite(value)) return fallback;
  if (max < min) return min;
  return Math.min(Math.max(value, min), max);
}

/** Duración segura para usar como `max` de un slider (siempre >= 1). */
export function safeDuration(durationSeconds: number): number {
  if (!Number.isFinite(durationSeconds) || durationSeconds <= 0) return 0;
  return durationSeconds;
}

/** Posición segura, acotada a [0, duration] cuando hay duración conocida. */
export function safePosition(positionSeconds: number, durationSeconds: number): number {
  const duration = safeDuration(durationSeconds);
  const position = clamp(positionSeconds, 0, duration > 0 ? duration : Number.MAX_SAFE_INTEGER, 0);
  return duration > 0 ? clamp(position, 0, duration) : position;
}

/**
 * Destino de un seek relativo (flechas ←/→), acotado a la duración.
 * Sin duración conocida solo se evita bajar de 0.
 */
export function seekTarget(
  positionSeconds: number,
  deltaSeconds: number,
  durationSeconds: number,
): number {
  const base = Number.isFinite(positionSeconds) ? positionSeconds : 0;
  const target = base + deltaSeconds;
  const duration = safeDuration(durationSeconds);
  if (duration > 0) return clamp(target, 0, duration);
  return Math.max(0, target);
}

/** Formatea segundos como `m:ss` (o `h:mm:ss` para audios largos). */
export function formatDuration(totalSeconds: number): string {
  if (!Number.isFinite(totalSeconds) || totalSeconds < 0) return "0:00";

  const whole = Math.floor(totalSeconds);
  const hours = Math.floor(whole / 3600);
  const minutes = Math.floor((whole % 3600) / 60);
  const seconds = whole % 60;

  if (hours > 0) {
    return `${hours}:${minutes.toString().padStart(2, "0")}:${seconds
      .toString()
      .padStart(2, "0")}`;
  }
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}
