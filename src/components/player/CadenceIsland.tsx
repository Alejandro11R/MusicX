"use client";

import { Play, Pause, SkipForward, Maximize2, Music2, X } from "lucide-react";
import type { PlayerState, LyricsDoc } from "@/lib/tauri";
import { findActiveLineIndex } from "@/lib/lyrics";
import { formatDuration } from "@/lib/playback";
import { cn } from "@/lib/utils";

interface CadenceIslandProps {
  isOpen: boolean;
  onClose: () => void;
  playerState: PlayerState | null;
  lyricsDoc: LyricsDoc | null;
  onTogglePlayback: () => void;
  onNext: () => void;
  onExpand: () => void;
}

export function CadenceIsland({
  isOpen,
  onClose,
  playerState,
  lyricsDoc,
  onTogglePlayback,
  onNext,
  onExpand,
}: CadenceIslandProps) {
  if (!isOpen) return null;

  const current = playerState?.current;
  const isPlaying = playerState?.status === "Playing";
  const position = playerState?.position_seconds ?? 0;
  const duration = playerState?.duration_seconds ?? 0;

  // Encontrar la línea actual para el Lyric Peek
  const lines = lyricsDoc?.lines ?? [];
  const activeLineIndex = lines.length > 0 ? findActiveLineIndex(lines, position) : -1;
  const currentLyric = activeLineIndex >= 0 ? lines[activeLineIndex]?.text : null;

  return (
    <aside
      className="fixed bottom-6 left-1/2 -translate-x-1/2 z-50 flex items-center gap-3 px-4 py-2.5 rounded-full bg-neutral-950/85 backdrop-blur-2xl border border-white/20 shadow-[0_20px_50px_rgba(0,0,0,0.8)] text-white animate-in slide-in-from-bottom-5 duration-200 select-none max-w-[92vw] sm:max-w-lg"
      role="region"
      aria-label="Cadence Island Mini Player"
    >
      {/* Mini Portada con vinilo */}
      <div className="relative size-10 shrink-0">
        {current?.thumbnail_url ? (
          <img
            src={current.thumbnail_url}
            alt=""
            className={cn(
              "size-10 rounded-full object-cover border border-white/25 shadow-md",
              isPlaying && "animate-[spin_12s_linear_infinite]"
            )}
          />
        ) : (
          <div className="size-10 rounded-full bg-white/10 flex items-center justify-center border border-white/15">
            <Music2 className="size-4 text-teal-300" />
          </div>
        )}
        <div className="absolute inset-0 rounded-full border-2 border-white/10 pointer-events-none" />
      </div>

      {/* Info Canción & Lyric Peek dinámico */}
      <div className="flex-1 min-w-0 pr-1">
        <div className="flex items-center gap-2">
          <p className="text-xs font-semibold truncate text-white">
            {current?.title ?? "Sin reproducción activa"}
          </p>
          {isPlaying && (
            <span className="flex gap-0.5 items-end h-3 shrink-0">
              <span className="w-0.5 h-3 bg-teal-400 rounded-full animate-pulse" />
              <span className="w-0.5 h-2 bg-teal-400 rounded-full animate-pulse delay-75" />
              <span className="w-0.5 h-2.5 bg-teal-400 rounded-full animate-pulse delay-150" />
            </span>
          )}
        </div>

        {currentLyric ? (
          <p className="text-[11px] font-medium text-teal-300 truncate transition-all duration-200">
            {currentLyric}
          </p>
        ) : (
          <p className="text-[11px] text-white/50 truncate">
            {current?.artist ?? "Cadence Player"} • {formatDuration(position)} / {formatDuration(duration)}
          </p>
        )}
      </div>

      {/* Controles de transporte */}
      <div className="flex items-center gap-1.5 shrink-0">
        <button
          type="button"
          onClick={onTogglePlayback}
          className="size-8 rounded-full bg-white/15 hover:bg-white/25 active:scale-90 text-white flex items-center justify-center transition-all cursor-pointer"
          title={isPlaying ? "Pausar" : "Reanudar"}
        >
          {isPlaying ? <Pause className="size-4" /> : <Play className="size-4 ml-0.5" />}
        </button>

        <button
          type="button"
          onClick={onNext}
          className="size-7 rounded-full text-white/70 hover:text-white hover:bg-white/10 active:scale-90 flex items-center justify-center transition-all cursor-pointer"
          title="Siguiente pista"
        >
          <SkipForward className="size-3.5" />
        </button>

        <button
          type="button"
          onClick={onExpand}
          className="size-7 rounded-full text-white/70 hover:text-teal-300 hover:bg-white/10 active:scale-90 flex items-center justify-center transition-all cursor-pointer ml-1"
          title="Expandir a pantalla completa"
        >
          <Maximize2 className="size-3.5" />
        </button>

        <button
          type="button"
          onClick={onClose}
          className="size-6 rounded-full text-white/40 hover:text-white hover:bg-white/10 flex items-center justify-center transition-all cursor-pointer"
          title="Ocultar Island"
        >
          <X className="size-3" />
        </button>
      </div>
    </aside>
  );
}

export default CadenceIsland;
