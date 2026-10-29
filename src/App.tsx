import React, {
  useEffect,
  useRef,
  useState,
  useCallback,
  useMemo,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import {
  ChevronDown,
  Clock,
  Headphones,
  Link2,
  ListMusic,
  Loader2,
  Maximize2,
  Mic2,
  Minus,
  Pause,
  Play,
  Plus,
  Repeat,
  Search,
  SkipBack,
  SkipForward,
  Sliders,
  Sparkles,
  Square,
  Volume2,
  VolumeX,
  X,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Slider } from "@/components/ui/slider";
import { GlassButton, MeshBackground, GlassPanel, SpecularRing } from "@/components/ui/liquid-glass";
import {
  FocusNowPlayingDock,
  FocusStudio,
  FocusWidget,
} from "@/components/focus/FocusStudio";
import { LyricsPanel } from "@/components/lyrics/LyricsPanel";
import { ImmersivePlayer } from "@/components/player/ImmersivePlayer";
import { cn } from "@/lib/utils";
import { formatDuration, safeDuration, safePosition, seekTarget } from "@/lib/playback";
import { extractDominantColor } from "@/lib/album-color";
import {
  search,
  play,
  pause,
  resume,
  stop,
  setVolume,
  seek,
  setLoop,
  nextTrack,
  previousTrack,
  getQueue,
  addToQueue,
  removeFromQueue,
  clearQueue,
  playQueueIndex,
  setAudioPreset,
  setSpeed,
  startFocus,
  pauseFocus,
  resetFocus,
  skipFocus,
  setFocusAutoPause,
  setFocusDurations,
  getFocusState,
  listenFocusEvents,
  getState,
  health,
  quit,
  subscribeEvents,
  listenOrchestratorEvents,
  getLyrics,
  listenLyricsEvents,
  type PlayerState,
  type SearchResult,
  type OrchestratorEvent,
  type QueueItem,
  type AudioPreset,
  type FocusState,
  type LyricsDoc,
  type LyricsEvent,
} from "./lib/tauri";

const POLL_INTERVAL_MS = 5000; // Fallback polling every 5s
const RECENT_STORAGE_KEY = "cadence_recent_tracks";
// Tras un seek, ignoramos los `Progress` que aún traen la posición vieja
// (mpv tarda uno o dos ticks en reflejar el salto) para que el thumb no
// rebote visualmente.
const SEEK_GUARD_MS = 1200;

// ===== Memoized Components =====

function EqualizerBars({ className }: { className?: string }) {
  return (
    <div className={cn("cadence-eq-bars", className)} aria-label="Reproduciendo">
      <span className="cadence-eq-bar" />
      <span className="cadence-eq-bar" />
      <span className="cadence-eq-bar" />
    </div>
  );
}

interface AlbumHeroProps {
  currentTrack: SearchResult | null;
  isPlaying: boolean;
  isLooping: boolean;
  isLyricsOpen: boolean;
  onTogglePlayback: () => void;
  onToggleLoop: () => void;
  onToggleLyrics: () => void;
  onOpenImmersive?: () => void;
}

const AlbumHero = React.memo(function AlbumHero({
  currentTrack,
  isPlaying,
  isLooping,
  isLyricsOpen,
  onTogglePlayback,
  onToggleLoop,
  onToggleLyrics,
  onOpenImmersive,
}: AlbumHeroProps) {
  if (!currentTrack) return null;

  return (
    <div className="relative flex flex-col sm:flex-row items-center sm:items-end gap-4 p-4 rounded-2xl bg-white/[0.03] border border-white/10 shadow-lg backdrop-blur-md animate-in fade-in duration-200">
      <div
        onClick={onOpenImmersive}
        className="relative size-28 sm:size-32 shrink-0 overflow-hidden rounded-xl border border-white/15 shadow-[0_12px_28px_rgba(0,0,0,0.6)] cursor-pointer hover:scale-102 active:scale-98 transition-all group"
        title="Abrir vista inmersiva (I)"
      >
        {currentTrack.thumbnail_url ? (
          <img
            src={currentTrack.thumbnail_url}
            alt=""
            className="size-full object-cover group-hover:scale-105 transition-transform duration-300"
          />
        ) : (
          <div className="size-full flex items-center justify-center bg-teal-500/10">
            <Headphones className="size-10 text-teal-300" />
          </div>
        )}
      </div>

      <div className="flex min-w-0 flex-1 flex-col gap-1 text-center sm:text-left">
        <span className="text-[10px] uppercase tracking-wider font-semibold text-teal-400">
          En reproducción
        </span>
        <h2 className="text-base sm:text-lg font-bold text-white leading-tight truncate">
          {currentTrack.title}
        </h2>
        <p className="text-xs text-white/60 truncate">
          {currentTrack.artist || "Cadence Music"}
        </p>

        <div className="flex items-center justify-center sm:justify-start gap-2 pt-2">
          <button
            type="button"
            onClick={onTogglePlayback}
            className="px-4 py-1.5 rounded-full bg-white text-black font-semibold text-xs hover:bg-white/90 active:scale-95 transition-all flex items-center gap-1.5 shadow-md cursor-pointer"
          >
            {isPlaying ? (
              <>
                <Pause className="size-3.5 fill-black text-black" />
                <span>Pausar</span>
              </>
            ) : (
              <>
                <Play className="size-3.5 fill-black text-black ml-0.5" />
                <span>Reproducir</span>
              </>
            )}
          </button>

          <button
            type="button"
            onClick={onToggleLoop}
            className={cn(
              "p-1.5 rounded-full border transition-all text-xs cursor-pointer",
              isLooping
                ? "bg-teal-400/20 text-teal-300 border-teal-400/40 shadow-sm"
                : "border-white/10 text-white/70 hover:text-white hover:bg-white/10"
            )}
            title={isLooping ? "Bucle activo" : "Activar bucle"}
          >
            <Repeat className="size-3.5" />
          </button>

          <button
            type="button"
            onClick={onToggleLyrics}
            className={cn(
              "px-3 py-1.5 rounded-full border transition-all text-xs flex items-center gap-1.5 cursor-pointer",
              isLyricsOpen
                ? "bg-indigo-500/25 text-indigo-300 border-indigo-400/40 shadow-sm"
                : "border-white/10 text-white/70 hover:text-white hover:bg-white/10"
            )}
            title="Ver letras sincronizadas"
          >
            <Mic2 className="size-3.5" />
            <span>Letras</span>
          </button>

          {onOpenImmersive && (
            <button
              type="button"
              onClick={onOpenImmersive}
              className="px-3 py-1.5 rounded-full border border-white/10 text-white/75 hover:text-white hover:bg-white/10 transition-all text-xs flex items-center gap-1.5 cursor-pointer shadow-sm"
              title="Abrir vista inmersiva Apple Music (I)"
            >
              <Sparkles className="size-3.5 text-amber-300" />
              <span className="hidden sm:inline">Inmersivo</span>
            </button>
          )}
        </div>
      </div>
    </div>
  );
});

interface SearchResultItemProps {
  track: SearchResult;
  isSelected: boolean;
  isPlaying: boolean;
  isLoading: boolean;
  onClick: () => void;
  onAddToQueue?: (e: React.MouseEvent) => void;
  onMouseEnter: () => void;
}

const SearchResultItem = React.memo(function SearchResultItem({
  track,
  isSelected,
  isPlaying,
  isLoading,
  onClick,
  onAddToQueue,
  onMouseEnter,
}: SearchResultItemProps) {
  return (
    <li>
      <button
        type="button"
        className={cn(
          "group relative flex w-full items-center gap-3 rounded-xl p-2 text-left transition-all duration-200",
          "bg-white/[0.02] hover:bg-white/[0.08] border border-transparent hover:border-white/10",
          isSelected && "bg-white/[0.09] border-white/15",
          isPlaying &&
            "bg-teal-500/10 border-teal-500/30 shadow-[0_0_15px_-3px_rgba(45,212,191,0.2)]",
        )}
        onClick={onClick}
        onMouseEnter={onMouseEnter}
        disabled={isLoading}
      >
        <div className="relative size-10 shrink-0 overflow-hidden rounded-lg bg-white/5 border border-white/10 shadow-sm flex items-center justify-center">
          {track.thumbnail_url ? (
            <img
              src={track.thumbnail_url}
              alt=""
              className="size-full object-cover transition-transform duration-300 group-hover:scale-105"
              loading="lazy"
              onError={(e) => {
                (e.currentTarget as HTMLElement).style.display = "none";
              }}
            />
          ) : (
            <Headphones className="size-5 text-teal-300/80" />
          )}
          {isLoading ? (
            <div className="absolute inset-0 flex items-center justify-center bg-black/70 backdrop-blur-xs">
              <Loader2 className="size-4 animate-spin text-teal-300" />
            </div>
          ) : isPlaying ? (
            <div className="absolute inset-0 flex items-center justify-center bg-black/55 backdrop-blur-[1px] animate-in fade-in duration-150">
              <EqualizerBars />
            </div>
          ) : null}
        </div>
        <div className="flex min-w-0 flex-1 flex-col justify-center">
          <span
            className={cn(
              "truncate text-xs font-medium leading-tight",
              isPlaying ? "text-teal-300" : "text-white/90",
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
        <div className="flex items-center gap-2 shrink-0">
          <span className="text-[11px] tabular-nums text-white/40">
            {formatDuration(track.duration_seconds)}
          </span>
          {onAddToQueue && (
            <div
              role="button"
              tabIndex={0}
              className="opacity-0 group-hover:opacity-100 p-1 rounded-md bg-white/10 hover:bg-teal-400/20 hover:text-teal-300 text-white/70 transition-all cursor-pointer"
              title="Añadir a la cola"
              onClick={onAddToQueue}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.stopPropagation();
                  onAddToQueue(e as unknown as React.MouseEvent);
                }
              }}
            >
              <Plus className="size-3.5" />
            </div>
          )}
        </div>
      </button>
    </li>
  );
});

