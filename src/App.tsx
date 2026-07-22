import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { Pause, Play, Search, Square, Volume2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
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
  getState,
  quit,
  type PlayerState,
  type SearchResult,
} from "./lib/tauri";

// Fast enough for the progress bar to read as live rather than ticking.
const POLL_INTERVAL_MS = 250;

function formatDuration(totalSeconds: number): string {
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = Math.floor(totalSeconds % 60);
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

function App() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  // The query that produced `results`, so a stale list (query edited since
  // the last search) never gets treated as "ready to play from".
  const [resultsQuery, setResultsQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [playerState, setPlayerState] = useState<PlayerState | null>(null);
  // While the user is dragging the progress thumb, the 250ms poll must not
  // fight the drag — this holds the in-progress value instead, and is
  // cleared once the seek is committed so polling takes back over.
  const [dragPosition, setDragPosition] = useState<number | null>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    searchInputRef.current?.focus();
  }, []);

  useEffect(() => {
    const poll = async () => {
      try {
        setPlayerState(await getState());
      } catch (err) {
        // A failed background poll shouldn't spam the visible error banner.
        console.error("state() poll failed:", err);
      }
    };
    poll();
    const interval = setInterval(poll, POLL_INTERVAL_MS);
    return () => clearInterval(interval);
  }, []);

  // "/" and Ctrl/Cmd+K refocus search from anywhere, like Raycast/Spotlight.
  // "/" only fires when NOT already typing in the search box, so it can
  // still be typed as a literal character in a query. Ctrl/Cmd+Q actually
  // ends the process — closing the window only hides it (see lib.rs), so
  // this is the one explicit way out, same as e.g. Discord/Telegram.
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
      }
    }
    document.addEventListener("keydown", handleGlobalKeyDown);
    return () => document.removeEventListener("keydown", handleGlobalKeyDown);
  }, []);

  async function runSearch(q: string) {
    setError(null);
    try {
      const found = await search(q, 10);
      setResults(found);
      setResultsQuery(q);
      setSelectedIndex(null);
    } catch (err) {
      setError(String(err));
    }
  }

  async function handlePlay(track: SearchResult) {
    setError(null);
    setIsLoading(true);
    try {
      await play(track);
    } catch (err) {
      setError(String(err));
    } finally {
      setIsLoading(false);
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

  async function handleStop() {
    setError(null);
    try {
      await stop();
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleVolumeChange(level: number) {
    try {
      await setVolume(level);
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
  const duration = playerState?.duration_seconds ?? 0;
  const position = dragPosition ?? playerState?.position_seconds ?? 0;

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden">
      <Card className="flex h-full flex-col gap-4 rounded-none px-4">
        <header>
          <h1 className="text-lg font-semibold tracking-tight">Cadence</h1>
        </header>

        <div className="flex gap-2">
          <Input
            ref={searchInputRef}
            value={query}
            onChange={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={handleSearchKeyDown}
            placeholder="Buscar..."
            autoComplete="off"
          />
          <Button
            type="button"
            size="icon"
            aria-label="Buscar"
            onClick={() => runSearch(query)}
          >
            <Search />
          </Button>
        </div>

        {error && <p className="text-sm text-destructive">{error}</p>}

        {results.length > 0 && (
          <>
            <ScrollArea className="min-h-0 flex-1">
              <ol className="flex flex-col gap-1">
                {results.map((track, index) => (
                  <li key={track.id}>
                    <Button
                      variant="ghost"
                      className={cn(
                        "h-auto w-full justify-between gap-3 px-3 py-2",
                        index === selectedIndex && "bg-muted text-foreground",
                      )}
                      onClick={() => handlePlay(track)}
                      onMouseEnter={() => setSelectedIndex(index)}
                      disabled={isLoading}
                    >
                      <span className="truncate text-sm">{track.title}</span>
                      <span className="shrink-0 text-xs tabular-nums text-muted-foreground">
                        {formatDuration(track.duration_seconds)}
                      </span>
                    </Button>
                  </li>
                ))}
              </ol>
            </ScrollArea>

            <p className="flex items-center gap-3 text-xs text-muted-foreground">
              <span className="flex items-center gap-1">
                <kbd className="rounded border border-border px-1 py-0.5 font-sans">
                  ↑↓
                </kbd>
                Navegar
              </span>
              <span className="flex items-center gap-1">
                <kbd className="rounded border border-border px-1 py-0.5 font-sans">
                  Enter
                </kbd>
                Reproducir
              </span>
              <span className="flex items-center gap-1">
                <kbd className="rounded border border-border px-1 py-0.5 font-sans">
                  Esc
                </kbd>
                Limpiar
              </span>
            </p>
          </>
        )}

        {(isLoading || playerState?.current) && (
          <div className="flex flex-col gap-4 border-t border-border pt-4 pb-4">
            {isLoading && (
              <p className="text-sm text-muted-foreground">Cargando...</p>
            )}
            {!isLoading && playerState?.current && (
              <div className="flex flex-col gap-0.5">
                <div className="flex items-center gap-2">
                  <span
                    className={cn(
                      "size-2 shrink-0 rounded-full transition-colors",
                      isPlaying ? "bg-playing" : "bg-muted-foreground",
                    )}
                  />
                  <p className="truncate text-base font-semibold">
                    {playerState.current.title}
                  </p>
                </div>
                {playerState.current.artist && (
                  <p className="truncate pl-4 text-sm text-muted-foreground">
                    {playerState.current.artist}
                  </p>
                )}
              </div>
            )}

            {!isLoading && playerState?.current && (
              <div className="flex items-center gap-2">
                <span className="w-9 shrink-0 text-right text-xs tabular-nums text-muted-foreground">
                  {formatDuration(position)}
                </span>
                <Slider
                  className="flex-1"
                  value={[position]}
                  max={Math.max(duration, 1)}
                  step={1}
                  onValueChange={([v]) => setDragPosition(v)}
                  onValueCommit={([v]) => handleSeekCommit(v)}
                />
                <span className="w-9 shrink-0 text-xs tabular-nums text-muted-foreground">
                  {formatDuration(duration)}
                </span>
              </div>
            )}

            <div className="flex items-center justify-center gap-3">
              <Button
                size="icon-lg"
                onClick={handleTogglePlayback}
                aria-label={isPlaying ? "Pause" : "Play"}
              >
                {isPlaying ? <Pause /> : <Play />}
              </Button>
              <Button
                variant="ghost"
                size="icon"
                onClick={handleStop}
                aria-label="Stop"
              >
                <Square />
              </Button>
            </div>

            <div className="flex items-center gap-2">
              <Volume2 className="size-4 shrink-0 text-muted-foreground" />
              <Slider
                className="flex-1"
                value={[volume]}
                max={100}
                step={1}
                onValueChange={([level]) => handleVolumeChange(level)}
              />
              <span className="w-8 shrink-0 text-right text-xs tabular-nums text-muted-foreground">
                {volume}
              </span>
            </div>
          </div>
        )}
      </Card>
    </div>
  );
}

export default App;
