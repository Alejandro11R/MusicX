import React, { useState, useRef, useEffect } from "react";
import {
  X,
  RefreshCw,
  Search,
  Music2,
  Play,
  ArrowDown,
  Minus,
  Plus,
  Sparkles,
  SlidersHorizontal,
  Loader2,
} from "lucide-react";
import {
  SearchResult,
  LyricsDoc,
  LyricsCandidate,
  getLyrics,
  searchLyrics,
  getLyricsById,
} from "../../lib/tauri";
import { findActiveLineIndex, formatLyricTime } from "../../lib/lyrics";
import { useLyricClock } from "./useLyricClock";
import { useLyricsScroll } from "./useLyricsScroll";
import { useAlbumPalette } from "../../lib/palette";

interface LyricsPanelProps {
  currentTrack: SearchResult | null;
  lyricsDoc: LyricsDoc | null;
  isLoading: boolean;
  positionSeconds: number;
  isPlaying: boolean;
  speed?: number;
  onClose: () => void;
  onSeek: (seconds: number) => void;
  onLyricsUpdated: (doc: LyricsDoc) => void;
}

export function LyricsPanel({
  currentTrack,
  lyricsDoc,
  isLoading: externalLoading,
  positionSeconds,
  isPlaying,
  speed = 1.0,
  onClose,
  onSeek,
  onLyricsUpdated,
}: LyricsPanelProps) {
  const [internalLoading, setInternalLoading] = useState(false);
  const [userOffsetMs, setUserOffsetMs] = useState(0);
  const [showSearchModal, setShowSearchModal] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [isSearching, setIsSearching] = useState(false);
  const [candidates, setCandidates] = useState<LyricsCandidate[]>([]);
  const [showOffsetControls, setShowOffsetControls] = useState(false);

  const palette = useAlbumPalette(currentTrack?.thumbnail_url);
  const viewportRef = useRef<HTMLDivElement>(null);
  const lineRefs = useRef<(HTMLDivElement | null)[]>([]);

  // Reloj de alta precisión a 60 fps sincronizado con el reproductor
  const interpolatedTime = useLyricClock({
    isPlaying,
    basePosition: positionSeconds,
    speed,
  });

  const lines = lyricsDoc?.lines || [];
  const isSynced = Boolean(lyricsDoc?.synced && lines.length > 0);
  const isInstrumental = Boolean(lyricsDoc?.instrumental);

  // Offset total = offset embebido en el LRC + ajuste fino del usuario
  const totalOffsetMs = (lyricsDoc?.offset_ms || 0) + userOffsetMs;

  const activeLineIndex = isSynced
    ? findActiveLineIndex(lines, interpolatedTime, totalOffsetMs)
    : -1;

  const { isUserScrolling, resumeScroll } = useLyricsScroll({
    activeLineIndex,
    viewportRef,
    lineRefs,
    enabled: isSynced,
  });

  // Resetear offset y búsqueda cuando cambia la canción
  useEffect(() => {
    setUserOffsetMs(0);
    setShowSearchModal(false);
    setCandidates([]);
  }, [currentTrack?.id]);

  const handleRefresh = async () => {
    if (!currentTrack) return;
    setInternalLoading(true);
    try {
      const doc = await getLyrics({
        track_id: currentTrack.id,
        title: currentTrack.title,
        artist: currentTrack.artist,
        duration_seconds: currentTrack.duration_seconds,
        force: true,
      });
      onLyricsUpdated(doc);
    } catch (err) {
      console.error("Error recargando letras:", err);
    } finally {
      setInternalLoading(false);
    }
  };

  const handleOpenSearch = () => {
    if (currentTrack) {
      setSearchQuery(`${currentTrack.artist || ""} ${currentTrack.title}`.trim());
    }
    setShowSearchModal(true);
  };

  const handleSearchSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!currentTrack || !searchQuery.trim()) return;

    setIsSearching(true);
    try {
      const results = await searchLyrics({
        track_id: currentTrack.id,
        title: searchQuery.trim(),
        artist: null,
      });
      setCandidates(results);
    } catch (err) {
      console.error("Error buscando alternativas de letras:", err);
    } finally {
      setIsSearching(false);
    }
  };

  const handleSelectCandidate = async (candidate: LyricsCandidate) => {
    if (!currentTrack) return;
    setInternalLoading(true);
    setShowSearchModal(false);
    try {
      const doc = await getLyricsById(candidate.id, currentTrack.id);
      onLyricsUpdated(doc);
    } catch (err) {
      console.error("Error al cargar letra seleccionada:", err);
    } finally {
      setInternalLoading(false);
    }
  };

  const isLoading = externalLoading || internalLoading;

  return (
    <div
      className="lyrics-container"
      style={
        {
          "--palette-primary": palette.primary,
          "--palette-secondary": palette.secondary,
          "--palette-ambient": palette.ambient,
        } as React.CSSProperties
      }
    >
      {/* Dynamic Ambient Aura */}
      <div className="lyrics-aura-background">
        <div className="lyrics-aura-blob-1" />
        <div className="lyrics-aura-blob-2" />
        <div className="lyrics-aura-blob-3" />
      </div>

      {/* Header Bar */}
      <header className="lyrics-header">
        <div className="lyrics-track-info">
          {currentTrack?.thumbnail_url && (
            <img
              src={currentTrack.thumbnail_url}
              alt=""
              className="lyrics-track-thumb"
            />
          )}
          <div className="lyrics-track-meta">
            <h2 className="lyrics-track-title">
              {currentTrack?.title || "Reproduciendo..."}
            </h2>
            <p className="lyrics-track-artist">
              {currentTrack?.artist || "Artista desconocido"}
            </p>
          </div>
        </div>

        <div className="lyrics-tools">
          {isSynced && (
            <div className="lyrics-pill">
              <Sparkles className="w-3.5 h-3.5 text-indigo-400" />
              <span>Sincronizada</span>
            </div>
          )}

          {/* Toggle Offset Controls */}
          {isSynced && (
            <button
              onClick={() => setShowOffsetControls((prev) => !prev)}
              className="lyrics-btn-icon"
              title="Ajuste fino de sincronización (offset)"
            >
              <SlidersHorizontal className="w-4 h-4" />
            </button>
          )}

          {/* Manual Search Button */}
          <button
            onClick={handleOpenSearch}
            className="lyrics-btn-icon"
            title="Buscar letra alternativa"
          >
            <Search className="w-4 h-4" />
          </button>

          {/* Refresh Button */}
          <button
            onClick={handleRefresh}
            disabled={isLoading}
            className="lyrics-btn-icon"
            title="Forzar recarga de letra"
          >
            <RefreshCw
              className={`w-4 h-4 ${isLoading ? "animate-spin text-indigo-400" : ""}`}
            />
          </button>

          {/* Close Panel Button */}
          <button
            onClick={onClose}
            className="lyrics-btn-icon"
            title="Cerrar letras"
          >
            <X className="size-4" />
          </button>
        </div>
      </header>

      {/* Floating Offset Adjuster Bar */}
      {showOffsetControls && isSynced && (
        <div className="relative z-10 flex items-center justify-center gap-2 py-1.5 px-3 bg-black/40 backdrop-blur-md border-b border-white/5 animate-in fade-in duration-200">
          <span className="text-[11px] text-white/70">Offset:</span>
          <button
            onClick={() => setUserOffsetMs((prev) => prev - 250)}
            className="px-2 py-0.5 text-[11px] rounded bg-white/10 hover:bg-white/20 text-white flex items-center gap-0.5 transition"
          >
            <Minus className="w-3 h-3" /> 250ms
          </button>
          <span
            onClick={() => setUserOffsetMs(0)}
            className="text-[11px] font-mono px-2 py-0.5 rounded bg-white/5 text-white/90 cursor-pointer hover:bg-white/10 transition"
            title="Haz clic para reiniciar a 0"
          >
            {userOffsetMs > 0 ? `+${userOffsetMs}` : userOffsetMs} ms
          </span>
          <button
            onClick={() => setUserOffsetMs((prev) => prev + 250)}
            className="px-2 py-0.5 text-[11px] rounded bg-white/10 hover:bg-white/20 text-white flex items-center gap-0.5 transition"
          >
            <Plus className="w-3 h-3" /> 250ms
          </button>
        </div>
      )}

      {/* Viewport for Lines */}
      <div className="lyrics-viewport" ref={viewportRef}>
        {isLoading ? (
          <div className="lyrics-empty-state">
            <Loader2 className="w-10 h-10 animate-spin text-indigo-400" />
            <p className="lyrics-empty-title">Buscando letra...</p>
            <p className="lyrics-empty-desc">
              Consultando sidecars locales y catálogo en tiempo real
            </p>
          </div>
        ) : isInstrumental ? (
          <div className="lyrics-empty-state">
            <Music2 className="lyrics-empty-icon text-indigo-300 animate-pulse" />
            <p className="lyrics-empty-title">Tema Instrumental</p>
            <p className="lyrics-empty-desc">
              Esta pista musical no contiene parte vocal según el registro.
            </p>
          </div>
        ) : lines.length > 0 ? (
          <div className="lyrics-stack">
            {lines.map((line, idx) => {
              const isPast = isSynced && activeLineIndex > -1 && idx < activeLineIndex;
              const isActive = isSynced && idx === activeLineIndex;
              const isFuture = isSynced && idx > activeLineIndex;

              const stateClass = isActive
                ? "lyric-line-active"
                : isPast
                ? "lyric-line-past"
                : isFuture
                ? "lyric-line-future"
                : "";

              return (
                <div
                  key={`${line.time_ms}_${idx}`}
                  ref={(el) => {
                    lineRefs.current[idx] = el;
                  }}
                  onClick={() => {
                    if (isSynced) {
                      onSeek(line.time_ms / 1000);
                    }
                  }}
                  className={`lyric-line-item ${stateClass}`}
                >
                  {isSynced && (
                    <span className="lyric-line-play-hint">
                      <Play className="w-4 h-4 fill-current text-white/70" />
                    </span>
                  )}
                  <span>{line.text || "•••"}</span>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="lyrics-empty-state">
            <Music2 className="lyrics-empty-icon" />
            <p className="lyrics-empty-title">No hay letra disponible</p>
            <p className="lyrics-empty-desc">
              No se encontró una letra coincidente automáticamente.
            </p>
            <button
              onClick={handleOpenSearch}
              className="mt-3 px-4 py-2 rounded-full bg-white/10 hover:bg-white/20 text-white text-sm font-medium transition flex items-center gap-2"
            >
              <Search className="w-4 h-4" /> Buscar manualmente
            </button>
          </div>
        )}
      </div>

      {/* Floating Button: Resume Auto-Scroll */}
      {isSynced && isUserScrolling && activeLineIndex >= 0 && (
        <button
          onClick={resumeScroll}
          className="lyrics-resume-scroll-btn"
          title="Centrar en la línea activa"
        >
          <ArrowDown className="w-4 h-4 animate-bounce" />
          <span>Volver al punto actual</span>
        </button>
      )}

      {/* Manual Search Modal */}
      {showSearchModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-3 bg-black/70 backdrop-blur-md">
          <div className="w-full max-w-[380px] bg-[#14141e] border border-white/10 rounded-2xl p-4 shadow-2xl flex flex-col max-h-[82vh]">
            <div className="flex items-center justify-between pb-3 border-b border-white/10">
              <h3 className="text-sm font-semibold text-white flex items-center gap-2">
                <Search className="size-4 text-indigo-400" />
                Buscar letra alternativa
              </h3>
              <button
                onClick={() => setShowSearchModal(false)}
                className="p-1 rounded-full text-white/60 hover:text-white hover:bg-white/10 transition"
              >
                <X className="size-4" />
              </button>
            </div>

            <form onSubmit={handleSearchSubmit} className="mt-3 flex gap-2">
              <input
                type="text"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Título o Artista..."
                className="flex-1 bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-xs text-white placeholder-white/40 focus:outline-none focus:border-indigo-500 transition"
                autoFocus
              />
              <button
                type="submit"
                disabled={isSearching}
                className="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white rounded-lg text-xs font-medium transition flex items-center gap-1.5"
              >
                {isSearching ? (
                  <Loader2 className="size-3.5 animate-spin" />
                ) : (
                  <Search className="size-3.5" />
                )}
                Buscar
              </button>
            </form>

            <div className="mt-4 flex-1 overflow-y-auto space-y-2 pr-1">
              {candidates.length > 0 ? (
                candidates.map((cand) => (
                  <div
                    key={cand.id}
                    onClick={() => handleSelectCandidate(cand)}
                    className="p-3 rounded-xl bg-white/5 hover:bg-white/10 border border-white/5 hover:border-indigo-500/50 cursor-pointer transition flex items-center justify-between"
                  >
                    <div>
                      <div className="font-medium text-white text-sm">
                        {cand.title}
                      </div>
                      <div className="text-xs text-white/60">
                        {cand.artist} {cand.album ? `• ${cand.album}` : ""}
                      </div>
                    </div>
                    <div className="flex items-center gap-2">
                      <span className="text-xs font-mono text-white/40">
                        {formatLyricTime(cand.duration_seconds)}
                      </span>
                      {cand.has_synced && (
                        <span className="px-2 py-0.5 text-[10px] font-semibold rounded bg-indigo-500/20 text-indigo-300 border border-indigo-500/30">
                          Sincronizada
                        </span>
                      )}
                    </div>
                  </div>
                ))
              ) : isSearching ? (
                <div className="py-12 text-center text-white/50 text-sm">
                  Buscando coincidencias...
                </div>
              ) : (
                <div className="py-12 text-center text-white/40 text-sm">
                  Introduce el nombre de la canción o artista para buscar en el catálogo
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