interface RecentTrackItemProps {
  track: SearchResult;
  isPlaying: boolean;
  isLoading: boolean;
  onClick: () => void;
  onAddToQueue?: (e: React.MouseEvent) => void;
}

const RecentTrackItem = React.memo(function RecentTrackItem({
  track,
  isPlaying,
  isLoading,
  onClick,
  onAddToQueue,
}: RecentTrackItemProps) {
  return (
    <li>
      <button
        type="button"
        className={cn(
          "group relative flex w-full items-center gap-3 rounded-xl p-2 text-left transition-all duration-200",
          "bg-white/[0.02] hover:bg-white/[0.08] border border-transparent hover:border-white/10",
          isPlaying &&
            "bg-teal-500/10 border-teal-500/30 shadow-[0_0_15px_-3px_rgba(45,212,191,0.2)]",
        )}
        onClick={onClick}
        disabled={isLoading}
      >
        <div className="relative size-10 shrink-0 overflow-hidden rounded-lg bg-white/5 border border-white/10 shadow-sm flex items-center justify-center">
          {track.thumbnail_url ? (
            <img
              src={track.thumbnail_url}
              alt=""
              className="size-full object-cover transition-transform duration-300 group-hover:scale-105"
              loading="lazy"
              onError={(e) => {
                (e.currentTarget as HTMLElement).style.display = "none";
              }}
            />
          ) : (
            <Headphones className="size-5 text-teal-300/80" />
          )}
          {isLoading ? (
            <div className="absolute inset-0 flex items-center justify-center bg-black/70 backdrop-blur-xs">
              <Loader2 className="size-4 animate-spin text-teal-300" />
            </div>
          ) : isPlaying ? (
            <div className="absolute inset-0 flex items-center justify-center bg-black/55 backdrop-blur-[1px] animate-in fade-in duration-150">
              <EqualizerBars />
            </div>
          ) : null}
        </div>
        <div className="flex min-w-0 flex-1 flex-col justify-center">
          <span
            className={cn(
              "truncate text-xs font-medium leading-tight",
              isPlaying ? "text-teal-300" : "text-white/90",
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
        <div className="flex items-center gap-2 shrink-0">
          <span className="text-[11px] tabular-nums text-white/40">
            {formatDuration(track.duration_seconds)}
          </span>
          {onAddToQueue && (
            <div
              role="button"
              tabIndex={0}
              className="opacity-0 group-hover:opacity-100 p-1 rounded-md bg-white/10 hover:bg-teal-400/20 hover:text-teal-300 text-white/70 transition-all cursor-pointer"
              title="Añadir a la cola"
              onClick={onAddToQueue}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.stopPropagation();
                  onAddToQueue(e as unknown as React.MouseEvent);
                }
              }}
            >
              <Plus className="size-3.5" />
            </div>
          )}
        </div>
      </button>
    </li>
  );
});

interface QueuePanelProps {
  queue: QueueItem[];
  currentTrackId?: string;
  onPlayItem: (index: number) => void;
  onRemoveItem: (id: string, e: React.MouseEvent) => void;
  onClearQueue: () => void;
  onClose: () => void;
}

const QueuePanel = React.memo(function QueuePanel({
  queue,
  currentTrackId,
  onPlayItem,
  onRemoveItem,
  onClearQueue,
  onClose,
}: QueuePanelProps) {
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2 p-1">
      <div className="flex items-center justify-between px-1">
        <span className="flex items-center gap-1.5 text-xs font-semibold text-white/80">
          <ListMusic className="size-3.5 text-teal-400" />
          Cola de reproducción ({queue.length})
        </span>
        <div className="flex items-center gap-2">
          {queue.length > 0 && (
            <button
              type="button"
              onClick={onClearQueue}
              className="text-[10px] text-white/50 hover:text-rose-300 transition-colors"
            >
              Limpiar
            </button>
          )}
          <button
            type="button"
            onClick={onClose}
            className="text-[10px] text-white/50 hover:text-white transition-colors"
          >
            Cerrar
          </button>
        </div>
      </div>

      {queue.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 text-center p-6 select-none">
          <div className="size-10 rounded-xl liquid-pill flex items-center justify-center text-white/40 shadow-sm">
            <ListMusic className="size-5" />
          </div>
          <p className="text-xs text-white/60">La cola está vacía</p>
          <p className="text-[11px] text-white/40">Usa el botón + en cualquier canción para encolarla</p>
        </div>
      ) : (
        <ScrollArea className="min-h-0 flex-1 pr-1">
          <ol className="flex flex-col gap-1.5">
            {queue.map((item, index) => {
              const isCurrent = currentTrackId === item.track.id;
              return (
                <li key={item.id}>
                  <div
                    className={cn(
                      "group relative flex w-full items-center gap-3 rounded-xl p-2 text-left transition-all duration-200",
                      "bg-white/[0.02] hover:bg-white/[0.08] border border-transparent hover:border-white/10",
                      isCurrent && "bg-teal-500/10 border-teal-500/30 shadow-[0_0_15px_-3px_rgba(45,212,191,0.2)]",
                    )}
                  >
                    <button
                      type="button"
                      className="flex min-w-0 flex-1 items-center gap-3 text-left"
                      onClick={() => onPlayItem(index)}
                    >
                      <div className="relative size-10 shrink-0 overflow-hidden rounded-lg bg-white/5 border border-white/10 shadow-sm">
                        <img
                          src={item.track.thumbnail_url}
                          alt=""
                          className="size-full object-cover"
                          loading="lazy"
                          onError={(e) => {
                            (e.currentTarget as HTMLElement).style.display = "none";
                          }}
                        />
                        {isCurrent && (
                          <div className="absolute inset-0 flex items-center justify-center bg-black/60 backdrop-blur-xs">
                            <EqualizerBars />
                          </div>
                        )}
                      </div>
                      <div className="flex min-w-0 flex-1 flex-col justify-center">
                        <span
                          className={cn(
                            "truncate text-xs font-medium leading-tight",
                            isCurrent ? "text-teal-300" : "text-white/90",
                          )}
                        >
                          {item.track.title}
                        </span>
                        {item.track.artist && (
                          <span className="truncate text-[11px] text-white/50">
                            {item.track.artist}
                          </span>
                        )}
                      </div>
                      <span className="shrink-0 text-[11px] tabular-nums text-white/40">
                        {formatDuration(item.track.duration_seconds)}
                      </span>
                    </button>
                    <button
                      type="button"
                      onClick={(e) => onRemoveItem(item.id, e)}
                      className="size-6 shrink-0 rounded-lg flex items-center justify-center text-white/30 hover:text-rose-300 hover:bg-rose-500/10 transition-colors"
                      title="Quitar de la cola"
                    >
                      <X className="size-3.5" />
                    </button>
                  </div>
                </li>
              );
            })}
          </ol>
        </ScrollArea>
      )}
    </div>
  );
});

const AUDIO_PRESETS: { id: AudioPreset; label: string; icon: string }[] = [
  { id: "normal", label: "Normal", icon: "✨" },
  { id: "lofi", label: "Lo-Fi", icon: "☕" },
  { id: "bass_boost", label: "Bass", icon: "🔊" },
  { id: "loudnorm", label: "Loudnorm", icon: "⚖️" },
  { id: "slowed_reverb", label: "Slowed", icon: "🌙" },
  { id: "nightcore", label: "Nightcore", icon: "⚡" },
];

