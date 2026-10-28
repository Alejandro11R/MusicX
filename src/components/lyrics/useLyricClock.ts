import { useState, useEffect, useRef } from "react";

interface LyricClockProps {
  isPlaying: boolean;
  basePosition: number;
  speed?: number;
}

/**
 * Reloj ultra-eficiente a 60 fps basado en requestAnimationFrame para sincronización suave.
 *
 * Características:
 * - Se congela automáticamente al pausar o si la ventana se oculta (document.hidden) para 0% consumo de CPU.
 * - Compensa dinámicamente la velocidad de reproducción (Slowed 0.88x, Nightcore 1.05x, etc.).
 * - Se auto-corrige cada vez que llega un tick oficial desde el backend (2 Hz).
 */
export function useLyricClock({ isPlaying, basePosition, speed = 1.0 }: LyricClockProps): number {
  const [position, setPosition] = useState(basePosition);
  const basePosRef = useRef(basePosition);
  const baseTimeRef = useRef(performance.now());
  const rafIdRef = useRef<number | null>(null);

  // Cada vez que llega una posición base desde el backend, sincronizamos el ancla
  useEffect(() => {
    basePosRef.current = basePosition;
    baseTimeRef.current = performance.now();
    setPosition(basePosition);
  }, [basePosition]);

  useEffect(() => {
    if (!isPlaying) {
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = null;
      }
      return;
    }

    let isDocumentVisible = !document.hidden;

    const handleVisibilityChange = () => {
      isDocumentVisible = !document.hidden;
      if (isDocumentVisible) {
        // Al regresar de estar oculto, reiniciamos ancla para no saltar tiempo
        baseTimeRef.current = performance.now();
      }
    };

    document.addEventListener("visibilitychange", handleVisibilityChange);

    const tick = () => {
      if (isDocumentVisible) {
        const now = performance.now();
        const elapsedSecs = ((now - baseTimeRef.current) / 1000) * speed;
        setPosition(basePosRef.current + elapsedSecs);
      }
      rafIdRef.current = requestAnimationFrame(tick);
    };

    rafIdRef.current = requestAnimationFrame(tick);

    return () => {
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = null;
      }
    };
  }, [isPlaying, speed]);

  return position;
}
