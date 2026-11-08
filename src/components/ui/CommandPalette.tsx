"use client";

import { useEffect, useRef, useState, useMemo } from "react";
import {
  Search,
  Play,
  Pause,
  SkipForward,
  SkipBack,
  Volume2,
  Volume1,
  VolumeX,
  Sparkles,
  Music2,
  Timer,
  Layers,
  Repeat,
  Radio,
  Sliders,
  X,
  History,
  type LucideIcon,
} from "lucide-react";
import type { AudioPreset, PlayerState, SearchResult } from "@/lib/tauri";
import { cn } from "@/lib/utils";

export interface CommandPaletteProps {
  isOpen: boolean;
  onClose: () => void;
  onSearch: (q: string) => void;
  onPlayTrack: (track: SearchResult) => void;
  onTogglePlayback: () => void;
  onNext: () => void;
  onPrevious: () => void;
  onToggleMute: () => void;
  onToggleLoop: () => void;
  onSetVolume: (vol: number) => void;
  onSelectPreset: (preset: AudioPreset) => void;
  onToggleFocus: () => void;
  onToggleImmersive: () => void;
  onToggleQueue: () => void;
  recentTracks: SearchResult[];
  playerState: PlayerState | null;
}

type Category =
  | "Búsqueda Directa"
  | "Comandos de Reproducción"
  | "Vistas y Modos"
  | "Presets de Audio"
  | "Historial Reciente";

interface CommandItem {
  id: string;
  category: Category;
  label: string;
  description?: string;
  keywords?: string;
  icon: LucideIcon;
  active?: boolean;
  execute: () => void;
}

const CATEGORIES: readonly Category[] = [
  "Búsqueda Directa",
  "Comandos de Reproducción",
  "Vistas y Modos",
  "Presets de Audio",
  "Historial Reciente",
];

const PRESETS: ReadonlyArray<{
  value: AudioPreset;
  label: string;
  description: string;
  icon: LucideIcon;
}> = [
  {
    value: "normal",
    label: "Sonido Original / Plano",
    description: "Audio sin ecualizaciones añadidas",
    icon: Sliders,
  },
  {
    value: "lofi",
    label: "Lofi Chill & Warmth",
    description: "Filtro cálido y suave para relajación",
    icon: Sparkles,
  },
  {
    value: "bass_boost",
    label: "Bass Boost Intenso",
    description: "Realce potente de frecuencias bajas",
    icon: Volume2,
  },
  {
    value: "loudnorm",
    label: "Normalización EBU R128",
    description: "Nivela el volumen constante",
    icon: Radio,
  },
  {
    value: "slowed_reverb",
    label: "Slowed + Reverb",
    description: "Atmósfera espaciosa y ritmo pausado",
    icon: Music2,
  },
  {
    value: "nightcore",
    label: "Nightcore Enérgico",
    description: "Acelera el pitch y tempo +25%",
    icon: Sparkles,
  },
];

function normalize(value: string): string {
  return value
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLocaleLowerCase("es")
    .trim();
}

