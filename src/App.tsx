import {
  useEffect,
  useRef,
  useState,
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
  type PlayerState,
  type SearchResult,
} from "./lib/tauri";

const POLL_INTERVAL_MS = 250;
const RECENT_STORAGE_KEY = "cadence_recent_tracks";

function formatDuration(totalSeconds: number): string {
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = Math.floor(totalSeconds % 60);
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

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

    health()
      .then((h) => {
        if (!h.mpv) {
          setError("mpv no está disponible en el sistema.");
        } else if (!h.yt_dlp) {
          setError("yt-dlp no está disponible en el sistema.");
        }
      })
      .catch((err) => console.error("health() check failed:", err));
  }, []);

  useEffect(() => {
    const poll = async () => {
      try {
        setPlayerState(await getState());
      } catch (err) {
        console.error("state() poll failed:", err);
      }
    };
    poll();
    const interval = setInterval(poll, POLL_INTERVAL_MS);
    return () => clearInterval(interval);
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
        const cur = playerStateRef.current?.position_seconds ?? 0;
        seek(Math.max(0, cur - 5)).catch(() => {});
      } else if (!isSearchFocused && e.key === "ArrowRight") {
        e.preventDefault();
        const cur = playerStateRef.current?.position_seconds ?? 0;
        const dur = playerStateRef.current?.duration_seconds ?? 0;
        seek(Math.min(dur, cur + 5)).catch(() => {});
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
    setError(null);
    setIsSearching(true);
    try {
      const found = await search(q, 10);
      setResults(found);
      setResultsQuery(q);
      setSelectedIndex(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setIsSearching(false);
    }
  }

  async function handlePlay(track: SearchResult) {
    setError(null);
    setLoadingTrackId(track.id);
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
    setError(null);
    try {
      if (playerState?.status === "Playing") {
        await pause();
      } else {
        await resume();
      }
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleToggleLoop() {
    if (!playerState) return;
    try {
      await setLoop(!playerState.looping);
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleStop() {
    setError(null);
    try {
      await stop();
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleVolumeChange(level: number) {
    if (level > 0) {
      setPrevVolume(level);
    }
    try {
      await setVolume(level);
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleToggleMute() {
    const curVol = playerStateRef.current?.volume ?? 100;
    try {
      if (curVol === 0) {
        const restore = prevVolumeRef.current > 0 ? prevVolumeRef.current : 100;
        await setVolume(restore);
      } else {
        setPrevVolume(curVol);
        await setVolume(0);
      }
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleSeekCommit(positionSeconds: number) {
    try {
      await seek(positionSeconds);
    } catch (err) {
      setError(String(err));
    } finally {
      setDragPosition(null);
    }
  }

  const volume = playerState?.volume ?? 100;
  const isPlaying = playerState?.status === "Playing";
  const isLooping = playerState?.looping ?? false;
  const duration = playerState?.duration_seconds ?? 0;
  const position = dragPosition ?? playerState?.position_seconds ?? 0;
  const isTrackLoading = loadingTrackId !== null;

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

        {/* Resultados de Búsqueda */}
        {results.length > 0 && (
          <div className="flex min-h-0 flex-1 flex-col gap-2">
            <ScrollArea className="min-h-0 flex-1 pr-1">
              <ol className="flex flex-col gap-1.5">
                {results.map((track, index) => {
                  const isCurrentItemLoading = loadingTrackId === track.id;
                  const isCurrentTrackPlaying = playerState?.current?.id === track.id;

                  return (
                    <li key={track.id}>
                      <button
                        type="button"
                        className={cn(
                          "group relative flex w-full items-center gap-3 rounded-xl p-2 text-left transition-all duration-200",
                          "bg-white/[0.02] hover:bg-white/[0.08] border border-transparent hover:border-white/10",
                          index === selectedIndex && "bg-white/[0.09] border-white/15",
                          isCurrentTrackPlaying &&
                            "bg-teal-500/10 border-teal-500/30 shadow-[0_0_15px_-3px_rgba(45,212,191,0.2)]",
                        )}
                        onClick={() => handlePlay(track)}
                        onMouseEnter={() => setSelectedIndex(index)}
                        disabled={isTrackLoading}
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
                          {isCurrentItemLoading && (
                            <div className="absolute inset-0 flex items-center justify-center bg-black/70 backdrop-blur-xs">
                              <Loader2 className="size-4 animate-spin text-teal-300" />
                            </div>
                          )}
                        </div>
                        <div className="flex min-w-0 flex-1 flex-col justify-center">
                          <span
                            className={cn(
                              "truncate text-xs font-medium leading-tight",
                              isCurrentTrackPlaying ? "text-teal-300" : "text-white/90",
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
        {results.length === 0 && resultsQuery === "" && recentTracks.length > 0 && (
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
                {recentTracks.map((track) => {
                  const isCurrentItemLoading = loadingTrackId === track.id;
                  const isCurrentTrackPlaying = playerState?.current?.id === track.id;

                  return (
                    <li key={track.id}>
                      <button
                        type="button"
                        className={cn(
                          "group relative flex w-full items-center gap-3 rounded-xl p-2 text-left transition-all duration-200",
                          "bg-white/[0.02] hover:bg-white/[0.08] border border-transparent hover:border-white/10",
                          isCurrentTrackPlaying &&
                            "bg-teal-500/10 border-teal-500/30 shadow-[0_0_15px_-3px_rgba(45,212,191,0.2)]",
                        )}
                        onClick={() => handlePlay(track)}
                        disabled={isTrackLoading}
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
                          {isCurrentItemLoading && (
                            <div className="absolute inset-0 flex items-center justify-center bg-black/70 backdrop-blur-xs">
                              <Loader2 className="size-4 animate-spin text-teal-300" />
                            </div>
                          )}
                        </div>
                        <div className="flex min-w-0 flex-1 flex-col justify-center">
                          <span
                            className={cn(
                              "truncate text-xs font-medium leading-tight",
                              isCurrentTrackPlaying ? "text-teal-300" : "text-white/95",
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
        {results.length === 0 && resultsQuery === "" && recentTracks.length === 0 && (
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
          <div className="liquid-card mt-auto flex flex-col gap-3 rounded-2xl p-3 border border-white/10 shadow-xl" role="region" aria-label="Reproductor">
            {isTrackLoading && !playerState?.current && (
              <div className="flex items-center gap-2 py-1 text-xs text-teal-300" role="status">
                <Loader2 className="size-3.5 animate-spin" />
                <span>Cargando pista de audio...</span>
              </div>
            )}

            {playerState?.current && (
              <div className="flex items-center gap-3">
                <div
                  className={cn(
                    "size-12 shrink-0 overflow-hidden rounded-xl bg-white/10 border border-white/15 transition-all duration-300",
                    isPlaying && "shadow-[0_4px_20px_-2px_rgba(45,212,191,0.35)]",
                  )}
                >
                  <img
                    src={playerState.current.thumbnail_url}
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
                      {playerState.current.title}
                    </p>
                  </div>
                  {playerState.current.artist && (
                    <p className="truncate pl-3 text-[11px] text-white/60">
                      {playerState.current.artist}
                    </p>
                  )}
                </div>
              </div>
            )}

            {playerState?.current && (
              <div className="flex items-center gap-2 px-0.5">
                <span className="w-8 shrink-0 text-right text-[10px] tabular-nums text-white/60">
                  {formatDuration(position)}
                </span>
                <Slider
                  className="flex-1"
                  value={[position]}
                  max={Math.max(duration, 1)}
                  step={1}
                  onValueChange={([v]) => setDragPosition(v)}
                  onValueCommit={([v]) => handleSeekCommit(v)}
                  aria-label="Posición de reproducción"
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
                onClick={handleToggleLoop}
                aria-label={isLooping ? "Desactivar bucle (L)" : "Activar bucle (L)"}
                title={isLooping ? "Bucle activo (L)" : "Bucle inactivo (L)"}
              >
                <Repeat className={cn("size-3.5", isLooping && "stroke-[2.5]")} />
              </Button>

              <Button
                size="icon"
                className="size-10 rounded-full bg-gradient-to-br from-teal-400 to-emerald-500 text-slate-950 shadow-[0_0_18px_rgba(45,212,191,0.45)] hover:shadow-[0_0_24px_rgba(45,212,191,0.65)] hover:scale-105 active:scale-95 transition-all"
                onClick={handleTogglePlayback}
                aria-label={isPlaying ? "Pausar (Espacio)" : "Reproducir (Espacio)"}
                title={isPlaying ? "Pausar (Espacio)" : "Reproducir (Espacio)"}
              >
                {isPlaying ? <Pause className="size-4 fill-current" /> : <Play className="size-4 ml-0.5 fill-current" />}
              </Button>

              <Button
                variant="ghost"
                size="icon"
                className="size-8 rounded-full bg-white/[0.04] border border-white/5 text-white/60 hover:bg-white/[0.1] hover:text-white transition-all"
                onClick={handleStop}
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
                onClick={handleToggleMute}
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
                onValueChange={([level]) => handleVolumeChange(level)}
                aria-label="Volumen"
              />
              <span className="w-6 shrink-0 text-right text-[10px] tabular-nums text-white/60">
                {volume}
              </span>
            </div>
          </div>
        )}
      </div>
    </main>
  );
}

export default App;
