import { useEffect, useRef, useState, useCallback } from "react";

interface LyricsScrollOptions {
  activeLineIndex: number;
  viewportRef: React.RefObject<HTMLDivElement | null>;
  lineRefs: React.MutableRefObject<(HTMLDivElement | null)[]>;
  enabled?: boolean;
}

export function useLyricsScroll({
  activeLineIndex,
  viewportRef,
  lineRefs,
  enabled = true,
}: LyricsScrollOptions) {
  const [isUserScrolling, setIsUserScrolling] = useState(false);
  const scrollTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isProgrammaticScrollRef = useRef(false);

  const scrollToActiveLine = useCallback(() => {
    const viewport = viewportRef.current;
    if (!viewport || activeLineIndex < 0) return;

    const activeEl = lineRefs.current[activeLineIndex];
    if (!activeEl) return;

    isProgrammaticScrollRef.current = true;

    // Calcular desplazamiento para centrar la línea activa en el tercio superior/medio
    const viewportHeight = viewport.clientHeight;
    const targetScrollTop =
      activeEl.offsetTop - viewportHeight * 0.38 + activeEl.clientHeight / 2;

    viewport.scrollTo({
      top: Math.max(0, targetScrollTop),
      behavior: "smooth",
    });

    // Desactivar bandera de scroll programático tras completarse la animación
    setTimeout(() => {
      isProgrammaticScrollRef.current = false;
    }, 450);
  }, [activeLineIndex, viewportRef, lineRefs]);

  // Centrar la línea activa automáticamente si el usuario no está scrolleando manualmente
  useEffect(() => {
    if (!enabled || isUserScrolling) return;
    scrollToActiveLine();
  }, [activeLineIndex, enabled, isUserScrolling, scrollToActiveLine]);

  // Listener para detectar intervención manual del usuario
  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;

    const handleUserInteraction = () => {
      if (isProgrammaticScrollRef.current) return;

      setIsUserScrolling(true);

      if (scrollTimeoutRef.current) {
        clearTimeout(scrollTimeoutRef.current);
      }

      // Reanuda el auto-scroll automáticamente tras 3.5 segundos de inactividad
      scrollTimeoutRef.current = setTimeout(() => {
        setIsUserScrolling(false);
      }, 3500);
    };

    viewport.addEventListener("wheel", handleUserInteraction, { passive: true });
    viewport.addEventListener("touchmove", handleUserInteraction, { passive: true });

    return () => {
      viewport.removeEventListener("wheel", handleUserInteraction);
      viewport.removeEventListener("touchmove", handleUserInteraction);
      if (scrollTimeoutRef.current) {
        clearTimeout(scrollTimeoutRef.current);
      }
    };
  }, [viewportRef]);

  const resumeScroll = useCallback(() => {
    if (scrollTimeoutRef.current) {
      clearTimeout(scrollTimeoutRef.current);
      scrollTimeoutRef.current = null;
    }
    setIsUserScrolling(false);
    scrollToActiveLine();
  }, [scrollToActiveLine]);

  return {
    isUserScrolling,
    resumeScroll,
  };
}
