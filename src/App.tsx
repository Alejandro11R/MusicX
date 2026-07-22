import { useState, type ChangeEvent, type FormEvent } from "react";
import {
  search,
  play,
  pause,
  resume,
  stop,
  setVolume,
  type SearchResult,
} from "./lib/tauri";

function App() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [current, setCurrent] = useState<SearchResult | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [volume, setVolumeValue] = useState(100);

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
      setCurrent(track);
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
      setCurrent(null);
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleVolumeChange(e: ChangeEvent<HTMLInputElement>) {
    const level = Number(e.currentTarget.value);
    setVolumeValue(level);
    try {
      await setVolume(level);
    } catch (err) {
      setError(String(err));
    }
  }

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
        {!isLoading && current && <p>{current.title}</p>}
        {!isLoading && !current && <p>(nada)</p>}

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
