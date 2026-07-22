import { useEffect, useState, type FormEvent } from "react";
import { Pause, Play, Search, Square } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Slider } from "@/components/ui/slider";
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

function App() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [playerState, setPlayerState] = useState<PlayerState | null>(null);

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

  async function handlePause() {
    setError(null);
    try {
      await pause();
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleResume() {
    setError(null);
    try {
      await resume();
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

  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden">
      <Card className="flex h-full flex-col gap-4 rounded-none px-4">
        <header>
          <h1 className="text-lg font-semibold">Cadence</h1>
        </header>

        <form onSubmit={handleSearch} className="flex gap-2">
          <Input
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
                    className="w-full justify-start"
                    onClick={() => handlePlay(track)}
                    disabled={isLoading}
                  >
                    {track.title}
                  </Button>
                </li>
              ))}
            </ol>
          </ScrollArea>
        )}

        {(isLoading || playerState?.current) && (
          <div className="flex flex-col gap-3 border-t border-border pt-4 pb-4">
            {isLoading && (
              <p className="text-sm text-muted-foreground">Cargando...</p>
            )}
            {!isLoading && (
              <p className="flex items-center gap-2 text-sm">
                <span
                  className={`size-1.5 rounded-full ${
                    playerState?.status === "Playing"
                      ? "bg-playing"
                      : "bg-muted-foreground"
                  }`}
                />
                <span>
                  {playerState?.status}
                  {playerState?.current && ` — ${playerState.current.title}`}
                </span>
              </p>
            )}

            <div className="flex gap-2">
              <Button
                variant="outline"
                size="icon"
                onClick={handlePause}
                aria-label="Pause"
              >
                <Pause />
              </Button>
              <Button
                variant="outline"
                size="icon"
                onClick={handleResume}
                aria-label="Resume"
              >
                <Play />
              </Button>
              <Button
                variant="outline"
                size="icon"
                onClick={handleStop}
                aria-label="Stop"
              >
                <Square />
              </Button>
            </div>

            <div className="flex items-center gap-2">
              <span className="text-xs text-muted-foreground">Vol</span>
              <Slider
                value={[volume]}
                max={100}
                step={1}
                onValueChange={([level]) => handleVolumeChange(level)}
              />
              <span className="w-8 text-right text-xs text-muted-foreground">
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
