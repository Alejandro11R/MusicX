import { useEffect, useRef, useState, type FormEvent } from "react";
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
  getState,
  type PlayerState,
  type SearchResult,
} from "./lib/tauri";

const POLL_INTERVAL_MS = 1000;

function formatDuration(totalSeconds: number): string {
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = Math.floor(totalSeconds % 60);
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

function App() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [playerState, setPlayerState] = useState<PlayerState | null>(null);
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

  async function handleSearch(e: FormEvent) {
    e.preventDefault();
    setError(null);
    try {
      setResults(await search(query, 10));
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

  const volume = playerState?.volume ?? 100;
  const isPlaying = playerState?.status === "Playing";

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden">
      <Card className="flex h-full flex-col gap-4 rounded-none px-4">
        <header>
          <h1 className="text-lg font-semibold tracking-tight">Cadence</h1>
        </header>

        <form onSubmit={handleSearch} className="flex gap-2">
          <Input
            ref={searchInputRef}
            value={query}
            onChange={(e) => setQuery(e.currentTarget.value)}
            placeholder="Buscar..."
          />
          <Button type="submit" size="icon" aria-label="Buscar">
            <Search />
          </Button>
        </form>

        {error && <p className="text-sm text-destructive">{error}</p>}

        {results.length > 0 && (
          <ScrollArea className="min-h-0 flex-1">
            <ol className="flex flex-col gap-1">
              {results.map((track) => (
                <li key={track.id}>
                  <Button
                    variant="ghost"
                    className="h-auto w-full justify-between gap-3 px-3 py-2"
                    onClick={() => handlePlay(track)}
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