export function CommandPalette({
  isOpen,
  onClose,
  onSearch,
  onPlayTrack,
  onTogglePlayback,
  onNext,
  onPrevious,
  onToggleMute,
  onToggleLoop,
  onSetVolume,
  onSelectPreset,
  onToggleFocus,
  onToggleImmersive,
  onToggleQueue,
  recentTracks,
  playerState,
}: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const isPlaying = playerState?.status === "Playing";
  const isMuted = (playerState?.volume ?? 100) === 0;
  const isLooping = playerState?.looping ?? false;

  const systemCommands = useMemo<CommandItem[]>(() => {
    return [
      {
        id: "toggle-playback",
        category: "Comandos de Reproducción",
        label: isPlaying ? "Pausar reproducción" : "Reanudar música",
        description: isPlaying ? "Detener temporalmente el audio" : "Seguir reproduciendo la pista",
        keywords: "play pause pausar continuar",
        icon: isPlaying ? Pause : Play,
        execute: onTogglePlayback,
      },
      {
        id: "next-track",
        category: "Comandos de Reproducción",
        label: "Siguiente canción",
        description: "Avanzar a la próxima pista de la cola",
        keywords: "siguiente next avanzar saltar",
        icon: SkipForward,
        execute: onNext,
      },
      {
        id: "prev-track",
        category: "Comandos de Reproducción",
        label: "Canción anterior",
        description: "Volver a reproducir la pista anterior",
        keywords: "anterior prev atras volver",
        icon: SkipBack,
        execute: onPrevious,
      },
      {
        id: "toggle-mute",
        category: "Comandos de Reproducción",
        label: isMuted ? "Activar sonido" : "Silenciar audio (Mute)",
        description: isMuted ? "Restaurar nivel de volumen anterior" : "Poner volumen en cero",
        keywords: "mute silencio desilenciar volumen audio",
        icon: isMuted ? VolumeX : Volume2,
        execute: onToggleMute,
      },
      {
        id: "toggle-loop",
        category: "Comandos de Reproducción",
        label: isLooping ? "Desactivar repetición continua" : "Repetir canción en bucle",
        description: isLooping ? "Reproducción secuencial de la cola" : "Repetir el track actual indefinidamente",
        keywords: "loop bucle repetir repeticion",
        icon: Repeat,
        active: isLooping,
        execute: onToggleLoop,
      },
      {
        id: "vol-50",
        category: "Comandos de Reproducción",
        label: "Volumen al 50%",
        description: "Ajustar volumen a nivel medio cómodo",
        keywords: "volumen 50 mitad medio",
        icon: Volume1,
        execute: () => onSetVolume(50),
      },
      {
        id: "vol-100",
        category: "Comandos de Reproducción",
        label: "Volumen al 100%",
        description: "Ajustar volumen al máximo",
        keywords: "volumen 100 maximo full",
        icon: Volume2,
        execute: () => onSetVolume(100),
      },
      {
        id: "toggle-immersive",
        category: "Vistas y Modos",
        label: "Modo Inmersivo & Letras",
        description: "Abrir vista a pantalla completa con letras sincronizadas",
        keywords: "letras lyrics inmersivo karaoke pantalla",
        icon: Music2,
        execute: onToggleImmersive,
      },
      {
        id: "toggle-focus",
        category: "Vistas y Modos",
        label: "Alternar Focus Studio (Pomodoro)",
        description: "Espacio de trabajo sin distracciones con temporizador",
        keywords: "focus pomodoro estudio concentracion tiempo",
        icon: Timer,
        execute: onToggleFocus,
      },
      {
        id: "toggle-queue",
        category: "Vistas y Modos",
        label: "Abrir / Cerrar Cola de Reproducción",
        description: "Ver pistas en cola y añadir o reordenar",
        keywords: "cola queue lista reproduccion",
        icon: Layers,
        execute: onToggleQueue,
      },
      ...PRESETS.map((p) => ({
        id: `preset-${p.value}`,
        category: "Presets de Audio" as Category,
        label: `Preset: ${p.label}`,
        description: p.description,
        keywords: `preset ecualizador eq audio ${p.value}`,
        icon: p.icon,
        active: playerState?.audio_preset === p.value,
        execute: () => onSelectPreset(p.value),
      })),
      ...recentTracks.map((track, idx) => ({
        id: `recent-${track.id}-${idx}`,
        category: "Historial Reciente" as Category,
        label: track.title,
        description: track.artist ?? "Pista reciente",
        keywords: `reciente historia ${track.title} ${track.artist ?? ""}`,
        icon: History,
        execute: () => onPlayTrack(track),
      })),
    ];
  }, [
    isPlaying,
    isMuted,
    isLooping,
    playerState?.audio_preset,
    recentTracks,
    onTogglePlayback,
    onNext,
    onPrevious,
    onToggleMute,
    onToggleLoop,
    onSetVolume,
    onToggleImmersive,
    onToggleFocus,
    onToggleQueue,
    onSelectPreset,
    onPlayTrack,
  ]);

  const filteredCommands = useMemo<CommandItem[]>(() => {
    const trimmed = query.trim();
    if (!trimmed) {
      return systemCommands;
    }

    const tokens = normalize(trimmed).split(/\s+/);
    const matched = systemCommands.filter((cmd) => {
      const text = normalize(`${cmd.label} ${cmd.description ?? ""} ${cmd.keywords ?? ""}`);
      return tokens.every((tok) => text.includes(tok));
    });

    // Añadir acción de búsqueda directa en internet si el usuario escribe texto libre
    const directSearch: CommandItem = {
      id: "direct-search",
      category: "Búsqueda Directa",
      label: `Buscar canciones: "${trimmed}"`,
      description: "Buscar resultados en YouTube y biblioteca",
      keywords: "buscar search youtube",
      icon: Search,
      execute: () => onSearch(trimmed),
    };

    return [directSearch, ...matched];
  }, [query, systemCommands, onSearch]);

  // Reset del selector al cambiar la búsqueda
  useEffect(() => {
    setSelectedIndex(0);
  }, [query]);

  // Foco en el input al abrir
  useEffect(() => {
    if (isOpen) {
      setQuery("");
      setSelectedIndex(0);
      window.setTimeout(() => {
        inputRef.current?.focus();
      }, 50);
    }
  }, [isOpen]);

  // Manejo de teclado
  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
      return;
    }

    if (filteredCommands.length === 0) return;

    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex((prev) => (prev + 1) % filteredCommands.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex((prev) => (prev - 1 + filteredCommands.length) % filteredCommands.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const current = filteredCommands[selectedIndex];
      if (current) {
        current.execute();
        onClose();
      }
    }
  }

  // Scroll automático hacia el elemento seleccionado
  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const selectedEl = list.querySelector(`[data-index="${selectedIndex}"]`) as HTMLElement | null;
    if (selectedEl) {
      selectedEl.scrollIntoView({ block: "nearest", behavior: "smooth" });
    }
  }, [selectedIndex]);

  if (!isOpen) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center pt-[12vh] px-4 bg-black/70 backdrop-blur-xl animate-in fade-in duration-150"
      onClick={onClose}
    >
      <div
        className="w-full max-w-xl rounded-2xl bg-neutral-950/90 border border-white/15 p-4 shadow-2xl shadow-black/80 flex flex-col gap-3 text-white animate-in zoom-in-95 duration-150 overflow-hidden"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        {/* Cabecera con campo de búsqueda */}
        <div className="relative flex items-center gap-3 border-b border-white/10 pb-3">
          <Search className="size-5 text-teal-300 shrink-0" />
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Escribe un comando o busca canciones..."
            className="w-full bg-transparent text-sm font-medium text-white placeholder-white/40 focus:outline-none"
          />
          {query ? (
            <button
              type="button"
              onClick={() => setQuery("")}
              className="text-white/40 hover:text-white p-1 rounded-md"
            >
              <X className="size-4" />
            </button>
          ) : (
            <kbd className="hidden sm:inline-block px-2 py-0.5 rounded text-[10px] font-mono font-semibold bg-white/10 text-white/50 border border-white/10">
              ESC
            </kbd>
          )}
        </div>

        {/* Lista de comandos agrupada por categoría */}
        <div
          ref={listRef}
          className="max-h-[50vh] overflow-y-auto space-y-4 pr-1 scrollbar-thin scrollbar-thumb-white/20"
        >
          {filteredCommands.length === 0 ? (
            <div className="py-8 text-center text-sm text-white/40">
              No se encontraron comandos o canciones coincidentes.
            </div>
          ) : (
            CATEGORIES.map((category) => {
              const items = filteredCommands.filter((cmd) => cmd.category === category);
              if (items.length === 0) return null;

              return (
                <div key={category} className="space-y-1">
                  <div className="px-2 text-[10px] font-semibold uppercase tracking-wider text-teal-300/70">
                    {category}
                  </div>
                  {items.map((cmd) => {
                    const idx = filteredCommands.indexOf(cmd);
                    const isSelected = idx === selectedIndex;
                    const Icon = cmd.icon;

                    return (
                      <div
                        key={cmd.id}
                        data-index={idx}
                        onClick={() => {
                          cmd.execute();
                          onClose();
                        }}
                        onMouseEnter={() => setSelectedIndex(idx)}
                        className={cn(
                          "flex items-center justify-between gap-3 px-3 py-2.5 rounded-xl cursor-pointer select-none transition-all duration-100",
                          isSelected
                            ? "bg-white/15 text-white border border-white/20 shadow-md translate-x-1"
                            : "text-white/70 hover:bg-white/5 hover:text-white border border-transparent"
                        )}
                      >
                        <div className="flex items-center gap-3 min-w-0">
                          <div
                            className={cn(
                              "p-1.5 rounded-lg shrink-0",
                              isSelected ? "bg-teal-500/20 text-teal-300" : "bg-white/5 text-white/60"
                            )}
                          >
                            <Icon className="size-4" />
                          </div>
                          <div className="min-w-0">
                            <p className="text-xs font-medium truncate">{cmd.label}</p>
                            {cmd.description && (
                              <p className="text-[11px] text-white/40 truncate">{cmd.description}</p>
                            )}
                          </div>
                        </div>

                        {cmd.active && (
                          <span className="text-[10px] font-semibold px-2 py-0.5 rounded-full bg-teal-500/20 text-teal-300 border border-teal-500/30 shrink-0">
                            Activo
                          </span>
                        )}

                        {isSelected && (
                          <kbd className="hidden sm:inline-block px-1.5 py-0.5 rounded text-[10px] font-mono bg-white/20 text-white/90 shrink-0">
                            ↵
                          </kbd>
                        )}
                      </div>
                    );
                  })}
                </div>
              );
            })
          )}
        </div>

        {/* Pie de paleta con tips de navegación */}
        <div className="flex items-center justify-between border-t border-white/10 pt-2 text-[11px] text-white/40 px-1">
          <div className="flex items-center gap-3">
            <span>
              <kbd className="font-mono text-white/60">↑ ↓</kbd> navegar
            </span>
            <span>
              <kbd className="font-mono text-white/60">↵</kbd> ejecutar
            </span>
            <span>
              <kbd className="font-mono text-white/60">esc</kbd> cerrar
            </span>
          </div>
          <div className="flex items-center gap-1.5 text-teal-300/60 font-medium">
            <Sparkles className="size-3" />
            <span>Cadence Command Center</span>
          </div>
        </div>
      </div>
    </div>
  );
}
export default CommandPalette;