interface NowPlayingProps {
  playerState: PlayerState | null;
  isTrackLoading: boolean;
  isPlaying: boolean;
  isLooping: boolean;
  dragPosition: number | null;
  onTogglePlayback: () => void;
  onToggleLoop: () => void;
  onStop: () => void;
  onNext: () => void;
  onPrevious: () => void;
  onToggleQueue: () => void;
  isQueueOpen: boolean;
  queueCount: number;
  onToggleLyrics?: () => void;
  isLyricsOpen?: boolean;
  onSelectPreset: (preset: AudioPreset) => void;
  onCycleSpeed: () => void;
  /** Solo mueve el thumb; no llama al backend hasta soltar. */
  onSeekPreview: (position: number) => void;
  /** Confirma el seek al soltar el slider. */
  onSeekCommit: (position: number) => void;
  /** Mueve el volumen en la UI sin llamar a mpv todavía. */
  onVolumePreview: (level: number) => void;
  /** Aplica el volumen definitivo al soltar el slider. */
  onVolumeCommit: (level: number) => void;
  onToggleMute: () => void;
  volume: number;
  onOpenImmersive?: () => void;
}

const NowPlaying = React.memo(function NowPlaying({
  playerState,
  isTrackLoading,
  isPlaying,
  isLooping,
  dragPosition,
  onTogglePlayback,
  onToggleLoop,
  onStop,
  onNext,
  onPrevious,
  onToggleQueue,
  isQueueOpen,
  queueCount,
  onToggleLyrics,
  isLyricsOpen,
  onSeekPreview,
  onSeekCommit,
  onVolumePreview,
  onVolumeCommit,
  onToggleMute,
  volume,
  onSelectPreset,
  onCycleSpeed,
  onOpenImmersive,
}: NowPlayingProps) {
  const [showAudioFx, setShowAudioFx] = useState(false);
  const current = playerState?.current;
  const duration = safeDuration(playerState?.duration_seconds ?? 0);
  const position = safePosition(
    dragPosition ?? playerState?.position_seconds ?? 0,
    duration,
  );
  const [albumColor, setAlbumColor] = useState<string | null>(null);

  useEffect(() => {
    const url = current?.thumbnail_url;
    if (!url) {
      setAlbumColor(null);
      return;
    }
    extractDominantColor(url).then(setAlbumColor);
  }, [current?.thumbnail_url]);

  return (
    <div className="liquid-card mt-auto flex flex-col gap-3 rounded-2xl p-3 border border-white/10 shadow-xl" role="region" aria-label="Reproductor">
      {isTrackLoading && !current && (
        <div className="flex items-center gap-2 py-1 text-xs text-teal-300" role="status">
          <Loader2 className="size-3.5 animate-spin" />
          <span>Cargando pista de audio...</span>
        </div>
      )}

      {current && (
        <div className="flex items-center gap-3">
          <SpecularRing
            className="size-12 shrink-0 cursor-pointer hover:scale-105 active:scale-95 transition-transform"
            onClick={onOpenImmersive}
            title="Abrir Modo Inmersivo Apple Music (I)"
          >
            <div
              className={cn(
                "overflow-hidden rounded-xl bg-white/10 transition-all duration-300",
                isPlaying && "shadow-[0_4px_20px_-2px_rgba(45,212,191,0.35)]",
              )}
              style={{
                borderColor: albumColor ?? undefined,
                borderWidth: albumColor ? "1px" : undefined,
                borderStyle: albumColor ? "solid" : undefined,
              }}
            >
              {current.thumbnail_url ? (
                <img
                  src={current.thumbnail_url}
                  alt=""
                  className="size-full object-cover"
                  onError={(e) => {
                    (e.currentTarget as HTMLElement).style.display = "none";
                  }}
                />
              ) : (
                <div className="size-full flex items-center justify-center bg-teal-500/10">
                  <Headphones className="size-6 text-teal-300" />
                </div>
              )}
            </div>
          </SpecularRing>
          <div className="flex min-w-0 flex-1 flex-col justify-center gap-0.5">
            <div className="flex items-center gap-1.5">
              <span
                className="size-1.5 shrink-0 rounded-full transition-all"
                style={{
                  backgroundColor: isPlaying
                    ? (albumColor ?? "#2dd4bf")
                    : undefined,
                  boxShadow: isPlaying
                    ? `0 0 6px ${albumColor ?? "#2dd4bf"}`
                    : undefined,
                  animation: isPlaying ? "pulse 2s infinite" : undefined,
                }}
              />
              <p className="truncate text-xs font-semibold text-white/95">
                {current.title}
              </p>
            </div>
            {current.artist && (
              <p className="truncate pl-3 text-[11px] text-white/60">
                {current.artist}
              </p>
            )}
          </div>
        </div>
      )}

      {current && (
        <div className="flex items-center gap-2 px-0.5">
          <span className="w-8 shrink-0 text-right text-[10px] tabular-nums text-white/60">
            {formatDuration(position)}
          </span>
          <Slider
            className="flex-1"
            value={[position]}
            max={Math.max(duration, 1)}
            step={1}
            variant="glass"
            onValueChange={([value]) => onSeekPreview(value)}
            onValueCommit={([value]) => onSeekCommit(value)}
            aria-label="Posición de reproducción"
            aria-valuetext={`${formatDuration(position)} de ${formatDuration(duration)}`}
          />
          <span className="w-8 shrink-0 text-[10px] tabular-nums text-white/60">
            {formatDuration(duration)}
          </span>
        </div>
      )}

      {/* Controles Principales */}
      <div className="flex items-center justify-center gap-2 pt-0.5">
        <GlassButton
          variant="ghost"
          size="md"
          aria-label={isLooping ? "Desactivar bucle (L)" : "Activar bucle (L)"}
          title={isLooping ? "Bucle activo (L)" : "Bucle inactivo (L)"}
          onClick={onToggleLoop}
          className={cn(
            isLooping
              ? "bg-teal-400/20 text-teal-300 border-teal-400/40 shadow-[0_0_12px_rgba(45,212,191,0.3)]"
              : "border-white/5 hover:bg-white/[0.1] hover:text-white",
          )}
        >
          <Repeat className={cn("size-3.5", isLooping && "stroke-[2.5]")} />
        </GlassButton>

        <GlassButton
          variant="ghost"
          size="md"
          aria-label="Pista anterior (P)"
          title="Pista anterior (P)"
          onClick={onPrevious}
          className="border-white/5 hover:bg-white/[0.1] hover:text-white"
        >
          <SkipBack className="size-3.5 fill-current" />
        </GlassButton>

        <GlassButton
          variant="primary"
          size="lg"
          aria-label={isPlaying ? "Pausar (Espacio)" : "Reproducir (Espacio)"}
          title={isPlaying ? "Pausar (Espacio)" : "Reproducir (Espacio)"}
          onClick={onTogglePlayback}
        >
          {isPlaying ? <Pause className="size-4 fill-current" /> : <Play className="size-4 ml-0.5 fill-current" />}
        </GlassButton>

        <GlassButton
          variant="ghost"
          size="md"
          aria-label="Siguiente pista (N)"
          title="Siguiente pista (N)"
          onClick={onNext}
          className="border-white/5 hover:bg-white/[0.1] hover:text-white"
        >
          <SkipForward className="size-3.5 fill-current" />
        </GlassButton>

        <GlassButton
          variant="ghost"
          size="md"
          aria-label="Detener"
          title="Detener"
          onClick={onStop}
          className="border-white/5 hover:bg-white/[0.1] hover:text-white"
        >
          <Square className="size-3.5 fill-current" />
        </GlassButton>

        <GlassButton
          variant="ghost"
          size="md"
          aria-label="Cola de reproducción (Q)"
          title={`Cola de reproducción (${queueCount}) (Q)`}
          onClick={onToggleQueue}
          className={cn(
            "relative border-white/5 hover:bg-white/[0.1] hover:text-white",
            isQueueOpen && "bg-teal-400/20 text-teal-300 border-teal-400/40 shadow-[0_0_12px_rgba(45,212,191,0.3)]",
          )}
        >
          <ListMusic className="size-3.5" />
          {queueCount > 0 && (
            <span className="absolute -top-1 -right-1 flex size-3.5 items-center justify-center rounded-full bg-teal-400 text-[8px] font-bold text-black shadow-xs">
              {queueCount > 9 ? "9+" : queueCount}
            </span>
          )}
        </GlassButton>

        {onToggleLyrics && (
          <GlassButton
            variant="ghost"
            size="md"
            aria-label="Letras sincronizadas (Y)"
            title="Letras en tiempo real (Y)"
            onClick={onToggleLyrics}
            className={cn(
              "relative border-white/5 hover:bg-white/[0.1] hover:text-white",
              isLyricsOpen && "bg-indigo-500/25 text-indigo-300 border-indigo-400/40 shadow-[0_0_12px_rgba(99,102,241,0.35)]",
            )}
          >
            <Mic2 className="size-3.5" />
          </GlassButton>
        )}

        <GlassButton
          variant="ghost"
          size="md"
          aria-label={showAudioFx ? "Ocultar efectos de audio" : "Mostrar efectos de audio"}
          title="Efectos de audio y velocidad (EQ)"
          onClick={() => setShowAudioFx((prev) => !prev)}
          className={cn(
            "relative border-white/5 hover:bg-white/[0.1] hover:text-white",
            (showAudioFx || (playerState?.audio_preset ?? "normal") !== "normal" || (playerState?.speed ?? 1.0) !== 1.0) &&
              "bg-purple-500/20 text-purple-300 border-purple-400/40 shadow-[0_0_10px_rgba(168,85,247,0.3)]",
          )}
        >
          <Sliders className="size-3.5" />
        </GlassButton>

        {onOpenImmersive && (
          <GlassButton
            variant="ghost"
            size="md"
            aria-label="Modo Inmersivo (I)"
            title="Modo Inmersivo Apple Music (I)"
            onClick={onOpenImmersive}
            className="border-white/5 hover:bg-white/[0.1] hover:text-white text-amber-300"
          >
            <Sparkles className="size-3.5" />
          </GlassButton>
        )}
      </div>

      {/* Control de Volumen Líquido */}
      <div className="flex items-center gap-2 px-1 pt-0.5">
        <button
          type="button"
          onClick={onToggleMute}
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
          variant="glass"
          onValueChange={([level]) => onVolumePreview(level)}
          onValueCommit={([level]) => onVolumeCommit(level)}
          aria-label="Volumen"
          aria-valuetext={`${volume}%`}
        />
        <span className="w-6 shrink-0 text-right text-[10px] tabular-nums text-white/60">
          {volume}
        </span>
      </div>

      {/* Audio FX Presets & Velocidad colapsable */}
      {showAudioFx && (
        <div className="flex items-center justify-between gap-1 pt-1.5 border-t border-white/5 text-[10px] animate-in fade-in duration-200">
          <div className="flex items-center gap-1 overflow-x-auto py-0.5 no-scrollbar">
            {AUDIO_PRESETS.map((p) => {
              const active = (playerState?.audio_preset ?? "normal") === p.id;
              return (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => onSelectPreset(p.id)}
                  className={cn(
                    "flex items-center gap-1 px-2 py-0.5 rounded-lg transition-all duration-200 border whitespace-nowrap",
                    active
                      ? "bg-teal-400/25 border-teal-300/50 text-teal-200 font-medium shadow-[0_0_8px_rgba(45,212,191,0.25)]"
                      : "bg-white/[0.03] border-white/5 text-white/60 hover:text-white hover:bg-white/[0.08]"
                  )}
                  title={`Preset ${p.label}`}
                >
                  <span>{p.icon}</span>
                  <span>{p.label}</span>
                </button>
              );
            })}
          </div>

          <div className="flex items-center gap-1 shrink-0">
            <button
              type="button"
              onClick={onCycleSpeed}
              className={cn(
                "px-2 py-0.5 rounded-lg border text-[10px] font-mono transition-all",
                (playerState?.speed ?? 1.0) !== 1.0
                  ? "bg-purple-500/20 border-purple-400/40 text-purple-200 shadow-[0_0_8px_rgba(168,85,247,0.25)]"
                  : "bg-white/[0.03] border-white/5 text-white/60 hover:text-white"
              )}
              title="Cambiar velocidad de reproducción"
            >
              {(playerState?.speed ?? 1.0).toFixed(2)}x
            </button>
          </div>
        </div>
      )}
    </div>
  );
});

