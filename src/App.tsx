import React, {
  useEffect,
  useRef,
  useState,
  useCallback,
  useMemo,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import {
  Clock,
  Headphones,
  Loader2,
  Minus,
  Pause,
  Play,
  Repeat,
  Search,
  Square,
  Volume2,
  VolumeX,
  X,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Slider } from "@/components/ui/slider";
import { cn } from "@/lib/utils";
import { formatDuration, safeDuration, safePosition, seekTarget } from "@/lib/playback";
import {
  search,
  play,
  pause,
  resume,
  stop,
  setVolume,
  seek,
  setLoop,
  getState,
  health,
  quit,
  subscribeEvents,
  listenOrchestratorEvents,
  type PlayerState,
  type SearchResult,
  type OrchestratorEvent,
} from "./lib/tauri";

const POLL_INTERVAL_MS = 5000; // Fallback polling every 5s
const RECENT_STORAGE_KEY = "cadence_recent_tracks";
// Tras un seek, ignoramos los `Progress` que aún traen la posición vieja
// (mpv tarda uno o dos ticks en reflejar el salto) para que el thumb no
// rebote visualmente.
const SEEK_GUARD_MS = 1200;

// ===== Memoized Components =====

interface SearchResultItemProps {
  track: SearchResult;
  isSelected: boolean;
  isPlaying: boolean;
  isLoading: boolean;
  onClick: () => void;
  onMouseEnter: () => void;
}

const SearchResultItem = React.memo(function SearchResultItem({
  track,
  isSelected,
  isPlaying,
  isLoading,
  onClick,
  onMouseEnter,
}: SearchResultItemProps) {
  return (
    <li>
      <button
        type="button"
        className={cn(
          "group relative flex w-full items-center gap-3 rounded-xl p-2 text-left transition-all duration-200",
          "bg-white/[0.02] hover:bg-white/[0.08] border border-transparent hover:border-white/10",
          isSelected && "bg-white/[0.09] border-white/15",
          isPlaying &&
            "bg-teal-500/10 border-teal-500/30 shadow-[0_0_15px_-3px_rgba(45,212,191,0.2)]",
        )}
        onClick={onClick}
        onMouseEnter={onMouseEnter}
        disabled={isLoading}
      >
        <div className="relative size-10 shrink-0 overflow-hidden rounded-lg bg-white/5 border border-white/10 shadow-sm">
          <img
            src={track.thumbnail_url}
            alt=""
            className="size-full object-cover transition-transform duration-300 group-hover:scale-105"
            loading="lazy"
            onError={(e) => {
              (e.currentTarget as HTMLElement).style.display = "none";
            }}
          />
          {isLoading && (
            <div className="absolute inset-0 flex items-center justify-center bg-black/70 backdrop-blur-xs">
              <Loader2 className="size-4 animate-spin text-teal-300" />
            </div>
          )}
        </div>
        <div className="flex min-w-0 flex-1 flex-col justify-center">
          <span
            className={cn(
              "truncate text-xs font-medium leading-tight",
              isPlaying ? "text-teal-300" : "text-white/90",
            )}
          >
            {track.title}
          </span>
          {track.artist && (
            <span className="truncate text-[11px] text-white/50">
              {track.artist}
            </span>
          )}
        </div>
        <span className="shrink-0 text-[11px] tabular-nums text-white/40">
          {formatDuration(track.duration_seconds)}
        </span>
      </button>
    </li>
  );
});

interface RecentTrackItemProps {
  track: SearchResult;
  isPlaying: boolean;
  isLoading: boolean;
  onClick: () => void;
}

const RecentTrackItem = React.memo(function RecentTrackItem({
  track,
  isPlaying,
  isLoading,
  onClick,
}: RecentTrackItemProps) {
  return (
    <li>
      <button
        type="button"
        className={cn(
          "group relative flex w-full items-center gap-3 rounded-xl p-2 text-left transition-all duration-200",
          "bg-white/[0.02] hover:bg-white/[0.08] border border-transparent hover:border-white/10",
          isPlaying &&
            "bg-teal-500/10 border-teal-500/30 shadow-[0_0_15px_-3px_rgba(45,212,191,0.2)]",
        )}
        onClick={onClick}
        disabled={isLoading}
      >
        <div className="relative size-10 shrink-0 overflow-hidden rounded-lg bg-white/5 border border-white/10 shadow-sm">
          <img
            src={track.thumbnail_url}
            alt=""
            className="size-full object-cover transition-transform duration-300 group-hover:scale-105"
            loading="lazy"
            onError={(e) => {
              (e.currentTarget as HTMLElement).style.display = "none";
            }}
          />
          {isLoading && (
            <div className="absolute inset-0 flex items-center justify-center bg-black/70 backdrop-blur-xs">
              <Loader2 className="size-4 animate-spin text-teal-300" />
            </div>
          )}
        </div>
        <div className="flex min-w-0 flex-1 flex-col justify-center">
          <span
            className={cn(
              "truncate text-xs font-medium leading-tight",
              isPlaying ? "text-teal-300" : "text-white/95",
            )}
          >
            {track.title}
          </span>
          {track.artist && (
            <span className="truncate text-[11px] text-white/60">
              {track.artist}
            </span>
          )}
        </div>
        <span className="shrink-0 text-[11px] tabular-nums text-white/50">
          {formatDuration(track.duration_seconds)}
        </span>
      </button>
    </li>
  );
});

interface NowPlayingProps {
  playerState: PlayerState | null;
  isTrackLoading: boolean;
  isPlaying: boolean;
  isLooping: boolean;
  dragPosition: number | null;
  onTogglePlayback: () => void;
  onToggleLoop: () => void;
  onStop: () => void;
  /** Solo mueve el thumb; no llama al backend hasta soltar. */
  onSeekPreview: (position: number) => void;
  /** Confirma el seek al soltar el slider. */
  onSeekCommit: (position: number) => void;
  /** Mueve el volumen en la UI sin llamar a mpv todavía. */
  onVolumePreview: (level: number) => void;
  /** Aplica el volumen definitivo al soltar el slider. */
  onVolumeCommit: (level: number) => void;
  onToggleMute: () => void;
  volume: number;
}

const NowPlaying = React.memo(function NowPlaying({
  playerState,
  isTrackLoading,
  isPlaying,
  isLooping,
  dragPosition,
  onTogglePlayback,
  onToggleLoop,
  onStop,
  onSeekPreview,
  onSeekCommit,
  onVolumePreview,
  onVolumeCommit,
  onToggleMute,
  volume,
}: NowPlayingProps) {
  const current = playerState?.current;
  const duration = safeDuration(playerState?.duration_seconds ?? 0);
  const position = safePosition(
    dragPosition ?? playerState?.position_seconds ?? 0,
    duration,
  );

  return (
    <div className="liquid-card mt-auto flex flex-col gap-3 rounded-2xl p-3 border border-white/10 shadow-xl" role="region" aria-label="Reproductor">
      {isTrackLoading && !current && (
        <div className="flex items-center gap-2 py-1 text-xs text-teal-300" role="status">
          <Loader2 className="size-3.5 animate-spin" />
          <span>Cargando pista de audio...</span>
        </div>
      )}

      {current && (
        <div className="flex items-center gap-3">
          <div
            className={cn(
              "size-12 shrink-0 overflow-hidden rounded-xl bg-white/10 border border-white/15 transition-all duration-300",
              isPlaying && "shadow-[0_4px_20px_-2px_rgba(45,212,191,0.35)]",
            )}
          >
            <img
              src={current.thumbnail_url}
              alt=""
              className="size-full object-cover"
              onError={(e) => {
                (e.currentTarget as HTMLElement).style.display = "none";
              }}
            />
          </div>
          <div className="flex min-w-0 flex-1 flex-col justify-center gap-0.5">
            <div className="flex items-center gap-1.5">
              <span
                className={cn(
                  "size-1.5 shrink-0 rounded-full transition-all",
                  isPlaying ? "bg-teal-400 shadow-[0_0_6px_#2dd4bf] animate-pulse" : "bg-white/30",
                )}
              />
              <p className="truncate text-xs font-semibold text-white/95">
                {current.title}
              </p>
            </div>
            {current.artist && (
              <p className="truncate pl-3 text-[11px] text-white/60">
                {current.artist}
              </p>
            )}
          </div>
        </div>
      )}

      {current && (
        <div className="flex items-center gap-2 px-0.5">
          <span className="w-8 shrink-0 text-right text-[10px] tabular-nums text-white/60">
            {formatDuration(position)}
          </span>
          <Slider
            className="flex-1"
            value={[position]}
            max={Math.max(duration, 1)}
            step={1}
            onValueChange={([value]) => onSeekPreview(value)}
            onValueCommit={([value]) => onSeekCommit(value)}
            aria-label="Posición de reproducción"
            aria-valuetext={`${formatDuration(position)} de ${formatDuration(duration)}`}
          />
          <span className="w-8 shrink-0 text-[10px] tabular-nums text-white/60">
            {formatDuration(duration)}
          </span>
        </div>
      )}

      {/* Controles Principales */}
      <div className="flex items-center justify-center gap-3 pt-0.5">
        <Button
          variant="ghost"
          size="icon"
          className={cn(
            "size-8 rounded-full border transition-all",
            isLooping
              ? "bg-teal-400/20 text-teal-300 border-teal-400/40 shadow-[0_0_12px_rgba(45,212,191,0.3)]"
              : "bg-white/[0.04] text-white/60 border-white/5 hover:bg-white/[0.1] hover:text-white",
          )}
          onClick={onToggleLoop}
          aria-label={isLooping ? "Desactivar bucle (L)" : "Activar bucle (L)"}
          title={isLooping ? "Bucle activo (L)" : "Bucle inactivo (L)"}
        >
          <Repeat className={cn("size-3.5", isLooping && "stroke-[2.5]")} />
        </Button>

        <Button
          size="icon"
          className="size-10 rounded-full bg-gradient-to-br from-teal-400 to-emerald-500 text-slate-950 shadow-[0_0_18px_rgba(45,212,191,0.45)] hover:shadow-[0_0_24px_rgba(45,212,191,0.65)] hover:scale-105 active:scale-95 transition-all"
          onClick={onTogglePlayback}
          aria-label={isPlaying ? "Pausar (Espacio)" : "Reproducir (Espacio)"}
          title={isPlaying ? "Pausar (Espacio)" : "Reproducir (Espacio)"}
        >
          {isPlaying ? <Pause className="size-4 fill-current" /> : <Play className="size-4 ml-0.5 fill-current" />}
        </Button>

        <Button
          variant="ghost"
          size="icon"
          className="size-8 rounded-full bg-white/[0.04] border border-white/5 text-white/60 hover:bg-white/[0.1] hover:text-white transition-all"
          onClick={onStop}
          aria-label="Detener"
          title="Detener"
        >
          <Square className="size-3.5 fill-current" />
        </Button>
      </div>

      {/* Control de Volumen Líquido */}
      <div className="flex items-center gap-2 px-1 pt-0.5">
        <button
          type="button"
          onClick={onToggleMute}
          className="text-white/60 hover:text-white transition-colors"
          aria-label={volume === 0 ? "Activar sonido (M)" : "Silenciar (M)"}
          title={volume === 0 ? "Activar sonido (M)" : "Silenciar (M)"}
        >
          {volume === 0 ? (
            <VolumeX className="size-3.5 shrink-0 text-rose-300" />
          ) : (
            <Volume2 className="size-3.5 shrink-0 text-white/60" />
          )}
        </button>
        <Slider
          className="flex-1"
          value={[volume]}
          max={100}
          step={1}
          onValueChange={([level]) => onVolumePreview(level)}
          onValueCommit={([level]) => onVolumeCommit(level)}
          aria-label="Volumen"
          aria-valuetext={`${volume}%`}
        />
        <span className="w-6 shrink-0 text-right text-[10px] tabular-nums text-white/60">
          {volume}
        </span>
      </div>
    </div>
  );
});

// ===== Main App =====

function App() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [resultsQuery, setResultsQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isSearching, setIsSearching] = useState(false);
  const [loadingTrackId, setLoadingTrackId] = useState<string | null>(null);
  const [playerState, setPlayerState] = useState<PlayerState | null>(null);
  const [dragPosition, setDragPosition] = useState<number | null>(null);
  const [recentTracks, setRecentTracks] = useState<SearchResult[]>([]);
  const [prevVolume, setPrevVolume] = useState<number>(100);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const playerStateRef = useRef<PlayerState | null>(null);
  const prevVolumeRef = useRef<number>(100);
  const searchAbortRef = useRef<AbortController | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);
  const seekGuardRef = useRef<{ target: number; at: number } | null>(null);

  useEffect(() => {
    playerStateRef.current = playerState;
  }, [playerState]);

  useEffect(() => {
    prevVolumeRef.current = prevVolume;
  }, [prevVolume]);

  useEffect(() => {
    searchInputRef.current?.focus();

    // Cargar historial reciente
    try {
      const stored = localStorage.getItem(RECENT_STORAGE_KEY);
      if (stored) {
        setRecentTracks(JSON.parse(stored));
      }
    } catch (e) {
      console.error("Error al cargar historial:", e);
    }

    // Health check inicial
    health()
      .then((h) => {
        if (!h.mpv) {
          setError("mpv no está disponible en el sistema.");
        } else if (!h.yt_dlp) {
          setError("yt-dlp no está disponible en el sistema.");
        }
      })
      .catch((err) => console.error("health() check failed:", err));

    // Suscribirse a eventos del orquestador
    listenOrchestratorEvents(handleOrchestratorEvent).then((unlisten) => {
      unlistenRef.current = unlisten;
    });

    // Suscribirse a eventos Tauri (para que el backend emita)
    subscribeEvents().catch((err) => console.error("subscribeEvents failed:", err));

    // Fallback polling (cada 5s) solo para recovery
    const poll = async () => {
      try {
        setPlayerState(await getState());
      } catch (err) {
        console.error("state() poll failed:", err);
      }
    };
    poll();
    const interval = setInterval(poll, POLL_INTERVAL_MS);

    return () => {
      clearInterval(interval);
      unlistenRef.current?.();
    };
  }, []);

  // Manejar eventos del orquestador
  const handleOrchestratorEvent = useCallback((event: OrchestratorEvent) => {
    switch (event.type) {
      case "Progress":
        // Patch local: evita un round-trip IPC a 2 Hz. Mientras se arrastra el
        // slider, NowPlaying ignora esto y usa `dragPosition`.
        setPlayerState((prev) => {
          if (!prev) return prev;
          const guard = seekGuardRef.current;
          let position = event.data.position_seconds;
          if (guard) {
            if (Date.now() - guard.at > SEEK_GUARD_MS) {
              seekGuardRef.current = null;
            } else if (Math.abs(position - guard.target) > 1.5) {
              // mpv todavía reporta la posición anterior al seek.
              position = prev.position_seconds;
            } else {
              seekGuardRef.current = null;
            }
          }
          return {
            ...prev,
            position_seconds: position,
            duration_seconds: event.data.duration_seconds,
          };
        });
        break;
      case "ConnectionChanged":
        setPlayerState((prev) =>
          prev ? { ...prev, connected: event.data.connected } : prev,
        );
        if (event.data.connected) {
          getState().then(setPlayerState).catch(console.error);
        }
        break;
      case "PlaybackError":
        setError(event.data);
        getState().then(setPlayerState).catch(console.error);
        break;
      case "TrackStarted":
      case "TrackEnded":
      case "StateChanged":
        getState().then(setPlayerState).catch(console.error);
        break;
    }
  }, []);

  useEffect(() => {
    function handleGlobalKeyDown(e: KeyboardEvent) {
      const isSearchFocused = document.activeElement === searchInputRef.current;
      if (e.key === "/" && !isSearchFocused) {
        e.preventDefault();
        searchInputRef.current?.focus();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "q") {
        e.preventDefault();
        quit();
      } else if (!isSearchFocused && (e.code === "Space" || e.key === " ")) {
        e.preventDefault();
        handleTogglePlayback();
      } else if (!isSearchFocused && e.key.toLowerCase() === "m") {
        e.preventDefault();
        handleToggleMute();
      } else if (!isSearchFocused && e.key.toLowerCase() === "l") {
        e.preventDefault();
        handleToggleLoop();
      } else if (!isSearchFocused && e.key === "ArrowLeft") {
        e.preventDefault();
        handleSeekBy(-5);
      } else if (!isSearchFocused && e.key === "ArrowRight") {
        e.preventDefault();
        handleSeekBy(5);
      }
    }
    document.addEventListener("keydown", handleGlobalKeyDown);
    return () => document.removeEventListener("keydown", handleGlobalKeyDown);
  }, []);

  function saveToRecent(track: SearchResult) {
    setRecentTracks((prev) => {
      const filtered = prev.filter((t) => t.id !== track.id);
      const updated = [track, ...filtered].slice(0, 8);
      try {
        localStorage.setItem(RECENT_STORAGE_KEY, JSON.stringify(updated));
      } catch (e) {
        console.error("Error al guardar historial:", e);
      }
      return updated;
    });
  }

  function clearRecent() {
    setRecentTracks([]);
    try {
      localStorage.removeItem(RECENT_STORAGE_KEY);
    } catch (e) {
      console.error("Error al borrar historial:", e);
    }
  }

  async function runSearch(q: string) {
    if (!q.trim()) return;
    
    // Cancelar búsqueda anterior
    searchAbortRef.current?.abort();
    const controller = new AbortController();
    searchAbortRef.current = controller;

    setError(null);
    setIsSearching(true);
    try {
      const found = await search(q, 10);
      // Verificar si fue abortado
      if (controller.signal.aborted) return;
      setResults(found);
      setResultsQuery(q);
      setSelectedIndex(null);
    } catch (err) {
      if (err instanceof Error && err.name === "AbortError") return;
      setError(String(err));
    } finally {
      if (!controller.signal.aborted) {
        setIsSearching(false);
      }
    }
  }

  async function handlePlay(track: SearchResult) {
    setError(null);
    setLoadingTrackId(track.id);
    setDragPosition(null);
    seekGuardRef.current = null;
    saveToRecent(track);
    try {
      await play(track);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoadingTrackId(null);
    }
  }

  function handleSearchKeyDown(e: ReactKeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter") {
      e.preventDefault();
      const resultsAreFresh = results.length > 0 && query === resultsQuery;
      if (resultsAreFresh) {
        handlePlay(results[selectedIndex ?? 0]);
      } else {
        runSearch(query);
      }
    } else if (e.key === "ArrowDown") {
      if (results.length === 0) return;
      e.preventDefault();
      setSelectedIndex((current) =>
        current === null ? 0 : Math.min(current + 1, results.length - 1),
      );
    } else if (e.key === "ArrowUp") {
      if (results.length === 0) return;
      e.preventDefault();
      setSelectedIndex((current) =>
        current === null ? 0 : Math.max(current - 1, 0),
      );
    } else if (e.key === "Escape") {
      e.preventDefault();
      setQuery("");
      setResults([]);
      setResultsQuery("");
      setSelectedIndex(null);
    }
  }

  async function handleTogglePlayback() {
    const state = playerStateRef.current;
    if (!state?.current) return;

    const wasPlaying = state.status === "Playing";
    setError(null);
    // Optimista: el icono responde al instante; el evento/estado confirma.
    setPlayerState((prev) =>
      prev ? { ...prev, status: wasPlaying ? "Paused" : "Playing" } : prev,
    );
    try {
      if (wasPlaying) {
        await pause();
      } else {
        await resume();
      }
    } catch (err) {
      setError(String(err));
      getState().then(setPlayerState).catch(console.error);
    }
  }

  async function handleToggleLoop() {
    const state = playerStateRef.current;
    if (!state) return;

    const next = !state.looping;
    setPlayerState((prev) => (prev ? { ...prev, looping: next } : prev));
    try {
      await setLoop(next);
    } catch (err) {
      setError(String(err));
      getState().then(setPlayerState).catch(console.error);
    }
  }

  async function handleStop() {
    setError(null);
    seekGuardRef.current = null;
    try {
      await stop();
      setPlayerState((prev) =>
        prev
          ? { ...prev, status: "Stopped", current: null, position_seconds: 0 }
          : prev,
      );
      setDragPosition(null);
    } catch (err) {
      setError(String(err));
    }
  }

  /** Preview local: el slider se mueve sin saturar el IPC de mpv. */
  function handleVolumePreview(level: number) {
    const volume = Math.round(level);
    if (volume > 0) {
      setPrevVolume(volume);
    }
    setPlayerState((prev) => (prev ? { ...prev, volume } : prev));
  }

  /** Commit al soltar el slider: aquí sí hablamos con mpv. */
  async function handleVolumeCommit(level: number) {
    const volume = Math.round(level);
    handleVolumePreview(volume);
    try {
      await setVolume(volume);
    } catch (err) {
      setError(String(err));
      getState().then(setPlayerState).catch(console.error);
    }
  }

  async function handleToggleMute() {
    const curVol = playerStateRef.current?.volume ?? 100;
    const next = curVol === 0 ? (prevVolumeRef.current > 0 ? prevVolumeRef.current : 100) : 0;
    if (next > 0) {
      setPrevVolume(next);
    }
    setPlayerState((prev) => (prev ? { ...prev, volume: next } : prev));
    try {
      await setVolume(next);
    } catch (err) {
      setError(String(err));
      getState().then(setPlayerState).catch(console.error);
    }
  }

  async function handleSeekCommit(positionSeconds: number) {
    const state = playerStateRef.current;
    const target = safePosition(positionSeconds, state?.duration_seconds ?? 0);

    seekGuardRef.current = { target, at: Date.now() };
    setDragPosition(null);
    setPlayerState((prev) =>
      prev ? { ...prev, position_seconds: target } : prev,
    );
    try {
      await seek(target);
    } catch (err) {
      setError(String(err));
      getState().then(setPlayerState).catch(console.error);
    }
  }

  function handleSeekBy(deltaSeconds: number) {
    const state = playerStateRef.current;
    if (!state?.current) return;
    const target = seekTarget(
      state.position_seconds,
      deltaSeconds,
      state.duration_seconds,
    );
    handleSeekCommit(target);
  }

  const volume = playerState?.volume ?? 100;
  const isPlaying = playerState?.status === "Playing";
  const isLooping = playerState?.looping ?? false;
  const isTrackLoading = loadingTrackId !== null;

  // Memoized derived values
  const searchResults = useMemo(() => results, [results]);
  const recentTracksMemo = useMemo(() => recentTracks, [recentTracks]);

  return (
    <main className="flex h-screen w-screen p-2.5">
      <div className="liquid-glass-window relative flex h-full w-full flex-col gap-3 rounded-[24px] p-4 overflow-hidden">
        {/* Barra superior con arrastre nativo y controles Apple Glass */}
        <header
          data-tauri-drag-region
          className="flex items-center justify-between pt-0.5 pb-1 cursor-grab active:cursor-grabbing select-none"
        >
          <div className="flex items-center gap-2" data-tauri-drag-region>
            <div className="flex items-center gap-1.5">
              <span
                className={cn(
                  "size-2 rounded-full transition-all duration-300",
                  isPlaying
                    ? "bg-teal-400 shadow-[0_0_8px_#2dd4bf] animate-pulse"
                    : "bg-white/20",
                )}
              />
              <span className="text-xs font-semibold tracking-wider uppercase text-foreground/85">
                Cadence
              </span>
            </div>
            {isLooping && (
              <span className="liquid-pill px-2 py-0.5 text-[10px] font-medium text-teal-300">
                Loop
              </span>
            )}
          </div>

          <div className="flex items-center gap-1.5">
            <button
              type="button"
              onClick={() => getCurrentWindow().minimize()}
              aria-label="Minimizar"
              className="flex size-6 items-center justify-center rounded-full bg-white/[0.08] text-white/60 hover:bg-white/[0.15] hover:text-white transition-all"
            >
              <Minus className="size-3" />
            </button>
            <button
              type="button"
              onClick={() => getCurrentWindow().hide()}
              aria-label="Ocultar"
              className="flex size-6 items-center justify-center rounded-full bg-rose-500/20 text-rose-300 hover:bg-rose-500/35 hover:text-white transition-all"
            >
              <X className="size-3" />
            </button>
          </div>
        </header>

        {/* Buscador de vidrio líquido */}
        <div className="relative flex items-center gap-2">
          <div className="relative flex-1">
            <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-white/50" />
            <Input
              ref={searchInputRef}
              value={query}
              onChange={(e) => setQuery(e.currentTarget.value)}
              onKeyDown={handleSearchKeyDown}
              placeholder="Buscar canción, artista o ambiente..."
              autoComplete="off"
              className="h-10 pl-9 pr-9 rounded-xl bg-white/[0.06] backdrop-blur-md border-white/10 hover:border-white/20 focus-visible:border-teal-400/50 focus-visible:ring-2 focus-visible:ring-teal-400/20 text-xs transition-all placeholder:text-white/40"
            />
            {query.length > 0 && (
              <button
                type="button"
                onClick={() => {
                  setQuery("");
                  setResults([]);
                  setResultsQuery("");
                  setSelectedIndex(null);
                  searchInputRef.current?.focus();
                }}
                className="absolute right-3 top-1/2 -translate-y-1/2 rounded-full p-0.5 text-white/40 hover:text-white/90 transition-colors"
                aria-label="Limpiar búsqueda"
              >
                <X className="size-3.5" />
              </button>
            )}
          </div>
          <Button
            type="button"
            size="icon"
            className="h-10 w-10 shrink-0 rounded-xl bg-white/[0.08] hover:bg-white/[0.15] border border-white/10 shadow-sm transition-all"
            aria-label="Buscar"
            onClick={() => runSearch(query)}
            disabled={isSearching}
          >
            {isSearching ? (
              <Loader2 className="size-4 animate-spin text-teal-300" />
            ) : (
              <Search className="size-4 text-white/80" />
            )}
          </Button>
        </div>

        {error && (
          <div className="flex items-center justify-between rounded-lg bg-rose-500/15 border border-rose-500/30 px-3 py-1.5 text-xs text-rose-300" role="alert">
            <span className="flex-1">{error}</span>
            <button
              type="button"
              onClick={() => setError(null)}
              className="text-rose-300/70 hover:text-rose-200 p-0.5 ml-2 transition-colors"
              aria-label="Cerrar mensaje de error"
            >
              <X className="size-3" />
            </button>
          </div>
        )}

        {playerState?.connected === false && (
          <div
            className="flex items-center gap-2 rounded-lg bg-amber-400/15 border border-amber-400/30 px-3 py-1.5 text-xs text-amber-200"
            role="status"
            aria-live="polite"
          >
            <Loader2 className="size-3.5 shrink-0 animate-spin" />
            <span className="flex-1">
              Reconectando con el reproductor… la reproducción se reanudará sola.
            </span>
          </div>
        )}

        {/* Resultados de Búsqueda */}
        {results.length > 0 && (
          <div className="flex min-h-0 flex-1 flex-col gap-2">
            <ScrollArea className="min-h-0 flex-1 pr-1">
              <ol className="flex flex-col gap-1.5">
                {searchResults.map((track, index) => {
                  const isCurrentItemLoading = loadingTrackId === track.id;
                  const isCurrentTrackPlaying = playerState?.current?.id === track.id;

                  return (
                    <SearchResultItem
                      key={track.id}
                      track={track}
                      isSelected={index === selectedIndex}
                      isPlaying={isCurrentTrackPlaying}
                      isLoading={isCurrentItemLoading}
                      onClick={() => handlePlay(track)}
                      onMouseEnter={() => setSelectedIndex(index)}
                    />
                  );
                })}
              </ol>
            </ScrollArea>

            <footer className="flex flex-wrap items-center gap-3 pt-1 text-[11px] text-white/60">
              <span className="flex items-center gap-1">
                <kbd className="rounded bg-white/10 px-1 py-0.2 font-mono text-[10px]">
                  ↑↓
                </kbd>
                Navegar
              </span>
              <span className="flex items-center gap-1">
                <kbd className="rounded bg-white/10 px-1 py-0.2 font-mono text-[10px]">
                  Enter
                </kbd>
                Reproducir
              </span>
              <span className="flex items-center gap-1">
                <kbd className="rounded bg-white/10 px-1 py-0.2 font-mono text-[10px]">
                  Espacio
                </kbd>
                Play/Pausa
              </span>
              <span className="flex items-center gap-1">
                <kbd className="rounded bg-white/10 px-1 py-0.2 font-mono text-[10px]">
                  Esc
                </kbd>
                Limpiar
              </span>
            </footer>
          </div>
        )}

        {/* Historial Reciente (cuando no hay búsqueda activa) */}
        {results.length === 0 && resultsQuery === "" && recentTracksMemo.length > 0 && (
          <div className="flex min-h-0 flex-1 flex-col gap-2">
            <div className="flex items-center justify-between px-1">
              <span className="flex items-center gap-1.5 text-xs font-medium text-white/70">
                <Clock className="size-3.5 text-teal-400" />
                Recientes
              </span>
              <button
                type="button"
                onClick={clearRecent}
                className="text-[10px] text-white/50 hover:text-white/90 transition-colors"
              >
                Limpiar
              </button>
            </div>
            <ScrollArea className="min-h-0 flex-1 pr-1">
              <ol className="flex flex-col gap-1.5">
                {recentTracksMemo.map((track) => {
                  const isCurrentItemLoading = loadingTrackId === track.id;
                  const isCurrentTrackPlaying = playerState?.current?.id === track.id;

                  return (
                    <RecentTrackItem
                      key={track.id}
                      track={track}
                      isPlaying={isCurrentTrackPlaying}
                      isLoading={isCurrentItemLoading}
                      onClick={() => handlePlay(track)}
                    />
                  );
                })}
              </ol>
            </ScrollArea>
          </div>
        )}

        {/* Sin resultados de búsqueda */}
        {!isSearching && resultsQuery !== "" && results.length === 0 && (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center p-6 select-none">
            <div className="size-12 rounded-2xl liquid-pill flex items-center justify-center text-white/50 shadow-md">
              <Search className="size-6 text-white/40" />
            </div>
            <div className="flex flex-col gap-1 max-w-[260px]">
              <p className="text-xs font-semibold text-white/90">Sin resultados</p>
              <p className="text-[11px] text-white/60">
                No se encontró nada para "{resultsQuery}". Intenta con otras palabras clave.
              </p>
            </div>
          </div>
        )}

        {/* Pantalla vacía inicial de concentración */}
        {results.length === 0 && resultsQuery === "" && recentTracksMemo.length === 0 && (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center p-6 select-none">
            <div className="size-14 rounded-2xl liquid-pill flex items-center justify-center text-teal-300/80 shadow-lg">
              <Headphones className="size-7" />
            </div>
            <div className="flex flex-col gap-1">
              <p className="text-sm font-medium text-white/90">Espacio de concentración</p>
              <p className="text-xs text-white/60 max-w-[240px]">
                Busca cualquier tema en YouTube y empieza tu sesión sin distracciones.
              </p>
            </div>
          </div>
        )}

        {/* Tarjeta Flotante Now Playing (Glassmorphism Líquido) */}
        {(isTrackLoading || playerState?.current) && (
          <NowPlaying
            playerState={playerState}
            isTrackLoading={isTrackLoading}
            isPlaying={isPlaying}
            isLooping={isLooping}
            dragPosition={dragPosition}
            onTogglePlayback={handleTogglePlayback}
            onToggleLoop={handleToggleLoop}
            onStop={handleStop}
            onSeekPreview={setDragPosition}
            onSeekCommit={handleSeekCommit}
            onVolumePreview={handleVolumePreview}
            onVolumeCommit={handleVolumeCommit}
            onToggleMute={handleToggleMute}
            volume={volume}
          />
        )}
      </div>
    </main>
  );
}

export default App;