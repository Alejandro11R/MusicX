import React, { useEffect, useRef, useState } from "react";
import {
  X,
  Play,
  Pause,
  SkipBack,
  SkipForward,
  Repeat,
  Volume2,
  VolumeX,
  Headphones,
  ArrowDown,
  Sparkles,
  Search,
  RefreshCw,
  Music2,
  Loader2,
  Share2,
} from "lucide-react";
import {
  SearchResult,
  PlayerState,
  LyricsDoc,
  AudioPreset,
  LyricsCandidate,
  getLyrics,
  searchLyrics,
  getLyricsById,
} from "../../lib/tauri";
import { safeDuration, safePosition, formatDuration } from "../../lib/playback";
import { findActiveLineIndex, getWordProgress } from "../../lib/lyrics";
import { useLyricClock } from "../lyrics/useLyricClock";
import { useLyricsScroll } from "../lyrics/useLyricsScroll";
import { useAlbumPalette } from "../../lib/palette";
import { Slider } from "../ui/slider";
import { InteractiveTimeline } from "../ui/InteractiveTimeline";
import { LyricCardModal } from "../lyrics/LyricCardModal";
import { LyricsOwnership, requestInView } from "../../lib/async-ownership";
import { cn } from "../../lib/utils";

interface ImmersivePlayerProps {
  currentTrack: SearchResult | null;
  playerState: PlayerState | null;
  lyricsDoc: LyricsDoc | null;
  isLyricsLoading: boolean;
  isPlaying: boolean;
  isLooping: boolean;
  dragPosition: number | null;
  volume: number;
  onTogglePlayback: () => void;
  onToggleLoop: () => void;
  onPrevious: () => void;
  onNext: () => void;
  onSeekPreview: (seconds: number) => void;
  onSeekCommit: (seconds: number) => void;
  onVolumePreview: (level: number) => void;
  onVolumeCommit: (level: number) => void;
  onToggleMute: () => void;
  onSelectPreset?: (preset: AudioPreset) => void;
  onClose: () => void;
  requestLyrics: (load: () => Promise<LyricsDoc>, alive: () => boolean, trackId: string) => Promise<void>;
}

const IMMERSIVE_AUDIO_PRESETS: { id: AudioPreset; label: string; icon: string }[] = [
  { id: "normal", label: "Normal", icon: "✨" },
  { id: "lofi", label: "Lo-Fi", icon: "☕" },
  { id: "bass_boost", label: "Bass", icon: "🔊" },
  { id: "loudnorm", label: "Loudnorm", icon: "⚖️" },
  { id: "slowed_reverb", label: "Slowed", icon: "🌙" },
  { id: "nightcore", label: "Nightcore", icon: "⚡" },
];