interface NowPlayingMiniDockProps {
  playerState: PlayerState | null;
  isTrackLoading: boolean;
  isPlaying: boolean;
  dragPosition: number | null;
  onTogglePlayback: () => void;
  onPrevious: () => void;
  onNext: () => void;
  onSeekPreview: (position: number) => void;
  onSeekCommit: (position: number) => void;
  onCloseLyrics: () => void;
  onOpenImmersive?: () => void;
}

const NowPlayingMiniDock = React.memo(function NowPlayingMiniDock({
  playerState,
  isTrackLoading,
  isPlaying,
  dragPosition,
  onTogglePlayback,
  onPrevious,
  onNext,
  onSeekPreview,
  onSeekCommit,
  onCloseLyrics,
  onOpenImmersive,
}: NowPlayingMiniDockProps) {
  const current = playerState?.current;
  const duration = safeDuration(playerState?.duration_seconds ?? 0);
  const position = safePosition(
    dragPosition ?? playerState?.position_seconds ?? 0,
    duration,
  );
  const [albumColor, setAlbumColor] = useState<string | null>(null);

  useEffect(() => {
    const url = current?.thumbnail_url;
    if (!url) {
      setAlbumColor(null);
      return;
    }
    extractDominantColor(url).then(setAlbumColor);
  }, [current?.thumbnail_url]);

  if (!current && !isTrackLoading) return null;

  return (
    <div
      className="liquid-card relative mt-auto flex flex-col overflow-hidden rounded-2xl border border-white/10 shadow-xl backdrop-blur-xl animate-in fade-in slide-in-from-bottom-2 duration-200"
      role="region"
      aria-label="Reproductor compacto de letras"
    >
      {/* Barra de progreso interactiva fina superior */}
      {current && (
        <div className="relative px-2 pt-1 pb-0.5">
          <Slider
            className="h-1 cursor-pointer"
            value={[position]}
            max={Math.max(duration, 1)}
            step={1}
            variant="glass"
            onValueChange={([value]) => onSeekPreview(value)}
            onValueCommit={([value]) => onSeekCommit(value)}
            aria-label="Posición de reproducción"
          />
        </div>
      )}

      <div className="flex items-center gap-2.5 px-3 py-1.5">
        {/* Carátula */}
        <div
          onClick={onOpenImmersive}
          className="relative size-8 shrink-0 overflow-hidden rounded-lg border border-white/10 bg-white/5 cursor-pointer hover:scale-105 active:scale-95 transition-all"
          title="Abrir vista inmersiva Apple Music (I)"
        >
          {current?.thumbnail_url ? (
            <img
              src={current.thumbnail_url}
              alt=""
              className="size-full object-cover"
              onError={(e) => {
                (e.currentTarget as HTMLElement).style.display = "none";
              }}
            />
          ) : (
            <div className="flex size-full items-center justify-center bg-teal-500/10">
              <Headphones className="size-4 text-teal-300" />
            </div>
          )}
        </div>

        {/* Título y Artista */}
        <div className="flex min-w-0 flex-1 flex-col justify-center">
          <div className="flex items-center gap-1.5">
            <span
              className="size-1.5 shrink-0 rounded-full transition-all"
              style={{
                backgroundColor: isPlaying ? (albumColor ?? "#2dd4bf") : undefined,
                boxShadow: isPlaying ? `0 0 6px ${albumColor ?? "#2dd4bf"}` : undefined,
                animation: isPlaying ? "pulse 2s infinite" : undefined,
              }}
            />
            <p className="truncate text-xs font-semibold text-white/95">
              {current?.title || (isTrackLoading ? "Cargando pista..." : "Sin reproducción")}
            </p>
          </div>
          <div className="flex items-center gap-2 pl-3">
            <p className="truncate text-[10px] text-white/50">
              {current?.artist || "Cadence"}
            </p>
            <span className="text-[9px] tabular-nums text-white/40 shrink-0">
              {formatDuration(position)} / {formatDuration(duration)}
            </span>
          </div>
        </div>

        {/* Controles de reproducción rápida */}
        <div className="flex items-center gap-1 shrink-0">
          <GlassButton
            variant="ghost"
            size="sm"
            aria-label="Pista anterior (P)"
            title="Pista anterior (P)"
            onClick={onPrevious}
            className="border-white/5 hover:bg-white/[0.1] hover:text-white"
          >
            <SkipBack className="size-3 fill-current" />
          </GlassButton>

          <GlassButton
            variant="primary"
            size="sm"
            aria-label={isPlaying ? "Pausar (Espacio)" : "Reproducir (Espacio)"}
            title={isPlaying ? "Pausar (Espacio)" : "Reproducir (Espacio)"}
            onClick={onTogglePlayback}
          >
            {isPlaying ? (
              <Pause className="size-3.5 fill-current" />
            ) : (
              <Play className="size-3.5 ml-0.5 fill-current" />
            )}
          </GlassButton>

          <GlassButton
            variant="ghost"
            size="sm"
            aria-label="Siguiente pista (N)"
            title="Siguiente pista (N)"
            onClick={onNext}
            className="border-white/5 hover:bg-white/[0.1] hover:text-white"
          >
            <SkipForward className="size-3 fill-current" />
          </GlassButton>

          {onOpenImmersive && (
            <GlassButton
              variant="ghost"
              size="sm"
              aria-label="Modo Inmersivo (I)"
              title="Modo Inmersivo Apple Music (I)"
              onClick={onOpenImmersive}
              className="border-white/5 hover:bg-white/[0.1] hover:text-white text-amber-300"
            >
              <Sparkles className="size-3" />
            </GlassButton>
          )}

          <GlassButton
            variant="ghost"
            size="sm"
            aria-label="Cerrar letras y volver a biblioteca (Esc)"
            title="Cerrar letras (Esc)"
            onClick={onCloseLyrics}
            className="border-white/5 hover:bg-white/[0.1] hover:text-white text-indigo-300 ml-0.5"
          >
            <ChevronDown className="size-4" />
          </GlassButton>
        </div>
      </div>
    </div>
  );
});

