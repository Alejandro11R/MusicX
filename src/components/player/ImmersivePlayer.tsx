import React, { useRef, useState } from "react";
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
} from "lucide-react";
import { SearchResult, PlayerState, LyricsDoc, AudioPreset } from "../../lib/tauri";
import { safeDuration, safePosition, formatDuration } from "../../lib/playback";
import { findActiveLineIndex } from "../../lib/lyrics";
import { useLyricClock } from "../lyrics/useLyricClock";
import { useLyricsScroll } from "../lyrics/useLyricsScroll";
import { useAlbumPalette } from "../../lib/palette";
import { Slider } from "../ui/slider";
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
}

export function ImmersivePlayer({
  currentTrack,
  playerState,
  lyricsDoc,
  isLyricsLoading,
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
  onClose,
}: ImmersivePlayerProps) {
  const palette = useAlbumPalette(currentTrack?.thumbnail_url);
  const viewportRef = useRef<HTMLDivElement>(null);
  const lineRefs = useRef<(HTMLDivElement | null)[]>([]);
  const [showVolumePopup, setShowVolumePopup] = useState(false);

  const duration = safeDuration(playerState?.duration_seconds ?? 0);
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

  const activeLineIndex = isSynced
    ? findActiveLineIndex(lines, interpolatedTime, totalOffsetMs)
    : -1;

  const { isUserScrolling, resumeScroll } = useLyricsScroll({
    activeLineIndex,
    viewportRef,
    lineRefs,
    enabled: isSynced,
  });

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
      <header className="relative z-20 flex items-center justify-between p-6 md:p-8">
        <button
          type="button"
          onClick={onClose}
          className="size-10 rounded-full bg-white/10 hover:bg-white/20 active:scale-95 border border-white/15 backdrop-blur-md text-white/80 hover:text-white flex items-center justify-center transition-all cursor-pointer shadow-lg"
          title="Salir del modo inmersivo (Esc)"
          aria-label="Cerrar vista inmersiva"
        >
          <X className="size-5" />
        </button>

        {isSynced && (
          <div className="hidden sm:flex items-center gap-2 px-3.5 py-1.5 rounded-full bg-white/10 border border-white/15 backdrop-blur-md text-xs font-medium text-white/85 shadow-md">
            <Sparkles className="size-3.5 text-amber-300" />
            <span>Letras sincronizadas en vivo</span>
          </div>
        )}
      </header>

      {/* Contenido Central: Simetría 50 / 50 estilo Apple Music */}
      <main className="relative z-10 flex-1 min-h-0 w-full max-w-7xl mx-auto px-6 md:px-12 grid grid-cols-1 md:grid-cols-2 gap-8 md:gap-14 items-center">
        {/* Columna Izquierda: Carátula Héroe + Metadata + Transporte */}
        <div className="flex flex-col items-center md:items-start max-w-md mx-auto w-full gap-5">
          {/* Carátula Grande en Alta Definición */}
          <div className="relative size-60 sm:size-72 md:size-80 lg:size-96 shrink-0 overflow-hidden rounded-2xl border border-white/20 shadow-[0_24px_50px_-12px_rgba(0,0,0,0.95)] transition-all duration-300">
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
          <div className="w-full flex flex-col gap-1 text-center md:text-left">
            <div className="flex items-center justify-center md:justify-start gap-2">
              <h1 className="text-xl sm:text-2xl font-bold text-white tracking-tight truncate">
                {currentTrack?.title || "Sin reproducción"}
              </h1>
              <span className="px-1.5 py-0.5 rounded text-[10px] font-bold bg-white/20 text-white/90 border border-white/20 shrink-0">
                E
              </span>
            </div>
            <p className="text-sm sm:text-base font-medium text-white/60 truncate">
              {currentTrack?.artist || "Cadence"}
            </p>
          </div>

          {/* Barra de progreso estilo Apple Music con tiempo transcurrido y restante (-X:XX) */}
          <div className="w-full flex flex-col gap-1.5 pt-1">
            <Slider
              className="w-full cursor-pointer"
              value={[position]}
              max={Math.max(duration, 1)}
              step={1}
              variant="glass"
              onValueChange={([val]) => onSeekPreview(val)}
              onValueCommit={([val]) => onSeekCommit(val)}
              aria-label="Progreso de pista"
            />
            <div className="flex items-center justify-between text-xs tabular-nums text-white/50 font-medium px-0.5">
              <span>{formatDuration(position)}</span>
              <span>-{formatDuration(remaining)}</span>
            </div>
          </div>

          {/* Botonera de Transporte Centrada y Simétrica */}
          <div className="w-full flex items-center justify-center md:justify-start gap-5 pt-2">
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
              className="size-14 sm:size-16 rounded-full bg-white text-black hover:scale-105 active:scale-95 shadow-[0_12px_32px_rgba(255,255,255,0.3)] transition-all flex items-center justify-center cursor-pointer"
              title={isPlaying ? "Pausar (Espacio)" : "Reproducir (Espacio)"}
            >
              {isPlaying ? (
                <Pause className="size-7 fill-black text-black" />
              ) : (
                <Play className="size-7 fill-black text-black ml-1" />
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
                <div className="absolute bottom-12 left-1/2 -translate-x-1/2 w-32 p-3 rounded-2xl bg-black/80 border border-white/15 backdrop-blur-xl shadow-2xl flex items-center gap-2 animate-in fade-in slide-in-from-bottom-2 duration-150 z-30">
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
        </div>

        {/* Columna Derecha: Letras Tipográficas a Gran Escala con Desenfoque Cinemático */}
        <div className="relative h-[60vh] md:h-[75vh] w-full flex flex-col justify-center overflow-hidden">
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
              <div className="flex flex-col gap-6 py-[35vh]">
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
                      <span>{line.text || "•••"}</span>
                    </div>
                  );
                })}
              </div>
            ) : isLyricsLoading ? (
              <div className="h-full flex flex-col items-center justify-center gap-3 text-white/50">
                <div className="size-8 rounded-full border-2 border-white/20 border-t-white animate-spin" />
                <p className="text-sm font-medium">Buscando letra en tiempo real...</p>
              </div>
            ) : (
              <div className="h-full flex flex-col items-center justify-center gap-2 text-white/40 text-center">
                <p className="text-lg font-semibold text-white/70">Letra no disponible</p>
                <p className="text-xs max-w-xs">
                  Disfruta de la reproducción inmersiva con el arte del álbum
                </p>
              </div>
            )}
          </div>

          {/* Botón flotante para reanudar el auto-scroll */}
          {isSynced && isUserScrolling && activeLineIndex >= 0 && (
            <button
              onClick={resumeScroll}
              className="absolute bottom-4 right-4 z-20 flex items-center gap-1.5 px-4 py-2 rounded-full bg-white/20 hover:bg-white/30 text-white text-xs font-semibold backdrop-blur-xl border border-white/20 shadow-xl transition-all cursor-pointer animate-in fade-in"
            >
              <ArrowDown className="size-3.5 animate-bounce" />
              <span>Sincronizar</span>
            </button>
          )}
        </div>
      </main>

      {/* Pie de página sutil */}
      <footer className="relative z-20 flex items-center justify-between px-6 md:px-8 py-4 text-xs text-white/40">
        <span className="hidden sm:inline">Presiona [Esc] para salir</span>
        <span className="mx-auto sm:mx-0">Cadence Immersive Audio</span>
      </footer>
    </div>
  );
}