export function ImmersivePlayer({
  currentTrack,
  playerState,
  lyricsDoc,
  isLyricsLoading: externalLoading,
  isPlaying,
  isLooping,
  dragPosition,
  volume,
  onTogglePlayback,
  onToggleLoop,
  onPrevious,
  onNext,
  onSeekPreview,
  onSeekCommit,
  onVolumePreview,
  onVolumeCommit,
  onToggleMute,
  onSelectPreset,
  onClose,
  requestLyrics,
}: ImmersivePlayerProps) {
  const palette = useAlbumPalette(currentTrack?.thumbnail_url);
  const viewportRef = useRef<HTMLDivElement>(null);
  const lineRefs = useRef<(HTMLDivElement | null)[]>([]);
  const [showVolumePopup, setShowVolumePopup] = useState(false);

  // Estados para búsqueda y recarga de letras
  const [internalLoading, setInternalLoading] = useState(false);
  const [showSearchModal, setShowSearchModal] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [isSearching, setIsSearching] = useState(false);
  const [candidates, setCandidates] = useState<LyricsCandidate[]>([]);
  const [showLyricCardModal, setShowLyricCardModal] = useState(false);

  const requests = useRef(new LyricsOwnership<void>(setInternalLoading));
  const searches = useRef(new LyricsOwnership<LyricsCandidate[]>(setIsSearching));
  useEffect(() => {
    requests.current.track(currentTrack?.id ?? null);
    searches.current.track(currentTrack?.id ?? null);
    setCandidates([]);
    setShowSearchModal(false);
    return () => { requests.current.invalidate(); searches.current.invalidate(); };
  }, [currentTrack?.id]);
  const closeSearch = () => { searches.current.invalidate(); setCandidates([]); setShowSearchModal(false); };

  const duration = safeDuration(
    playerState?.duration_seconds && playerState.duration_seconds > 0
      ? playerState.duration_seconds
      : currentTrack?.duration_seconds ?? 0
  );
  const rawPosition = dragPosition ?? playerState?.position_seconds ?? 0;
  const position = safePosition(rawPosition, duration);
  const remaining = Math.max(0, duration - position);

  // Sincronización lírica a 60 fps
  const speed = playerState?.speed ?? 1.0;
  const interpolatedTime = useLyricClock({
    isPlaying,
    basePosition: playerState?.position_seconds ?? 0,
    speed,
  });

  const lines = lyricsDoc?.lines || [];
  const isSynced = Boolean(lyricsDoc?.synced && lines.length > 0);
  const totalOffsetMs = lyricsDoc?.offset_ms || 0;
  const currentMs = Math.max(0, Math.floor(interpolatedTime * 1000) + totalOffsetMs);

  const activeLineIndex = isSynced
    ? findActiveLineIndex(lines, interpolatedTime, totalOffsetMs)
    : -1;

  const { isUserScrolling, resumeScroll } = useLyricsScroll({
    activeLineIndex,
    viewportRef,
    lineRefs,
    enabled: isSynced,
  });

  const handleRefreshLyrics = async () => {
    if (!currentTrack) return;
    await requestInView(requests.current, requestLyrics, () => getLyrics({
        track_id: currentTrack.id,
        title: currentTrack.title,
        artist: currentTrack.artist,
        duration_seconds: currentTrack.duration_seconds,
        force: true,
      }), currentTrack.id);
  };

  const handleOpenSearch = () => {
    if (currentTrack) {
      // Limpiar ruido para sugerir una búsqueda efectiva
      const query = currentTrack.title
        .replace(/\(cover.*?\)/gi, "")
        .replace(/\(official.*?\)/gi, "")
        .replace(/\[.*?\]/g, "")
        .trim();
      setSearchQuery(query);
    }
    setShowSearchModal(true);
  };

  const handleSearchSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!currentTrack || !searchQuery.trim()) return;

    await searches.current.run(() => searchLyrics({
        track_id: currentTrack.id,
        title: searchQuery.trim(),
        artist: null,
      }), setCandidates);
  };

  const handleSelectCandidate = async (candidate: LyricsCandidate) => {
    if (!currentTrack) return;
    closeSearch();
    await requestInView(requests.current, requestLyrics,
      () => getLyricsById(candidate.id, currentTrack.id), currentTrack.id);
  };

  return (
    <div
      className="fixed inset-0 z-50 flex flex-col justify-between overflow-hidden select-none animate-in fade-in duration-300"
      style={
        {
          "--palette-primary": palette.primary,
          "--palette-secondary": palette.secondary,
          "--palette-ambient": palette.ambient,
          backgroundColor: "#0d0b0d",
        } as React.CSSProperties
      }
      role="dialog"
      aria-modal="true"
      aria-label="Reproductor Inmersivo Apple Music"
    >
      {/* Fondo Atmosférico Líquido Reactivo al Álbum */}
      <div className="absolute inset-0 pointer-events-none z-0 overflow-hidden">
        <div
          className="absolute -top-[20%] -left-[10%] w-[85vw] h-[85vh] rounded-full blur-[100px] opacity-60 animate-pulse duration-[8000ms]"
          style={{
            background: `radial-gradient(circle, ${palette.primary} 0%, transparent 70%)`,
          }}
        />
        <div
          className="absolute -bottom-[20%] -right-[10%] w-[80vw] h-[80vh] rounded-full blur-[110px] opacity-50"
          style={{
            background: `radial-gradient(circle, ${palette.secondary} 0%, transparent 70%)`,
          }}
        />
        <div
          className="absolute top-[30%] right-[20%] w-[60vw] h-[60vh] rounded-full blur-[120px] opacity-40"
          style={{
            background: `radial-gradient(circle, ${palette.ambient} 0%, transparent 65%)`,
          }}
        />
        {/* Viñeta suave para contraste */}
        <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-transparent to-black/60" />
      </div>

      {/* Barra superior de controles mínimos */}
      <header className="relative z-20 flex items-center justify-between px-6 py-3.5 sm:px-8">
        <button
          type="button"
          onClick={onClose}
          className="size-9 sm:size-10 rounded-full bg-white/10 hover:bg-white/20 active:scale-95 border border-white/15 backdrop-blur-md text-white/80 hover:text-white flex items-center justify-center transition-all cursor-pointer shadow-lg"
          title="Salir del modo inmersivo (Esc)"
          aria-label="Cerrar vista inmersiva"
        >
          <X className="size-4 sm:size-5" />
        </button>

        <div className="flex items-center gap-2">
          {isSynced && (
            <div className="hidden sm:flex items-center gap-1.5 px-3 py-1 rounded-full bg-white/10 border border-white/15 backdrop-blur-md text-xs font-medium text-white/85 shadow-md">
              <Sparkles className="size-3.5 text-amber-300" />
              <span>Letras en vivo</span>
            </div>
          )}

          <button
            type="button"
            onClick={handleOpenSearch}
            className="size-9 rounded-full bg-white/10 hover:bg-white/20 active:scale-95 border border-white/15 backdrop-blur-md text-white/80 hover:text-white flex items-center justify-center transition-all cursor-pointer shadow-sm"
            title="Buscar letra alternativa"
          >
            <Search className="size-3.5" />
          </button>

          <button
            type="button"
            onClick={handleRefreshLyrics}
            className="size-9 rounded-full bg-white/10 hover:bg-white/20 active:scale-95 border border-white/15 backdrop-blur-md text-white/80 hover:text-white flex items-center justify-center transition-all cursor-pointer shadow-sm"
            title="Recargar letras"
          >
            <RefreshCw className={cn("size-3.5", internalLoading && "animate-spin")} />
          </button>

          {lines.length > 0 && (
            <button
              type="button"
              onClick={() => setShowLyricCardModal(true)}
              className="size-9 rounded-full bg-white/10 hover:bg-white/20 active:scale-95 border border-white/15 backdrop-blur-md text-white/80 hover:text-teal-300 flex items-center justify-center transition-all cursor-pointer shadow-sm"
              title="Generar Lyric Card estética para compartir"
            >
              <Share2 className="size-3.5" />
            </button>
          )}
        </div>
      </header>

      {/* Contenido Central: Simetría 50 / 50 estilo Apple Music */}
      <main className="relative z-10 flex-1 min-h-0 w-full max-w-6xl mx-auto px-6 md:px-10 grid grid-cols-1 md:grid-cols-2 gap-6 md:gap-12 items-center justify-center">
        {/* Columna Izquierda: Carátula Héroe + Metadata + Transporte */}
        <div className="flex flex-col items-center md:items-start justify-center max-w-md mx-auto w-full gap-3 sm:gap-4 my-auto">
          {/* Carátula Grande en Alta Definición que escala según la altura de pantalla */}
          <div className="relative size-44 sm:size-56 md:size-64 lg:size-72 max-h-[35vh] max-w-[35vh] shrink-0 overflow-hidden rounded-2xl border border-white/20 shadow-[0_20px_45px_-10px_rgba(0,0,0,0.9)] transition-all duration-300">
            {currentTrack?.thumbnail_url ? (
              <img
                src={currentTrack.thumbnail_url}
                alt=""
                className="size-full object-cover"
              />
            ) : (
              <div className="size-full flex items-center justify-center bg-teal-500/10">
                <Headphones className="size-16 text-teal-300" />
              </div>
            )}
          </div>

          {/* Metadata de la pista */}
          <div className="w-full flex flex-col gap-0.5 text-center md:text-left">
            <div className="flex items-center justify-center md:justify-start gap-2 flex-wrap">
              <h1 className="text-lg sm:text-xl md:text-2xl font-bold text-white tracking-tight truncate max-w-full">
                {currentTrack?.title || "Sin reproducción"}
              </h1>
              {lyricsDoc?.source && lyricsDoc.source !== "none" && (
                <span className="px-1.5 py-0.5 rounded text-[10px] font-semibold uppercase tracking-wide bg-white/15 text-white/85 border border-white/15 shrink-0">
                  {lyricsDoc.source === "lrclib"
                    ? "LRCLib"
                    : lyricsDoc.source === "local_lrc"
                    ? "LRC"
                    : "ID3"}
                </span>
              )}
              {speed !== 1.0 && (
                <span className="px-1.5 py-0.5 rounded text-[10px] font-mono font-semibold bg-teal-400/20 text-teal-200 border border-teal-400/30 shrink-0">
                  {speed.toFixed(2)}x
                </span>
              )}
            </div>
            <p className="text-xs sm:text-sm font-medium text-white/60 truncate">
              {currentTrack?.artist || "Cadence"}
            </p>
          </div>

          {/* Barra de progreso interactiva con hover preview de timestamp y tiempo restante */}
          <div className="w-full flex flex-col gap-0.5 pt-0.5">
            <InteractiveTimeline
              position={position}
              duration={duration}
              onSeekPreview={onSeekPreview}
              onSeekCommit={onSeekCommit}
              showLabels={false}
            />
            <div className="flex items-center justify-between text-[11px] tabular-nums text-white/50 font-medium px-0.5">
              <span>{formatDuration(position)}</span>
              <span>-{formatDuration(remaining)}</span>
            </div>
          </div>

          {/* Botonera de Transporte Centrada y Simétrica */}
          <div className="w-full flex items-center justify-center md:justify-start gap-3 sm:gap-4 pt-1 sm:pt-2">
            <button
              type="button"
              onClick={onToggleLoop}
              className={cn(
                "p-2.5 rounded-full border transition-all cursor-pointer",
                isLooping
                  ? "bg-white/25 text-white border-white/40 shadow-sm"
                  : "border-transparent text-white/60 hover:text-white hover:bg-white/10"
              )}
              title={isLooping ? "Bucle activo" : "Repetir"}
            >
              <Repeat className="size-4" />
            </button>

            <button
              type="button"
              onClick={onPrevious}
              className="p-2.5 rounded-full text-white/70 hover:text-white hover:bg-white/10 transition-all cursor-pointer active:scale-95"
              title="Pista anterior (P)"
            >
              <SkipBack className="size-6 fill-current" />
            </button>

            {/* Botón Play Sobredimensionado Blanco Sólido */}
            <button
              type="button"
              onClick={onTogglePlayback}
              className="size-13 sm:size-15 md:size-16 rounded-full bg-white text-black hover:scale-105 active:scale-95 shadow-[0_8px_28px_rgba(255,255,255,0.4)] transition-all flex items-center justify-center cursor-pointer shrink-0 z-20"
              title={isPlaying ? "Pausar (Espacio)" : "Reproducir (Espacio)"}
            >
              {isPlaying ? (
                <Pause className="size-6 sm:size-7 fill-black text-black" />
              ) : (
                <Play className="size-6 sm:size-7 fill-black text-black ml-0.5" />
              )}
            </button>

            <button
              type="button"
              onClick={onNext}
              className="p-2.5 rounded-full text-white/70 hover:text-white hover:bg-white/10 transition-all cursor-pointer active:scale-95"
              title="Siguiente pista (N)"
            >
              <SkipForward className="size-6 fill-current" />
            </button>

            {/* Control rápido de volumen en popover */}
            <div className="relative">
              <button
                type="button"
                onClick={() => setShowVolumePopup((prev) => !prev)}
                className="p-2.5 rounded-full text-white/60 hover:text-white hover:bg-white/10 transition-all cursor-pointer"
                title="Volumen"
              >
                {volume === 0 ? (
                  <VolumeX className="size-4 text-rose-300" />
                ) : (
                  <Volume2 className="size-4" />
                )}
              </button>

              {showVolumePopup && (
                <div className="absolute bottom-12 left-1/2 -translate-x-1/2 w-32 p-3 rounded-2xl bg-black/85 border border-white/20 backdrop-blur-xl shadow-2xl flex items-center gap-2 animate-in fade-in slide-in-from-bottom-2 duration-150 z-30">
                  <button
                    type="button"
                    onClick={onToggleMute}
                    className="text-white/60 hover:text-white"
                  >
                    {volume === 0 ? (
                      <VolumeX className="size-3.5 text-rose-300" />
                    ) : (
                      <Volume2 className="size-3.5" />
                    )}
                  </button>
                  <Slider
                    className="flex-1"
                    value={[volume]}
                    max={100}
                    step={1}
                    variant="glass"
                    onValueChange={([v]) => onVolumePreview(v)}
                    onValueCommit={([v]) => onVolumeCommit(v)}
                  />
                </div>
              )}
            </div>
          </div>

          {/* Barra compacta de Audio FX Presets en modo inmersivo */}
          {onSelectPreset && (
            <div className="w-full flex items-center justify-center md:justify-start gap-1.5 flex-wrap pt-1">
              {IMMERSIVE_AUDIO_PRESETS.map((preset) => {
                const active = (playerState?.audio_preset ?? "normal") === preset.id;
                return (
                  <button
                    key={preset.id}
                    type="button"
                    onClick={() => onSelectPreset(preset.id)}
                    className={cn(
                      "flex items-center gap-1 px-2.5 py-1 rounded-full text-[11px] border transition-all cursor-pointer active:scale-95",
                      active
                        ? "bg-white/25 border-white/45 text-white font-semibold shadow-sm"
                        : "bg-white/[0.06] border-white/10 text-white/65 hover:text-white hover:bg-white/[0.12]",
                    )}
                    title={`Preset ${preset.label}`}
                  >
                    <span>{preset.icon}</span>
                    <span>{preset.label}</span>
                  </button>
                );
              })}
            </div>
          )}
        </div>

        {/* Columna Derecha: Letras Tipográficas a Gran Escala con Desenfoque Cinemático */}
        <div className="relative h-[55vh] md:h-[70vh] w-full flex flex-col justify-center overflow-hidden">
          <div
            className="w-full h-full overflow-y-auto overflow-x-hidden pr-2 scroll-smooth"
            ref={viewportRef}
            style={{
              WebkitMaskImage:
                "linear-gradient(to bottom, transparent 0%, #000 12%, #000 88%, transparent 100%)",
              maskImage:
                "linear-gradient(to bottom, transparent 0%, #000 12%, #000 88%, transparent 100%)",
            }}
          >
            {lines.length > 0 ? (
              <div className="flex flex-col gap-6 py-[30vh]">
                {lines.map((line, idx) => {
                  const isPast = isSynced && activeLineIndex > -1 && idx < activeLineIndex;
                  const isActive = isSynced && idx === activeLineIndex;
                  const isFuture = isSynced && idx > activeLineIndex;

                  return (
                    <div
                      key={`${line.time_ms}_${idx}`}
                      ref={(el) => {
                        lineRefs.current[idx] = el;
                      }}
                      onClick={() => {
                        if (isSynced) {
                          onSeekCommit(line.time_ms / 1000);
                        }
                      }}
                      className={cn(
                        "transition-all duration-500 cursor-pointer font-bold leading-snug tracking-tight text-left",
                        isActive &&
                          "text-white text-2xl sm:text-3xl lg:text-4xl filter-none opacity-100 translate-x-2 drop-shadow-[0_0_24px_rgba(255,255,255,0.45)]",
                        isPast &&
                          "text-white/40 text-xl sm:text-2xl lg:text-3xl blur-[1.5px] opacity-35 hover:opacity-75 hover:filter-none",
                        isFuture &&
                          "text-white/30 text-xl sm:text-2xl lg:text-3xl blur-[2.5px] opacity-15 hover:opacity-60 hover:filter-none",
                        !isSynced && "text-white/80 text-xl sm:text-2xl opacity-90"
                      )}
                    >
                      {!line.text.trim() ? (
                        <span
                          className={cn(
                            "lyric-interlude-dots",
                            isActive && "lyric-interlude-active",
                          )}
                          aria-label="Interludio instrumental"
                        >
                          <span className="lyric-interlude-dot" />
                          <span className="lyric-interlude-dot" />
                          <span className="lyric-interlude-dot" />
                        </span>
                      ) : isActive && isSynced && line.words && line.words.length > 0 ? (
                        <span>
                          {line.words.map((word, wIdx) => {
                            const pct = Math.round(getWordProgress(word, currentMs) * 100);
                            return (
                              <span
                                key={`${word.time_ms}_${wIdx}`}
                                className="lyric-word-token"
                                style={{
                                  backgroundImage: `linear-gradient(90deg, #ffffff ${pct}%, rgba(255, 255, 255, 0.38) ${pct}%)`,
                                  WebkitBackgroundClip: "text",
                                  backgroundClip: "text",
                                  color: "transparent",
                                  transform: pct > 0 && pct < 100 ? "translateY(-1px)" : undefined,
                                }}
                              >
                                {word.text}
                                {wIdx < line.words!.length - 1 ? " " : ""}
                              </span>
                            );
                          })}
                        </span>
                      ) : (
                        <span>{line.text}</span>
                      )}
                    </div>
                  );
                })}
              </div>
            ) : externalLoading || internalLoading ? (
              <div className="h-full flex flex-col items-center justify-center gap-3 text-white/60">
                <Loader2 className="size-8 text-teal-300 animate-spin" />
                <p className="text-sm font-medium">Buscando letra sincronizada en vivo...</p>
              </div>
            ) : (
              <div className="h-full flex flex-col items-center justify-center gap-3 text-white/40 text-center px-4">
                <Music2 className="size-10 text-white/30" />
                <div className="flex flex-col gap-1">
                  <p className="text-base font-semibold text-white/80">Letra no disponible automáticamente</p>
                  <p className="text-xs text-white/50 max-w-xs mx-auto">
                    No se encontró letra exacta con el título de YouTube. Puedes buscarla manualmente o recargar.
                  </p>
                </div>
                <div className="flex items-center gap-2 pt-2">
                  <button
                    type="button"
                    onClick={handleOpenSearch}
                    className="flex items-center gap-1.5 px-3.5 py-1.5 rounded-full bg-white/10 hover:bg-white/20 active:scale-95 border border-white/15 text-xs font-medium text-white transition-all cursor-pointer shadow-md"
                  >
                    <Search className="size-3.5 text-teal-300" />
                    <span>Buscar letra</span>
                  </button>
                  <button
                    type="button"
                    onClick={handleRefreshLyrics}
                    className="flex items-center gap-1.5 px-3.5 py-1.5 rounded-full bg-white/5 hover:bg-white/15 active:scale-95 border border-white/10 text-xs font-medium text-white/80 hover:text-white transition-all cursor-pointer"
                  >
                    <RefreshCw className="size-3.5" />
                    <span>Reintentar</span>
                  </button>
                </div>
              </div>
            )}
          </div>

          {/* Modal de Búsqueda Manual de Letras (Overlay centrado y accesible) */}
          {showSearchModal && (
            <div
              className="absolute inset-0 z-40 flex items-center justify-center bg-black/75 backdrop-blur-md p-4 animate-in fade-in duration-150"
              onClick={closeSearch}
            >
              <div
                className="w-full max-w-md rounded-2xl bg-neutral-900/95 border border-white/20 p-5 shadow-2xl flex flex-col gap-4 text-white animate-in zoom-in-95 duration-150"
                onClick={(e) => e.stopPropagation()}
                onKeyDown={(e) => {
                  if (e.key === "Escape") {
                    e.stopPropagation();
                    closeSearch();
                  }
                }}
              >
                <div className="flex items-center justify-between">
                  <h3 className="text-sm font-semibold text-white/90">Buscar letra manualmente</h3>
                  <button
                    type="button"
                    onClick={closeSearch}
                    className="p-1 rounded-full text-white/50 hover:text-white cursor-pointer"
                  >
                    <X className="size-4" />
                  </button>
                </div>

                <form onSubmit={handleSearchSubmit} className="flex gap-2">
                  <input
                    type="text"
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    placeholder="Título de la canción o artista..."
                    className="flex-1 px-3 py-2 rounded-xl bg-white/10 border border-white/15 text-xs text-white placeholder:text-white/40 focus:outline-hidden focus:border-teal-400"
                    autoFocus
                  />
                  <button
                    type="submit"
                    disabled={isSearching}
                    className="px-3 py-2 rounded-xl bg-teal-400 text-black text-xs font-semibold hover:bg-teal-300 transition-colors flex items-center gap-1.5 cursor-pointer disabled:opacity-50"
                  >
                    {isSearching ? <Loader2 className="size-3.5 animate-spin" /> : <Search className="size-3.5" />}
                    <span>Buscar</span>
                  </button>
                </form>

                {candidates.length > 0 ? (
                  <div className="max-h-60 overflow-y-auto flex flex-col gap-1.5 pr-1">
                    {candidates.map((cand) => (
                      <button
                        key={cand.id}
                        type="button"
                        onClick={() => handleSelectCandidate(cand)}
                        className="flex items-center justify-between p-2 rounded-xl bg-white/5 hover:bg-white/15 border border-white/10 text-left transition-colors cursor-pointer group"
                      >
                        <div className="min-w-0 flex-1">
                          <p className="text-xs font-medium text-white truncate">{cand.title}</p>
                          <p className="text-[10px] text-white/50 truncate">{cand.artist} • {cand.album || "Sencillo"}</p>
                        </div>
                        {cand.has_synced && (
                          <span className="ml-2 px-1.5 py-0.5 rounded text-[9px] bg-teal-400/20 text-teal-300 border border-teal-400/30 shrink-0 font-medium">
                            Sincronizada
                          </span>
                        )}
                      </button>
                    ))}
                  </div>
                ) : isSearching ? (
                  <div className="py-6 flex items-center justify-center text-xs text-white/50 gap-2">
                    <Loader2 className="size-4 animate-spin text-teal-300" />
                    <span>Consultando biblioteca de letras...</span>
                  </div>
                ) : (
                  <p className="text-xs text-white/40 text-center py-4">
                    Escribe el nombre de la canción para buscar letras disponibles en LRCLIB.
                  </p>
                )}
              </div>
            </div>
          )}

          {/* Modal de Creación de Lyric Card para compartir */}
          <LyricCardModal
            isOpen={showLyricCardModal}
            onClose={() => setShowLyricCardModal(false)}
            track={currentTrack}
            lines={lines}
            activeLineIndex={activeLineIndex}
            palette={palette}
          />

          {/* Botón flotante para reanudar el auto-scroll */}
          {isSynced && isUserScrolling && activeLineIndex >= 0 && (
            <button
              type="button"
              onClick={resumeScroll}
              className="lyrics-resume-scroll-btn"
              title="Centrar en la línea activa"
            >
              <ArrowDown className="size-3.5 animate-bounce" />
              <span>Reanudar sincronización</span>
            </button>
          )}
        </div>
      </main>

      {/* Pie de página sutil con pointer-events-none para nunca tapar clicks */}
      <footer className="relative z-20 flex items-center justify-between px-6 md:px-8 py-2.5 text-xs text-white/40 pointer-events-none">
        <span className="hidden sm:inline">Presiona [Esc] para salir</span>
        <span className="mx-auto sm:mx-0">Cadence Immersive Audio</span>
      </footer>
    </div>
  );
}