// ===== Main App =====

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
  const [queue, setQueue] = useState<QueueItem[]>([]);
  const [isQueueOpen, setIsQueueOpen] = useState(false);
  const [isLyricsOpen, setIsLyricsOpen] = useState(false);
  const [isImmersiveOpen, setIsImmersiveOpen] = useState(false);
  const [lyricsDoc, setLyricsDoc] = useState<LyricsDoc | null>(null);
  const [isLyricsLoading, setIsLyricsLoading] = useState(false);
  const [prevVolume, setPrevVolume] = useState<number>(100);
  const [focusState, setFocusState] = useState<FocusState | null>(null);
  const [isWide, setIsWide] = useState(
    typeof window !== "undefined" ? window.innerWidth >= 768 : false
  );
  const [isMaximized, setIsMaximized] = useState(false);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const playerStateRef = useRef<PlayerState | null>(null);
  const prevVolumeRef = useRef<number>(100);
  const searchAbortRef = useRef<AbortController | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);
  const seekGuardRef = useRef<{ target: number; at: number } | null>(null);

  useEffect(() => {
    const handleResize = () => {
      setIsWide(window.innerWidth >= 768);
    };
    window.addEventListener("resize", handleResize);

    getCurrentWindow().isMaximized().then(setIsMaximized).catch(() => {});
    const unlistenPromise = getCurrentWindow().onResized(() => {
      getCurrentWindow().isMaximized().then(setIsMaximized).catch(() => {});
      setIsWide(window.innerWidth >= 768);
    });

    return () => {
      window.removeEventListener("resize", handleResize);
      unlistenPromise.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);

  const handleToggleMaximize = useCallback(async () => {
    try {
      const max = await getCurrentWindow().isMaximized();
      if (max) {
        await getCurrentWindow().unmaximize();
        setIsMaximized(false);
      } else {
        await getCurrentWindow().maximize();
        setIsMaximized(true);
      }
    } catch (err) {
      console.error("Error toggle maximize:", err);
    }
  }, []);

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

    // Cargar cola inicial y focus
    getQueue().then(setQueue).catch(console.error);
    getFocusState().then(setFocusState).catch(console.error);
    const unlistenFocusPromise = listenFocusEvents(setFocusState);

    // Health check inicial
    health()
      .then((h) => {
        if (!h.mpv) {
          setError("mpv no está disponible en el sistema.");
        } else if (!h.yt_dlp) {
          setError("yt-dlp no está disponible en el sistema.");
        }
      })
      .catch((err) => console.error("health() check failed:", err));

    // Suscribirse a eventos del orquestador
    listenOrchestratorEvents(handleOrchestratorEvent).then((unlisten) => {
      unlistenRef.current = unlisten;
    });

    // Suscribirse a eventos Tauri (para que el backend emita)
    subscribeEvents().catch((err) => console.error("subscribeEvents failed:", err));

    // Suscribirse a eventos de letras
    const unlistenLyricsPromise = listenLyricsEvents((event: LyricsEvent) => {
      if (event.type === "Ready") {
        setLyricsDoc(event.data);
        setIsLyricsLoading(false);
      } else if (event.type === "Unavailable") {
        setIsLyricsLoading(false);
      }
    });

    // Fallback polling (cada 5s) solo para recovery
    const poll = async () => {
      try {
        setPlayerState(await getState());
      } catch (err) {
        console.error("state() poll failed:", err);
      }
    };
    poll();
    const interval = setInterval(poll, POLL_INTERVAL_MS);

    return () => {
      clearInterval(interval);
      unlistenRef.current?.();
      unlistenFocusPromise.then((unlisten) => unlisten()).catch(console.error);
      unlistenLyricsPromise.then((unlisten) => unlisten()).catch(console.error);
    };
  }, []);

  // Manejar eventos del orquestador
  const handleOrchestratorEvent = useCallback((event: OrchestratorEvent) => {
    switch (event.type) {
      case "Progress":
        // Patch local: evita un round-trip IPC a 2 Hz. Mientras se arrastra el
        // slider, NowPlaying ignora esto y usa `dragPosition`.
        setPlayerState((prev) => {
          if (!prev) return prev;
          const guard = seekGuardRef.current;
          let position = event.data.position_seconds;
          if (guard) {
            if (Date.now() - guard.at > SEEK_GUARD_MS) {
              seekGuardRef.current = null;
            } else if (Math.abs(position - guard.target) > 1.5) {
              // mpv todavía reporta la posición anterior al seek.
              position = prev.position_seconds;
            } else {
              seekGuardRef.current = null;
            }
          }
          return {
            ...prev,
            position_seconds: position,
            duration_seconds: event.data.duration_seconds,
          };
        });
        break;
      case "ConnectionChanged":
        setPlayerState((prev) =>
          prev ? { ...prev, connected: event.data.connected } : prev,
        );
        if (event.data.connected) {
          getState().then(setPlayerState).catch(console.error);
        }
        break;
      case "QueueUpdated":
        setQueue(event.data);
        break;
      case "AudioPresetChanged":
        setPlayerState((prev) => (prev ? { ...prev, audio_preset: event.data } : prev));
        break;
      case "SpeedChanged":
        setPlayerState((prev) => (prev ? { ...prev, speed: event.data } : prev));
        break;
      case "PlaybackError":
        setError(event.data);
        getState().then(setPlayerState).catch(console.error);
        break;
      case "TrackStarted":
        setLyricsDoc(null);
        setIsLyricsLoading(true);
        getState().then(setPlayerState).catch(console.error);
        break;
      case "TrackEnded":
      case "StateChanged":
        getState().then(setPlayerState).catch(console.error);
        break;
    }
  }, []);

  const handleNext = useCallback(async () => {
    try {
      await nextTrack();
    } catch (err) {
      console.error("nextTrack error:", err);
    }
  }, []);

  const handlePrevious = useCallback(async () => {
    try {
      await previousTrack();
    } catch (err) {
      console.error("previousTrack error:", err);
    }
  }, []);

  const handleAddToQueue = useCallback(async (track: SearchResult, e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await addToQueue(track);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const handleRemoveFromQueue = useCallback(async (itemId: string, e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await removeFromQueue(itemId);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const handleClearQueue = useCallback(async () => {
    try {
      await clearQueue();
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const handleToggleLyrics = useCallback(() => {
    setIsLyricsOpen((prev) => {
      const next = !prev;
      if (next && !lyricsDoc && playerState?.current) {
        setIsLyricsLoading(true);
        getLyrics({
          track_id: playerState.current.id,
          title: playerState.current.title,
          artist: playerState.current.artist,
          duration_seconds: playerState.current.duration_seconds,
        })
          .then((doc) => {
            setLyricsDoc(doc);
            setIsLyricsLoading(false);
          })
          .catch((err) => {
            console.error("Error al cargar letra inicial:", err);
            setIsLyricsLoading(false);
          });
      }
      return next;
    });
  }, [lyricsDoc, playerState?.current]);

  const handlePlayQueueIndex = useCallback(async (index: number) => {
    try {
      await playQueueIndex(index);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const handleStartFocus = useCallback(async () => {
    try {
      await startFocus();
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const handlePauseFocus = useCallback(async () => {
    try {
      await pauseFocus();
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const handleResetFocus = useCallback(async () => {
    try {
      await resetFocus();
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const handleSkipFocus = useCallback(async () => {
    try {
      await skipFocus();
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const handleSelectPreset = useCallback(async (preset: AudioPreset) => {
    setPlayerState((prev) => (prev ? { ...prev, audio_preset: preset } : prev));
    try {
      await setAudioPreset(preset);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const handleCycleSpeed = useCallback(async () => {
    const curSpeed = playerStateRef.current?.speed ?? 1.0;
    const nextSpeed =
      curSpeed === 1.0 ? 1.15 : curSpeed === 1.15 ? 1.25 : curSpeed === 1.25 ? 0.85 : 1.0;
    setPlayerState((prev) => (prev ? { ...prev, speed: nextSpeed } : prev));
    try {
      await setSpeed(nextSpeed);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const isYoutubeUrl = useMemo(() => {
    const t = query.trim();
    return (
      t.includes("youtube.com") ||
      t.includes("youtu.be") ||
      (t.length === 11 && !t.includes(" "))
    );
  }, [query]);

  const isLocalAudioPath = useMemo(() => {
    const t = query.trim();
    const audioExts = [".mp3", ".flac", ".wav", ".m4a", ".opus", ".ogg", ".aac"];
    return audioExts.some((ext) => t.toLowerCase().endsWith(ext));
  }, [query]);

  const [isDragOver, setIsDragOver] = useState(false);
  const [isFocusView, setIsFocusView] = useState(false);

  const handleSelectFocusDuration = useCallback(async (focusMins: number, breakMins: number) => {
    try {
      await setFocusDurations(focusMins * 60, breakMins * 60, (breakMins * 2) * 60);
      setFocusState((prev) =>
        prev
          ? {
              ...prev,
              remaining_seconds: focusMins * 60,
              total_seconds: focusMins * 60,
            }
          : prev
      );
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const handleToggleAutoPause = useCallback(async (enabled: boolean) => {
    try {
      await setFocusAutoPause(enabled);
      setFocusState((prev) => (prev ? { ...prev, auto_pause_on_break: enabled } : prev));
    } catch (err) {
      setError(String(err));
    }
  }, []);

  // Pausar animaciones en segundo plano para mínimo consumo de CPU/GPU
  useEffect(() => {
    const updateHiddenState = () => {
      const isHidden = document.hidden;
      document.documentElement.setAttribute(
        "data-window-hidden",
        isHidden ? "true" : "false",
      );
    };
    updateHiddenState();
    document.addEventListener("visibilitychange", updateHiddenState);
    window.addEventListener("blur", updateHiddenState);
    window.addEventListener("focus", updateHiddenState);
    return () => {
      document.removeEventListener("visibilitychange", updateHiddenState);
      window.removeEventListener("blur", updateHiddenState);
      window.removeEventListener("focus", updateHiddenState);
    };
  }, []);

  useEffect(() => {
    function handleGlobalKeyDown(e: KeyboardEvent) {
      const isSearchFocused = document.activeElement === searchInputRef.current;

      // Al buscar desde Focus, volvemos a la biblioteca sin tocar el temporizador.
      const focusSearch = (select = false) => {
        if (isFocusView) setIsFocusView(false);
        window.setTimeout(() => {
          searchInputRef.current?.focus();
          if (select) searchInputRef.current?.select();
        }, 0);
      };

      if (e.key === "Escape") {
        if (isImmersiveOpen) {
          e.preventDefault();
          setIsImmersiveOpen(false);
          return;
        }
        if (isLyricsOpen) {
          e.preventDefault();
          setIsLyricsOpen(false);
          return;
        }
        if (isFocusView) {
          e.preventDefault();
          setIsFocusView(false);
          return;
        }
      } else if (!isSearchFocused && e.shiftKey && (e.key === "F" || e.key === "f")) {
        e.preventDefault();
        setIsFocusView((prev) => !prev);
      } else if (!isSearchFocused && !e.shiftKey && (e.key === "i" || e.key === "I")) {
        e.preventDefault();
        setIsImmersiveOpen((prev) => !prev);
      } else if (e.key === "/" && !isSearchFocused) {
        e.preventDefault();
        focusSearch();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        focusSearch(true);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "q") {
        e.preventDefault();
        quit();
      } else if (!isSearchFocused && (e.code === "Space" || e.key === " ")) {
        e.preventDefault();
        handleTogglePlayback();
      } else if (!isSearchFocused && (e.key === "n" || e.key === "N")) {
        e.preventDefault();
        handleNext();
      } else if (!isSearchFocused && (e.key === "p" || e.key === "P")) {
        e.preventDefault();
        handlePrevious();
      } else if (!isSearchFocused && (e.key === "q" || e.key === "Q")) {
        e.preventDefault();
        setIsQueueOpen((prev) => !prev);
      } else if (!isSearchFocused && (e.key === "y" || e.key === "Y")) {
        e.preventDefault();
        handleToggleLyrics();
      } else if (!isSearchFocused && (e.key === "f" || e.key === "F")) {
        e.preventDefault();
        if (focusState?.is_running) {
          handlePauseFocus();
        } else {
          handleStartFocus();
        }
      } else if (!isSearchFocused && e.key.toLowerCase() === "m") {
        e.preventDefault();
        handleToggleMute();
      } else if (!isSearchFocused && e.key.toLowerCase() === "l") {
        e.preventDefault();
        handleToggleLoop();
      } else if (!isSearchFocused && e.key === "ArrowLeft") {
        e.preventDefault();
        handleSeekBy(-5);
      } else if (!isSearchFocused && e.key === "ArrowRight") {
        e.preventDefault();
        handleSeekBy(5);
      }
    }
    document.addEventListener("keydown", handleGlobalKeyDown);
    return () => document.removeEventListener("keydown", handleGlobalKeyDown);
  }, [
    focusState?.is_running,
    isFocusView,
    isLyricsOpen,
    isImmersiveOpen,
    handleNext,
    handlePauseFocus,
    handlePrevious,
    handleStartFocus,
    handleToggleLoop,
    handleToggleMute,
    handleTogglePlayback,
    handleToggleLyrics,
  ]);

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

    const audioExts = [".mp3", ".flac", ".wav", ".m4a", ".opus", ".ogg", ".aac"];
    if (audioExts.some((ext) => q.trim().toLowerCase().endsWith(ext))) {
      const path = q.trim();
      const name =
        path.split("/").pop()?.split("\\").pop()?.replace(/\.[^/.]+$/, "") ??
        "Pista Local";
      const localTrack: SearchResult = {
        id: path,
        title: name,
        artist: "Archivo Local",
        duration_seconds: 0,
        thumbnail_url: "",
      };
      setResults([localTrack]);
      setResultsQuery(q);
      setSelectedIndex(0);
      return;
    }
    
    // Cancelar búsqueda anterior
    searchAbortRef.current?.abort();
    const controller = new AbortController();
    searchAbortRef.current = controller;

    setError(null);
    setIsSearching(true);
    try {
      const found = await search(q, 10);
      // Verificar si fue abortado
      if (controller.signal.aborted) return;
      setResults(found);
      setResultsQuery(q);
      setSelectedIndex(null);
    } catch (err) {
      if (err instanceof Error && err.name === "AbortError") return;
      setError(String(err));
    } finally {
      if (!controller.signal.aborted) {
        setIsSearching(false);
      }
    }
  }

  async function handlePlay(track: SearchResult) {
    setError(null);
    setLoadingTrackId(track.id);
    setDragPosition(null);
    seekGuardRef.current = null;
    saveToRecent(track);
    try {
      await play(track);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoadingTrackId(null);
    }
  }

  // Soporte Drag & Drop para reproducir canciones locales instantáneamente
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    getCurrentWindow()
      .onDragDropEvent((event) => {
        if (event.payload.type === "over") {
          setIsDragOver(true);
        } else if (event.payload.type === "drop") {
          setIsDragOver(false);
          const paths = event.payload.paths;
          const audioExts = [".mp3", ".flac", ".wav", ".m4a", ".opus", ".ogg", ".aac"];
          const audioPaths = paths.filter((p) =>
            audioExts.some((ext) => p.toLowerCase().endsWith(ext)),
          );
          if (audioPaths.length > 0) {
            const first = audioPaths[0];
            const name =
              first.split("/").pop()?.split("\\").pop()?.replace(/\.[^/.]+$/, "") ??
              "Pista Local";
            const firstTrack: SearchResult = {
              id: first,
              title: name,
              artist: "Archivo Local",
              duration_seconds: 0,
              thumbnail_url: "",
            };
            handlePlay(firstTrack);

            for (let i = 1; i < audioPaths.length; i++) {
              const path = audioPaths[i];
              const trackName =
                path.split("/").pop()?.split("\\").pop()?.replace(/\.[^/.]+$/, "") ??
                "Pista Local";
              const trackItem: SearchResult = {
                id: path,
                title: trackName,
                artist: "Archivo Local",
                duration_seconds: 0,
                thumbnail_url: "",
              };
              addToQueue(trackItem);
            }
          }
        } else {
          setIsDragOver(false);
        }
      })
      .then((fn) => {
        unlisten = fn;
      })
      .catch((err) => {
        console.warn("DragDropEvent no soportado o falló:", err);
      });

    return () => {
      unlisten?.();
    };
  }, [handlePlay]);

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
    const state = playerStateRef.current;
    if (!state?.current) return;

    const wasPlaying = state.status === "Playing";
    setError(null);
    // Optimista: el icono responde al instante; el evento/estado confirma.
    setPlayerState((prev) =>
      prev ? { ...prev, status: wasPlaying ? "Paused" : "Playing" } : prev,
    );
    try {
      if (wasPlaying) {
        await pause();
      } else {
        await resume();
      }
    } catch (err) {
      setError(String(err));
      getState().then(setPlayerState).catch(console.error);
    }
  }

  async function handleToggleLoop() {
    const state = playerStateRef.current;
    if (!state) return;

    const next = !state.looping;
    setPlayerState((prev) => (prev ? { ...prev, looping: next } : prev));
    try {
      await setLoop(next);
    } catch (err) {
      setError(String(err));
      getState().then(setPlayerState).catch(console.error);
    }
  }

  async function handleStop() {
    setError(null);
    seekGuardRef.current = null;
    try {
      await stop();
      setPlayerState((prev) =>
        prev
          ? { ...prev, status: "Stopped", current: null, position_seconds: 0 }
          : prev,
      );
      setDragPosition(null);
    } catch (err) {
      setError(String(err));
    }
  }

  /** Preview local: el slider se mueve sin saturar el IPC de mpv. */
  function handleVolumePreview(level: number) {
    const volume = Math.round(level);
    if (volume > 0) {
      setPrevVolume(volume);
    }
    setPlayerState((prev) => (prev ? { ...prev, volume } : prev));
  }

  /** Commit al soltar el slider: aquí sí hablamos con mpv. */
  async function handleVolumeCommit(level: number) {
    const volume = Math.round(level);
    handleVolumePreview(volume);
    try {
      await setVolume(volume);
    } catch (err) {
      setError(String(err));
      getState().then(setPlayerState).catch(console.error);
    }
  }

  async function handleToggleMute() {
    const curVol = playerStateRef.current?.volume ?? 100;
    const next = curVol === 0 ? (prevVolumeRef.current > 0 ? prevVolumeRef.current : 100) : 0;
    if (next > 0) {
      setPrevVolume(next);
    }
    setPlayerState((prev) => (prev ? { ...prev, volume: next } : prev));
    try {
      await setVolume(next);
    } catch (err) {
      setError(String(err));
      getState().then(setPlayerState).catch(console.error);
    }
  }

  async function handleSeekCommit(positionSeconds: number) {
    const state = playerStateRef.current;
    const target = safePosition(positionSeconds, state?.duration_seconds ?? 0);

    seekGuardRef.current = { target, at: Date.now() };
    setDragPosition(null);
    setPlayerState((prev) =>
      prev ? { ...prev, position_seconds: target } : prev,
    );
    try {
      await seek(target);
    } catch (err) {
      setError(String(err));
      getState().then(setPlayerState).catch(console.error);
    }
  }

  function handleSeekBy(deltaSeconds: number) {
    const state = playerStateRef.current;
    if (!state?.current) return;
    const target = seekTarget(
      state.position_seconds,
      deltaSeconds,
      state.duration_seconds,
    );
    handleSeekCommit(target);
  }

  const volume = playerState?.volume ?? 100;
  const isPlaying = playerState?.status === "Playing";
  const isLooping = playerState?.looping ?? false;
  const isTrackLoading = loadingTrackId !== null;

  // Memoized derived values
  const searchResults = useMemo(() => results, [results]);
  const recentTracksMemo = useMemo(() => recentTracks, [recentTracks]);

  return (
    <main className="flex h-screen w-screen p-2.5">
      <MeshBackground playing={isPlaying} />
      <GlassPanel pressed={false} className="relative flex h-full w-full flex-col gap-3 rounded-[24px] p-4 overflow-hidden">
        {isDragOver && (
          <div className="absolute inset-0 z-50 flex flex-col items-center justify-center gap-3 bg-black/85 backdrop-blur-md rounded-[24px] border-2 border-dashed border-teal-400 select-none animate-in fade-in duration-150">
            <Headphones className="size-12 text-teal-300 animate-bounce" />
            <div className="text-center">
              <p className="text-sm font-semibold text-white">Suelta canciones aquí</p>
              <p className="text-xs text-white/60">Reproducción instantánea (.mp3, .flac, .wav, .m4a, .opus)</p>
            </div>
          </div>
        )}
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

          <FocusWidget
            focusState={focusState}
            isActive={isFocusView}
            onOpen={() => setIsFocusView(true)}
            onStart={handleStartFocus}
            onPause={handlePauseFocus}
          />

          <div className="flex items-center gap-1.5">
            <GlassButton
              variant="ghost"
              size="sm"
              aria-label={isMaximized ? "Restaurar ventana" : "Maximizar a Split View"}
              title={isMaximized ? "Restaurar (Compacto)" : "Maximizar (Apple Music Split View)"}
              onClick={handleToggleMaximize}
            >
              <Maximize2 className="size-3" />
            </GlassButton>
            <GlassButton variant="ghost" size="sm" aria-label="Minimizar" onClick={() => getCurrentWindow().minimize()}>
              <Minus className="size-3" />
            </GlassButton>
            <GlassButton variant="ghost" size="sm" aria-label="Ocultar" onClick={() => getCurrentWindow().hide()}>
              <X className="size-3" />
            </GlassButton>
          </div>
        </header>

        {isFocusView ? (
          focusState ? (
            <FocusStudio
              focusState={focusState}
              currentTrack={playerState?.current ?? null}
              isPlaying={isPlaying}
              onStart={handleStartFocus}
              onPause={handlePauseFocus}
              onReset={handleResetFocus}
              onSkip={handleSkipFocus}
              onSelectDuration={handleSelectFocusDuration}
              onToggleAutoPause={handleToggleAutoPause}
              onOpenLibrary={() => setIsFocusView(false)}
              onClose={() => setIsFocusView(false)}
            />
          ) : (
            <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 rounded-[22px] border border-white/10 bg-white/[0.02] text-center">
              <Loader2 className="size-5 animate-spin text-teal-300" />
              <p className="text-xs text-white/55">Preparando Focus Studio…</p>
            </div>
          )
        ) : (
          /* Layout Principal: Modo Compacto (< 768px) o Split View Apple Music (>= 768px) */
          <div className="flex-1 min-h-0 flex flex-col md:flex-row gap-3.5 overflow-hidden">
            {/* Columna Biblioteca / Búsqueda (Visible en modo ancho o cuando las letras no están activas) */}
            {(!isLyricsOpen || isWide) && (
              <div className={cn(
                "flex flex-col min-h-0 gap-3 overflow-hidden transition-all duration-300",
                isWide && isLyricsOpen ? "flex-1 md:w-[54%] lg:w-[56%]" : "flex-1 w-full"
              )}>
                {/* Buscador de vidrio líquido */}
                <div className="relative flex items-center gap-2">
                  <div className="relative flex-1">
                    <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-white/50" />
                    <Input
                      ref={searchInputRef}
                      value={query}
                      onChange={(e) => setQuery(e.currentTarget.value)}
                      onKeyDown={handleSearchKeyDown}
                      placeholder="Buscar en YouTube, pegar enlace o arrastrar archivo local..."
                      autoComplete="off"
                      className={cn(
                        "h-10 pl-9 rounded-xl bg-white/[0.06] backdrop-blur-md border-white/10 hover:border-white/20 focus-visible:border-teal-400/50 focus-visible:ring-2 focus-visible:ring-teal-400/20 text-xs transition-all placeholder:text-white/40",
                        isYoutubeUrl || isLocalAudioPath ? "pr-24" : "pr-9",
                      )}
                    />
                    {isYoutubeUrl && (
                      <div className="absolute right-9 top-1/2 -translate-y-1/2 flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-teal-400/20 border border-teal-400/30 text-[10px] text-teal-300 pointer-events-none select-none">
                        <Link2 className="size-3" />
                        <span>Enlace</span>
                      </div>
                    )}
                    {isLocalAudioPath && (
                      <div className="absolute right-9 top-1/2 -translate-y-1/2 flex items-center gap-1 px-1.5 py-0.5 rounded-md bg-emerald-400/20 border border-emerald-400/30 text-[10px] text-emerald-300 pointer-events-none select-none">
                        <Headphones className="size-3" />
                        <span>Local</span>
                      </div>
                    )}
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
                  <GlassButton
                    type="button"
                    size="md"
                    aria-label="Buscar"
                    onClick={() => runSearch(query)}
                    disabled={isSearching}
                  >
                    {isSearching ? (
                      <Loader2 className="size-4 animate-spin text-teal-300" />
                    ) : (
                      <Search className="size-4 text-white/80" />
                    )}
                  </GlassButton>
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

                {playerState?.connected === false && (
                  <div
                    className="flex items-center gap-2 rounded-lg bg-amber-400/15 border border-amber-400/30 px-3 py-1.5 text-xs text-amber-200"
                    role="status"
                    aria-live="polite"
                  >
                    <Loader2 className="size-3.5 shrink-0 animate-spin" />
                    <span className="flex-1">
                      Reconectando con el reproductor… la reproducción se reanudará sola.
                    </span>
                  </div>
                )}

                {isQueueOpen ? (
                  <QueuePanel
                    queue={queue}
                    currentTrackId={playerState?.current?.id}
                    onPlayItem={handlePlayQueueIndex}
                    onRemoveItem={handleRemoveFromQueue}
                    onClearQueue={handleClearQueue}
                    onClose={() => setIsQueueOpen(false)}
                  />
                ) : (
                  <>
                    {/* Resultados de Búsqueda */}
                    {results.length > 0 && (
                      <div className="flex min-h-0 flex-1 flex-col gap-2">
                        <ScrollArea className="min-h-0 flex-1 pr-1">
                          <ol className="flex flex-col gap-1.5">
                            {searchResults.map((track, index) => {
                              const isCurrentItemLoading = loadingTrackId === track.id;
                              const isCurrentTrackPlaying = playerState?.current?.id === track.id;

                              return (
                                <SearchResultItem
                                  key={track.id}
                                  track={track}
                                  isSelected={index === selectedIndex}
                                  isPlaying={isCurrentTrackPlaying}
                                  isLoading={isCurrentItemLoading}
                                  onClick={() => handlePlay(track)}
                                  onAddToQueue={(e) => handleAddToQueue(track, e)}
                                  onMouseEnter={() => setSelectedIndex(index)}
                                />
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
                              N / P
                            </kbd>
                            Sig/Ant
                          </span>
                          <span className="flex items-center gap-1">
                            <kbd className="rounded bg-white/10 px-1 py-0.2 font-mono text-[10px]">
                              Q
                            </kbd>
                            Cola
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

                    {/* Album Hero banner estilo Apple Music cuando hay pista activa y no hay búsqueda */}
                    {results.length === 0 && resultsQuery === "" && playerState?.current && (
                      <AlbumHero
                        currentTrack={playerState.current}
                        isPlaying={isPlaying}
                        isLooping={isLooping}
                        isLyricsOpen={isLyricsOpen}
                        onTogglePlayback={handleTogglePlayback}
                        onToggleLoop={handleToggleLoop}
                        onToggleLyrics={handleToggleLyrics}
                        onOpenImmersive={() => setIsImmersiveOpen(true)}
                      />
                    )}

                    {/* Historial Reciente (cuando no hay búsqueda activa) */}
                    {results.length === 0 && resultsQuery === "" && recentTracksMemo.length > 0 && (
                      <div className="flex min-h-0 flex-1 flex-col gap-2">
                        <div className="flex items-center justify-between px-1">
                          <span className="flex items-center gap-1.5 text-xs font-medium text-white/70">
                            <Clock className="size-3.5 text-teal-400" />
                            Recientes
                          </span>
                          <button
                            type="button"
                            onClick={clearRecent}
                            className="text-[10px] text-white/50 hover:text-white/90 transition-colors cursor-pointer"
                          >
                            Limpiar
                          </button>
                        </div>
                        <ScrollArea className="min-h-0 flex-1 pr-1">
                          <ol className="flex flex-col gap-1.5">
                            {recentTracksMemo.map((track) => {
                              const isCurrentItemLoading = loadingTrackId === track.id;
                              const isCurrentTrackPlaying = playerState?.current?.id === track.id;

                              return (
                                <RecentTrackItem
                                  key={track.id}
                                  track={track}
                                  isPlaying={isCurrentTrackPlaying}
                                  isLoading={isCurrentItemLoading}
                                  onClick={() => handlePlay(track)}
                                  onAddToQueue={(e) => handleAddToQueue(track, e)}
                                />
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

                    {/* Pantalla vacía inicial de concentración si no hay nada en reproducción */}
                    {results.length === 0 && resultsQuery === "" && !playerState?.current && recentTracksMemo.length === 0 && (
                      <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center p-6 select-none">
                        <div className="size-14 rounded-2xl liquid-pill flex items-center justify-center text-teal-300/80 shadow-lg">
                          <Headphones className="size-7" />
                        </div>
                        <div className="flex flex-col gap-1">
                          <p className="text-sm font-medium text-white/90">Espacio de concentración</p>
                          <p className="text-xs text-white/60 max-w-[240px]">
                            Busca cualquier tema en YouTube o pega un enlace directo y empieza tu sesión.
                          </p>
                        </div>
                      </div>
                    )}
                  </>
                )}
              </div>
            )}

            {/* Columna Derecha / Split View de Letras (Visible cuando las letras estén activas) */}
            {isLyricsOpen && (
              <div className={cn(
                "min-h-0 flex flex-col transition-all duration-300",
                isWide ? "flex-1 md:w-[46%] lg:w-[44%] animate-in fade-in slide-in-from-right-4 duration-300" : "flex-1 w-full"
              )}>
                <LyricsPanel
                  currentTrack={playerState?.current ?? null}
                  lyricsDoc={lyricsDoc}
                  isLoading={isLyricsLoading}
                  positionSeconds={playerState?.position_seconds ?? 0}
                  isPlaying={isPlaying}
                  speed={playerState?.speed ?? 1.0}
                  onClose={() => setIsLyricsOpen(false)}
                  onSeek={handleSeekCommit}
                  onLyricsUpdated={(updated) => setLyricsDoc(updated)}
                />
              </div>
            )}
          </div>
        )}

        {/* Música: dock compacto en Focus y Letras (en pantalla compacta), reproductor flotante en Biblioteca o Split View */}
        {isFocusView ? (
          <FocusNowPlayingDock
            playerState={playerState}
            isTrackLoading={isTrackLoading}
            isPlaying={isPlaying}
            onTogglePlayback={handleTogglePlayback}
            onNext={handleNext}
            onOpenLibrary={() => setIsFocusView(false)}
          />
        ) : (isLyricsOpen && !isWide) ? (
          <NowPlayingMiniDock
            playerState={playerState}
            isTrackLoading={isTrackLoading}
            isPlaying={isPlaying}
            dragPosition={dragPosition}
            onTogglePlayback={handleTogglePlayback}
            onPrevious={handlePrevious}
            onNext={handleNext}
            onSeekPreview={setDragPosition}
            onSeekCommit={handleSeekCommit}
            onCloseLyrics={() => setIsLyricsOpen(false)}
            onOpenImmersive={() => setIsImmersiveOpen(true)}
          />
        ) : (
          (isTrackLoading || playerState?.current) && (
            <div className={cn("w-full transition-all duration-300", isWide && "max-w-2xl mx-auto")}>
              <NowPlaying
                playerState={playerState}
                isTrackLoading={isTrackLoading}
                isPlaying={isPlaying}
                isLooping={isLooping}
                dragPosition={dragPosition}
                onTogglePlayback={handleTogglePlayback}
                onToggleLoop={handleToggleLoop}
                onStop={handleStop}
                onNext={handleNext}
                onPrevious={handlePrevious}
                onToggleQueue={() => setIsQueueOpen((prev) => !prev)}
                isQueueOpen={isQueueOpen}
                queueCount={queue.length}
                onToggleLyrics={handleToggleLyrics}
                isLyricsOpen={isLyricsOpen}
                onSeekPreview={setDragPosition}
                onSeekCommit={handleSeekCommit}
                onVolumePreview={handleVolumePreview}
                onVolumeCommit={handleVolumeCommit}
                onToggleMute={handleToggleMute}
                volume={volume}
                onSelectPreset={handleSelectPreset}
                onCycleSpeed={handleCycleSpeed}
                onOpenImmersive={() => setIsImmersiveOpen(true)}
              />
            </div>
          )
        )}
      </GlassPanel>

      {/* Modo Inmersivo Teatro a Pantalla Completa estilo Apple Music */}
      {isImmersiveOpen && (
        <ImmersivePlayer
          currentTrack={playerState?.current ?? null}
          playerState={playerState}
          lyricsDoc={lyricsDoc}
          isLyricsLoading={isLyricsLoading}
          isPlaying={isPlaying}
          isLooping={isLooping}
          dragPosition={dragPosition}
          volume={volume}
          onTogglePlayback={handleTogglePlayback}
          onToggleLoop={handleToggleLoop}
          onPrevious={handlePrevious}
          onNext={handleNext}
          onSeekPreview={setDragPosition}
          onSeekCommit={handleSeekCommit}
          onVolumePreview={handleVolumePreview}
          onVolumeCommit={handleVolumeCommit}
          onToggleMute={handleToggleMute}
          onSelectPreset={handleSelectPreset}
          onClose={() => setIsImmersiveOpen(false)}
        />
      )}
    </main>
  );
}

export default App;