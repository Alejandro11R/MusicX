import { useEffect, useState, type ChangeEvent, type FormEvent } from "react";
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

  async function handleVolumeChange(e: ChangeEvent<HTMLInputElement>) {
    try {
      await setVolume(Number(e.currentTarget.value));
    } catch (err) {
      setError(String(err));
    }
  }

  const volume = playerState?.volume ?? 100;

  return (
    <main>
      <h1>Cadence</h1>

      <form onSubmit={handleSearch}>
        <input
          value={query}
          onChange={(e) => setQuery(e.currentTarget.value)}
          placeholder="Buscar..."
        />
        <button type="submit">Buscar</button>
      </form>

      {error && <p>Error: {error}</p>}

      <ol>
        {results.map((track) => (
          <li key={track.id}>
            <button onClick={() => handlePlay(track)} disabled={isLoading}>
              {track.title}
            </button>
          </li>
        ))}
      </ol>

      <section>
        <h2>Reproduciendo</h2>
        {isLoading && <p>Cargando...</p>}
        {!isLoading && (
          <p>
            Estado: {playerState?.status ?? "?"}
            {playerState?.current && ` — ${playerState.current.title}`}
          </p>
        )}

        <button onClick={handlePause}>Pause</button>
        <button onClick={handleResume}>Resume</button>
        <button onClick={handleStop}>Stop</button>

        <div>
          <label htmlFor="volume">Volumen: {volume}</label>
          <input
            id="volume"
            type="range"
            min={0}
            max={100}
            value={volume}
            onChange={handleVolumeChange}
          />
        </div>
      </section>
    </main>
  );
}

export default App;
