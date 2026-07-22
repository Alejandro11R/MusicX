import { useState, type FormEvent } from "react";
import { search, play, type SearchResult } from "./lib/tauri";

function App() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [error, setError] = useState<string | null>(null);

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
    try {
      await play(track);
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
            <button onClick={() => handlePlay(track)}>{track.title}</button>
          </li>
        ))}
      </ol>
    </main>
  );
}

export default App;
