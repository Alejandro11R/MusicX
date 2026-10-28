import React, { useId } from "react";
import {
  Brain,
  ChevronRight,
  Coffee,
  Headphones,
  Music2,
  Pause,
  Play,
  RotateCcw,
  SkipForward,
  Timer,
  Volume2,
  Waves,
  X,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { FocusState, PlayerState, SearchResult } from "@/lib/tauri";

/* ============================================================
 * Focus Studio
 * ------------------------------------------------------------
 * El modo Focus NO es una cortina negra: es un espacio integrado
 * en el centro de la ventana. El header y la música siguen vivos.
 *
 * Composición:
 *   FocusAura  → ambiente radial que respira (teal / ámbar)
 *   FocusRing  → anillo tipo Apple Watch + tiempo protagonista
 *   Presets    → tabs de cristal (25 / 50 / 90 min)
 *   Controles  → acción principal en una sola línea + reset/skip
 *   Switch     → auto-pausa de música con toggle propio
 *   MusicCard  → "Sonando para concentrarte" + volver a la biblioteca
 * ============================================================ */

const FOCUS_PRESETS = [
  { id: "focus-25", label: "Focus", focus: 25, breakMins: 5, Icon: Timer },
  { id: "flow-50", label: "Flow", focus: 50, breakMins: 10, Icon: Waves },
  { id: "deep-90", label: "Deep Work", focus: 90, breakMins: 20, Icon: Brain },
] as const;

type FocusTone = {
  from: string;
  to: string;
  glow: string;
  aura: string;
  auraSoft: string;
  text: string;
  dot: string;
  iconChip: string;
  button: string;
  switchOn: string;
};

const TEAL_TONE: FocusTone = {
  from: "#5eead4",
  to: "#10b981",
  glow: "rgba(45, 212, 191, 0.55)",
  aura: "rgba(45, 212, 191, 0.16)",
  auraSoft: "rgba(16, 185, 129, 0.08)",
  text: "text-teal-200",
  dot: "bg-teal-300 shadow-[0_0_8px_rgba(45,212,191,0.8)]",
  iconChip: "border-teal-300/25 bg-teal-400/10 text-teal-200",
  button:
    "bg-[linear-gradient(180deg,#5eead4_0%,#2dd4bf_48%,#10b981_100%)] text-[#022c22] shadow-[0_12px_30px_-12px_rgba(45,212,191,0.9),0_2px_0_rgba(255,255,255,0.35)_inset] hover:shadow-[0_16px_36px_-12px_rgba(45,212,191,1),0_2px_0_rgba(255,255,255,0.45)_inset]",
  switchOn:
    "border-teal-300/50 bg-teal-400/80 shadow-[0_0_14px_rgba(45,212,191,0.4)]",
};

const AMBER_TONE: FocusTone = {
  from: "#fde68a",
  to: "#f59e0b",
  glow: "rgba(251, 191, 36, 0.5)",
  aura: "rgba(251, 191, 36, 0.14)",
  auraSoft: "rgba(245, 158, 11, 0.07)",
  text: "text-amber-200",
  dot: "bg-amber-300 shadow-[0_0_8px_rgba(251,191,36,0.8)]",
  iconChip: "border-amber-300/25 bg-amber-400/10 text-amber-200",
  button:
    "bg-[linear-gradient(180deg,#fde68a_0%,#fbbf24_48%,#f59e0b_100%)] text-[#2a1a03] shadow-[0_12px_30px_-12px_rgba(251,191,36,0.85),0_2px_0_rgba(255,255,255,0.35)_inset] hover:shadow-[0_16px_36px_-12px_rgba(251,191,36,0.95),0_2px_0_rgba(255,255,255,0.45)_inset]",
  switchOn:
    "border-amber-300/50 bg-amber-400/80 shadow-[0_0_14px_rgba(251,191,36,0.4)]",
};

/* ---------- Helpers ---------- */

function formatClock(totalSeconds: number) {
  const safe = Math.max(0, Math.floor(totalSeconds));
  const mins = Math.floor(safe / 60);
  const secs = safe % 60;
  return `${String(mins).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
}

function getProgress(state: FocusState) {
  if (state.total_seconds <= 0) return 0;
  const elapsed = state.total_seconds - state.remaining_seconds;
  return Math.max(0, Math.min(100, (elapsed / state.total_seconds) * 100));
}

function getPhaseMeta(state: FocusState) {
  switch (state.phase) {
    case "short_break":
      return {
        label: "Descanso corto",
        status: state.is_running ? "Respira profundo" : "Descanso en pausa",
        isBreak: true,
      };
    case "long_break":
      return {
        label: "Descanso largo",
        status: state.is_running ? "Desconecta un momento" : "Descanso en pausa",
        isBreak: true,
      };
    case "focus":
      return {
        label: "Enfoque",
        status: state.is_running
          ? "Máxima concentración"
          : "En pausa · retoma cuando quieras",
        isBreak: false,
      };
    default:
      return {
        label: "Enfoque",
        status: "Listo cuando tú lo estés",
        isBreak: false,
      };
  }
}

function resolveActivePreset(state: FocusState) {
  if (state.phase === "short_break" || state.phase === "long_break") {
    const breakMins = state.total_seconds / 60;
    const isLongBreak = state.phase === "long_break";
    return FOCUS_PRESETS.find(
      (preset) =>
        (isLongBreak ? preset.breakMins * 2 : preset.breakMins) === breakMins,
    )?.id;
  }
  return FOCUS_PRESETS.find((preset) => preset.focus * 60 === state.total_seconds)
    ?.id;
}

/* ============================================================
 * Aura ambiental
 * ============================================================ */

function FocusAura({
  tone,
  isRunning,
}: {
  tone: FocusTone;
  isRunning: boolean;
}) {
  return (
    <div
      aria-hidden
      className="pointer-events-none absolute inset-0 overflow-hidden rounded-[inherit]"
    >
      {/* Baño radial de color: el "clima" de la sesión. */}
      <div
        className="absolute inset-0"
        style={{
          background: `radial-gradient(120% 90% at 50% 28%, ${tone.aura} 0%, ${tone.auraSoft} 38%, transparent 72%)`,
        }}
      />

      {/* Núcleo que respira. El wrapper centra; el hijo escala. */}
      <div className="absolute left-1/2 top-[40%] -translate-x-1/2 -translate-y-1/2">
        <div
          className={cn(
            "size-[340px] rounded-full blur-3xl",
            isRunning && "focus-aura",
          )}
          style={{
            background: `radial-gradient(circle, ${tone.aura} 0%, transparent 68%)`,
          }}
        />
      </div>

      {/* Halos de esquina, casi imperceptibles: dan profundidad de vidrio. */}
      <div
        className="absolute -left-28 top-1/4 size-56 rounded-full opacity-70 blur-3xl"
        style={{ background: tone.auraSoft }}
      />
      <div
        className="absolute -right-28 bottom-0 size-60 rounded-full opacity-60 blur-3xl"
        style={{ background: tone.auraSoft }}
      />

      {/* Luz cenital + viñeta inferior para que el texto respire. */}
      <div className="absolute inset-0 bg-[linear-gradient(180deg,rgba(255,255,255,0.045),transparent_30%,rgba(0,0,0,0.2))]" />
    </div>
  );
}

/* ============================================================
 * Anillo de progreso tipo Apple Watch
 * ============================================================ */

function FocusRing({
  progress,
  time,
  label,
  status,
  tone,
  isRunning,
  isBreak,
}: {
  progress: number;
  time: string;
  label: string;
  status: string;
  tone: FocusTone;
  isRunning: boolean;
  isBreak: boolean;
}) {
  const uid = useId().replace(/[^a-zA-Z0-9]/g, "");
  const gradientId = `focus-gradient-${uid}`;
  const radius = 44;
  const circumference = 2 * Math.PI * radius;
  const dashOffset = circumference * (1 - progress / 100);
  const angle = (progress / 100) * Math.PI * 2;
  const tipX = 50 + radius * Math.cos(angle);
  const tipY = 50 + radius * Math.sin(angle);

  return (
    <div
      className="relative aspect-square shrink-0"
      style={{
        width: "clamp(150px, 34vh, 196px)",
        containerType: "inline-size",
      }}
    >
      {/* Halo difuso que abraza el anillo. */}
      <div
        className="pointer-events-none absolute -inset-7 rounded-full opacity-70 blur-2xl"
        style={{
          background: `radial-gradient(circle, ${tone.aura} 0%, transparent 70%)`,
        }}
      />

      {/* Halo cónico giratorio: sólo cuando la sesión está viva. */}
      {isRunning && (
        <div className="pointer-events-none absolute -inset-3 rounded-full opacity-25 blur-xl">
          <div
            className="focus-halo size-full rounded-full"
            style={{
              background: `conic-gradient(from 0deg, transparent 0deg, ${tone.glow} 52deg, transparent 118deg, transparent 360deg)`,
            }}
          />
        </div>
      )}

      <svg
        viewBox="0 0 100 100"
        className="relative size-full -rotate-90 overflow-visible"
        aria-hidden
      >
        <defs>
          <linearGradient
            id={gradientId}
            x1="0%"
            y1="0%"
            x2="100%"
            y2="100%"
          >
            <stop offset="0%" stopColor={tone.from} />
            <stop offset="100%" stopColor={tone.to} />
          </linearGradient>
        </defs>

        {/* Pista base. */}
        <circle
          cx="50"
          cy="50"
          r={radius}
          fill="none"
          stroke="rgba(255,255,255,0.065)"
          strokeWidth="4.5"
        />

        {/* Marcas tipo reloj, muy sutiles. */}
        <circle
          cx="50"
          cy="50"
          r={radius}
          fill="none"
          stroke="rgba(255,255,255,0.16)"
          strokeWidth="0.7"
          strokeDasharray="0.7 5.4"
          strokeLinecap="round"
        />

        {/* Progreso con degradado, puntas redondeadas y glow neón. */}
        <circle
          cx="50"
          cy="50"
          r={radius}
          fill="none"
          stroke={`url(#${gradientId})`}
          strokeWidth="4.5"
          strokeLinecap="round"
          strokeDasharray={circumference}
          strokeDashoffset={dashOffset}
          className="focus-ring-progress"
          style={{ filter: `drop-shadow(0 0 5px ${tone.glow})` }}
        />

        {/* Punta luminosa que remata el trazo. */}
        {progress > 0.5 && (
          <>
            <circle
              cx={tipX}
              cy={tipY}
              r="4.5"
              fill={tone.from}
              opacity="0.18"
              style={{ filter: "blur(2px)" }}
            />
            <circle
              cx={tipX}
              cy={tipY}
              r="2.1"
              fill="#ffffff"
              style={{ filter: `drop-shadow(0 0 4px ${tone.glow})` }}
            />
          </>
        )}
      </svg>

      {/* Disco interior de cristal. */}
      <div className="absolute inset-[10%] rounded-full border border-white/10 bg-[radial-gradient(circle_at_50%_15%,rgba(255,255,255,0.09),rgba(9,13,20,0.5)_55%,rgba(6,9,14,0.72))] shadow-[inset_0_1px_0_rgba(255,255,255,0.2),inset_0_-24px_48px_-20px_rgba(0,0,0,0.8),0_18px_40px_-20px_rgba(0,0,0,0.9)]" />

      {/* Tiempo y estado. */}
      <div className="absolute inset-0 flex flex-col items-center justify-center px-3 text-center">
        <div className="mb-1.5 flex items-center gap-1.5">
          {isBreak ? (
            <Coffee className={cn("size-3", tone.text)} />
          ) : (
            <span
              className={cn(
                "size-1.5 rounded-full",
                tone.dot,
                isRunning && "animate-pulse",
              )}
            />
          )}
          <span
            className={cn(
              "text-[9px] font-semibold uppercase tracking-[0.22em]",
              tone.text,
            )}
          >
            {label}
          </span>
        </div>

        <span
          role="timer"
          aria-live="off"
          className="font-semibold leading-none tracking-[-0.055em] text-white tabular-nums drop-shadow-[0_2px_10px_rgba(0,0,0,0.45)]"
          style={{ fontSize: "clamp(1.65rem, 24cqw, 3rem)" }}
        >
          {time}
        </span>

        <span className="mt-1.5 max-w-[132px] text-[9.5px] leading-tight text-white/50">
          {status}
        </span>
      </div>
    </div>
  );
}

/* ============================================================
 * Presets de duración
 * ============================================================ */

function FocusPresets({
  activeId,
  disabled,
  tone,
  onSelect,
}: {
  activeId?: string;
  disabled: boolean;
  tone: FocusTone;
  onSelect: (focusMins: number, breakMins: number) => void;
}) {
  return (
    <div className="grid w-full grid-cols-3 gap-1 rounded-2xl border border-white/10 bg-black/20 p-1 shadow-[inset_0_1px_0_rgba(255,255,255,0.06)] backdrop-blur-xl">
      {FOCUS_PRESETS.map((preset) => {
        const active = preset.id === activeId;
        const Icon = preset.Icon;

        return (
          <button
            key={preset.id}
            type="button"
            disabled={disabled}
            onClick={() => onSelect(preset.focus, preset.breakMins)}
            title={`${preset.focus} min de enfoque · ${preset.breakMins} min de descanso`}
            className={cn(
              "relative flex h-[42px] flex-col items-center justify-center gap-0.5 rounded-xl border border-transparent transition-all duration-200",
              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-teal-300/30",
              active
                ? "border-white/15 bg-white/[0.08] text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.16),0_6px_16px_-10px_rgba(0,0,0,0.9)]"
                : "text-white/60 hover:bg-white/[0.04] hover:text-white/85",
              disabled && "cursor-not-allowed opacity-45 hover:bg-transparent",
            )}
          >
            <span className="flex items-center gap-1.5">
              <Icon
                className={cn("size-3 transition-colors", active && tone.text)}
              />
              <span className="whitespace-nowrap text-[10px] font-semibold tracking-wide">
                {preset.label}
              </span>
            </span>
            <span className="text-[9px] tabular-nums text-white/40">
              {preset.focus} min
            </span>

            {active && (
              <span
                className={cn(
                  "absolute bottom-1 h-[2px] w-4 rounded-full bg-current opacity-70",
                  tone.text,
                )}
              />
            )}
          </button>
        );
      })}
    </div>
  );
}

/* ============================================================
 * Switch de auto-pausa
 * ============================================================ */

function AutoPauseSwitch({
  checked,
  tone,
  onChange,
}: {
  checked: boolean;
  tone: FocusTone;
  onChange: (enabled: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-xl border border-white/[0.07] bg-white/[0.025] px-2.5 py-1.5">
      <div className="flex min-w-0 items-center gap-2">
        <Volume2 className="size-3.5 shrink-0 text-white/40" />
        <p className="truncate text-[10.5px] font-medium text-white/70">
          Auto-pausar música en descansos
        </p>
      </div>

      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label="Auto-pausar música durante el descanso"
        onClick={() => onChange(!checked)}
        className={cn(
          "relative h-[22px] w-[38px] shrink-0 rounded-full border transition-all duration-300",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-teal-300/40",
          checked ? tone.switchOn : "border-white/15 bg-white/10",
        )}
      >
        <span
          className={cn(
            "absolute top-1/2 size-4 -translate-y-1/2 rounded-full bg-white shadow-[0_1px_4px_rgba(0,0,0,0.45)] transition-all duration-300",
            checked ? "left-[19px]" : "left-[3px]",
          )}
        />
      </button>
    </div>
  );
}

/* ============================================================
 * Tarjeta "Sonando para concentrarte"
 * ============================================================ */

function FocusMusicCard({
  track,
  isPlaying,
  onOpenLibrary,
}: {
  track: SearchResult | null;
  isPlaying: boolean;
  onOpenLibrary: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onOpenLibrary}
      className="group flex w-full items-center gap-2.5 rounded-xl border border-white/[0.08] bg-white/[0.035] px-2.5 py-2 text-left transition-all duration-200 hover:border-white/15 hover:bg-white/[0.06] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-teal-300/30"
      title="Volver a la biblioteca sin detener el temporizador"
    >
      <span className="relative flex size-8 shrink-0 items-center justify-center overflow-hidden rounded-lg border border-white/10 bg-teal-400/10">
        {track?.thumbnail_url ? (
          <img
            src={track.thumbnail_url}
            alt=""
            className="size-full object-cover"
            onError={(event) => {
              event.currentTarget.style.display = "none";
            }}
          />
        ) : (
          <Headphones className="size-4 text-teal-200/80" />
        )}
      </span>

      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1.5 text-[9px] font-medium uppercase tracking-[0.18em] text-white/40">
          <span
            className={cn(
              "size-1 rounded-full",
              isPlaying
                ? "bg-teal-300 shadow-[0_0_6px_rgba(45,212,191,0.8)] animate-pulse"
                : "bg-white/20",
            )}
          />
          {track ? "Sonando para concentrarte" : "Música para concentrarte"}
        </span>
        <span className="mt-0.5 block truncate text-[11px] font-medium text-white/85">
          {track?.title ?? "Elegir una pista"}
        </span>
      </span>

      <span className="flex shrink-0 items-center gap-0.5 text-[9px] font-medium text-white/45 transition-colors group-hover:text-white/65">
        Biblioteca
        <ChevronRight className="size-3" />
      </span>
    </button>
  );
}

/* ============================================================
 * FocusStudio — vista central integrada
 * ============================================================ */

export interface FocusStudioProps {
  focusState: FocusState | null;
  currentTrack: SearchResult | null;
  isPlaying: boolean;
  onStart: () => void;
  onPause: () => void;
  onReset: () => void;
  onSkip: () => void;
  onSelectDuration: (focusMins: number, breakMins: number) => void;
  onToggleAutoPause: (enabled: boolean) => void;
  onOpenLibrary: () => void;
  onClose: () => void;
}

export const FocusStudio = React.memo(function FocusStudio({
  focusState,
  currentTrack,
  isPlaying,
  onStart,
  onPause,
  onReset,
  onSkip,
  onSelectDuration,
  onToggleAutoPause,
  onOpenLibrary,
  onClose,
}: FocusStudioProps) {
  if (!focusState) return null;

  const meta = getPhaseMeta(focusState);
  const tone = meta.isBreak ? AMBER_TONE : TEAL_TONE;
  const progress = getProgress(focusState);
  const time = formatClock(focusState.remaining_seconds);
  const activePreset = resolveActivePreset(focusState);

  const isRunning = focusState.is_running;
  const inCycle = focusState.sessions_completed % 4;
  const filledSessions =
    inCycle === 0 && focusState.sessions_completed > 0 ? 4 : inCycle;

  const primaryLabel = isRunning
    ? meta.isBreak
      ? "Pausar descanso"
      : "Pausar"
    : meta.isBreak
      ? "Reanudar descanso"
      : focusState.phase === "focus"
        ? "Reanudar"
        : "Iniciar enfoque";

  return (
    <section
      aria-label="Focus Studio"
      className="relative flex min-h-0 flex-1 animate-in fade-in zoom-in-[0.99] flex-col overflow-hidden rounded-[22px] border border-white/10 bg-white/[0.02] shadow-[inset_0_1px_0_rgba(255,255,255,0.08)] duration-300"
    >
      <FocusAura tone={tone} isRunning={isRunning} />

      <div className="no-scrollbar relative z-10 flex min-h-0 flex-1 flex-col overflow-y-auto px-4 pb-3.5 pt-3">
        {/* Barra superior del estudio: fase, rondas y salida. */}
        <header className="flex shrink-0 items-center justify-between gap-2">
          <div className="flex min-w-0 items-center gap-2">
            <span
              className={cn(
                "flex size-7 shrink-0 items-center justify-center rounded-full border backdrop-blur-md",
                tone.iconChip,
              )}
            >
              {meta.isBreak ? (
                <Coffee className="size-3.5" />
              ) : (
                <Timer className="size-3.5" />
              )}
            </span>

            <div className="min-w-0">
              <p className="text-[9px] font-medium uppercase tracking-[0.2em] text-white/40">
                Focus Studio
              </p>
              <p className="truncate text-[11px] font-medium text-white/80">
                {meta.label}
                <span className="text-white/35">
                  {" "}
                  ·{" "}
                  {focusState.phase === "idle"
                    ? "listo para empezar"
                    : `${Math.round(progress)}% completado`}
                </span>
              </p>
            </div>
          </div>

          <div className="flex shrink-0 items-center gap-1.5">
            <div
              className="flex items-center gap-[3px]"
              aria-hidden
              title={`${focusState.sessions_completed} sesiones completadas`}
            >
              {Array.from({ length: 4 }).map((_, index) => (
                <span
                  key={index}
                  className={cn(
                    "size-1.5 rounded-full transition-all duration-300",
                    index < filledSessions
                      ? tone.dot
                      : index === filledSessions && isRunning && !meta.isBreak
                        ? "bg-white/30 animate-pulse"
                        : "bg-white/15",
                  )}
                />
              ))}
            </div>

            <span className="text-[9px] tabular-nums text-white/40">
              {focusState.sessions_completed} ses.
            </span>

            <button
              type="button"
              onClick={onClose}
              className="ml-0.5 flex size-6 items-center justify-center rounded-full border border-white/[0.08] bg-white/[0.04] text-white/45 transition-all duration-200 hover:border-white/20 hover:bg-white/[0.09] hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-teal-300/30"
              aria-label="Salir de Focus Studio (Esc). El temporizador sigue activo"
              title="Volver a la música (Esc)"
            >
              <X className="size-3" />
            </button>
          </div>
        </header>

        {/* Protagonista: anillo + presets. */}
        <div className="flex flex-1 flex-col items-center justify-center gap-4 py-3">
          <FocusRing
            progress={progress}
            time={time}
            label={meta.label}
            status={meta.status}
            tone={tone}
            isRunning={isRunning}
            isBreak={meta.isBreak}
          />

          <FocusPresets
            activeId={activePreset}
            disabled={isRunning || meta.isBreak}
            tone={tone}
            onSelect={onSelectDuration}
          />
        </div>

        {/* Controles: una sola línea, sin texto partido. */}
        <div className="shrink-0 space-y-2.5">
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={onReset}
              className="flex size-10 shrink-0 items-center justify-center rounded-full border border-white/10 bg-white/[0.04] text-white/50 transition-all duration-200 hover:border-white/20 hover:bg-white/[0.08] hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-teal-300/30"
              aria-label="Reiniciar temporizador"
              title="Reiniciar temporizador"
            >
              <RotateCcw className="size-3.5" />
            </button>

            <button
              type="button"
              onClick={isRunning ? onPause : onStart}
              className={cn(
                "group relative flex h-11 min-w-0 flex-1 items-center justify-center gap-2 overflow-hidden rounded-full px-5 text-[13px] font-semibold tracking-[-0.01em] transition-all duration-200",
                "hover:-translate-y-px active:translate-y-0 active:scale-[0.985]",
                "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white/50 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent",
                tone.button,
              )}
            >
              {/* Brillo que recorre el botón. */}
              <span
                aria-hidden
                className="focus-shimmer pointer-events-none absolute inset-0 bg-[linear-gradient(110deg,transparent_20%,rgba(255,255,255,0.35)_50%,transparent_80%)] opacity-40 mix-blend-overlay"
              />
              <span className="relative flex items-center gap-2 whitespace-nowrap">
                {isRunning ? (
                  <Pause className="size-4 fill-current" />
                ) : (
                  <Play className="size-4 fill-current" />
                )}
                {primaryLabel}
              </span>
            </button>

            <button
              type="button"
              onClick={onSkip}
              disabled={focusState.phase === "idle"}
              className="flex size-10 shrink-0 items-center justify-center rounded-full border border-white/10 bg-white/[0.04] text-white/50 transition-all duration-200 hover:border-white/20 hover:bg-white/[0.08] hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-teal-300/30 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:border-white/10 disabled:hover:bg-white/[0.04] disabled:hover:text-white/50"
              aria-label="Saltar a la siguiente fase"
              title="Saltar fase"
            >
              <SkipForward className="size-3.5" />
            </button>
          </div>

          <AutoPauseSwitch
            checked={focusState.auto_pause_on_break}
            tone={tone}
            onChange={onToggleAutoPause}
          />

          <FocusMusicCard
            track={currentTrack}
            isPlaying={isPlaying}
            onOpenLibrary={onOpenLibrary}
          />
        </div>
      </div>
    </section>
  );
});

/* ============================================================
 * FocusWidget — pill del header con micro-barra de progreso
 * ============================================================ */

export interface FocusWidgetProps {
  focusState: FocusState | null;
  isActive: boolean;
  onOpen: () => void;
  onStart: () => void;
  onPause: () => void;
}

export const FocusWidget = React.memo(function FocusWidget({
  focusState,
  isActive,
  onOpen,
  onStart,
  onPause,
}: FocusWidgetProps) {
  if (!focusState) return null;

  const isRunning = focusState.is_running;
  const isBreak =
    focusState.phase === "short_break" || focusState.phase === "long_break";
  const tone = isBreak ? AMBER_TONE : TEAL_TONE;
  const progress = getProgress(focusState);
  const time = formatClock(focusState.remaining_seconds);
  const label = isBreak ? "Descanso" : "Focus";

  return (
    <div
      className={cn(
        "group relative flex items-center overflow-hidden rounded-full border text-[11px] backdrop-blur-xl transition-all duration-300",
        isActive
          ? "border-white/20 bg-white/[0.08] shadow-[inset_0_1px_0_rgba(255,255,255,0.18)]"
          : isRunning
            ? isBreak
              ? "border-amber-300/30 bg-amber-400/10 shadow-[0_0_16px_-4px_rgba(251,191,36,0.45)]"
              : "border-teal-300/30 bg-teal-400/10 shadow-[0_0_16px_-4px_rgba(45,212,191,0.45)]"
            : "border-white/10 bg-white/[0.04] hover:border-white/20 hover:bg-white/[0.07]",
      )}
    >
      <button
        type="button"
        onClick={onOpen}
        aria-pressed={isActive}
        className="flex items-center gap-1.5 py-1.5 pl-2.5 pr-1.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-teal-300/40"
        title="Abrir Focus Studio (Shift+F)"
      >
        <span className="relative flex size-3.5 items-center justify-center">
          {isBreak ? (
            <Coffee className="size-3 text-amber-300" />
          ) : (
            <Timer
              className={cn(
                "size-3",
                isRunning ? "text-teal-300" : "text-white/45",
              )}
            />
          )}
          {isRunning && (
            <span
              className={cn(
                "absolute -right-0.5 -top-0.5 size-1 rounded-full",
                tone.dot,
                "animate-pulse",
              )}
            />
          )}
        </span>

        <span
          className={cn(
            "font-medium",
            isBreak
              ? "text-amber-100/90"
              : isRunning
                ? "text-teal-50/90"
                : "text-white/65",
          )}
        >
          {label}
        </span>

        <span className="font-mono text-[11px] font-semibold tabular-nums text-white/95">
          {time}
        </span>
      </button>

      <span className="h-3 w-px shrink-0 bg-white/15" aria-hidden />

      <button
        type="button"
        onClick={isRunning ? onPause : onStart}
        className="flex size-6 shrink-0 items-center justify-center text-white/70 transition-colors hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-teal-300/40"
        aria-label={
          isRunning
            ? "Pausar sesión de enfoque (F)"
            : "Iniciar sesión de enfoque (F)"
        }
        title={isRunning ? "Pausar sesión (F)" : "Iniciar sesión (F)"}
      >
        {isRunning ? (
          <Pause className="size-3 fill-current" />
        ) : (
          <Play className="size-3 fill-current" />
        )}
      </button>

      {/* Micro-barra de progreso, pegada al borde inferior del pill. */}
      <span
        aria-hidden
        className="pointer-events-none absolute inset-x-2.5 bottom-[2px] h-[2px] overflow-hidden rounded-full bg-white/10"
      >
        <span
          className="block h-full rounded-full transition-[width] duration-500 ease-out"
          style={{
            width: `${progress}%`,
            background: `linear-gradient(90deg, ${tone.from}, ${tone.to})`,
            boxShadow: `0 0 6px ${tone.glow}`,
          }}
        />
      </span>
    </div>
  );
});

/* ============================================================
 * FocusNowPlayingDock — música visible y controlable en Focus
 * ============================================================ */

export interface FocusNowPlayingDockProps {
  playerState: PlayerState | null;
  isTrackLoading: boolean;
  isPlaying: boolean;
  onTogglePlayback: () => void;
  onNext: () => void;
  onOpenLibrary: () => void;
}

export const FocusNowPlayingDock = React.memo(function FocusNowPlayingDock({
  playerState,
  isTrackLoading,
  isPlaying,
  onTogglePlayback,
  onNext,
  onOpenLibrary,
}: FocusNowPlayingDockProps) {
  const current = playerState?.current;

  if (!current && !isTrackLoading) return null;

  const duration = Math.max(playerState?.duration_seconds ?? 0, 1);
  const position = Math.max(0, playerState?.position_seconds ?? 0);
  const progress = Math.min(100, (position / duration) * 100);

  return (
    <div className="liquid-card relative mt-auto flex shrink-0 items-center gap-2.5 overflow-hidden rounded-2xl border border-white/10 p-2 shadow-[0_12px_30px_-18px_rgba(0,0,0,0.9)]">
      <div className="relative size-9 shrink-0 overflow-hidden rounded-[10px] border border-white/10 bg-white/5">
        {current?.thumbnail_url ? (
          <img
            src={current.thumbnail_url}
            alt=""
            className="size-full object-cover"
            onError={(event) => {
              event.currentTarget.style.display = "none";
            }}
          />
        ) : (
          <div className="flex size-full items-center justify-center bg-teal-500/10">
            <Headphones className="size-4 text-teal-200/80" />
          </div>
        )}
      </div>

      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          <span
            className={cn(
              "size-1.5 shrink-0 rounded-full",
              isPlaying
                ? "bg-teal-300 shadow-[0_0_6px_rgba(45,212,191,0.8)] animate-pulse"
                : "bg-white/25",
            )}
          />
          <p className="truncate text-[11px] font-semibold text-white/90">
            {current?.title ?? "Cargando pista…"}
          </p>
        </div>

        <p className="truncate pl-3 text-[9.5px] text-white/45">
          {current?.artist || "Cadence"}
        </p>

        <div className="ml-3 mt-1 h-[2px] overflow-hidden rounded-full bg-white/10">
          <div
            className="h-full rounded-full bg-[linear-gradient(90deg,#2dd4bf,#10b981)] shadow-[0_0_5px_rgba(45,212,191,0.6)] transition-[width] duration-500"
            style={{ width: `${progress}%` }}
          />
        </div>
      </div>

      <button
        type="button"
        onClick={onTogglePlayback}
        disabled={!current}
        className="flex size-8 shrink-0 items-center justify-center rounded-full border border-white/15 bg-white/10 text-white transition-all duration-200 hover:bg-white/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-teal-300/40 disabled:opacity-40"
        aria-label={isPlaying ? "Pausar música" : "Reproducir música"}
      >
        {isPlaying ? (
          <Pause className="size-3.5 fill-current" />
        ) : (
          <Play className="size-3.5 fill-current" />
        )}
      </button>

      <button
        type="button"
        onClick={onNext}
        disabled={!current}
        className="flex size-7 shrink-0 items-center justify-center rounded-full text-white/55 transition-colors hover:bg-white/10 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-teal-300/40 disabled:opacity-40"
        aria-label="Siguiente pista"
      >
        <SkipForward className="size-3.5" />
      </button>

      <button
        type="button"
        onClick={onOpenLibrary}
        className="flex size-7 shrink-0 items-center justify-center rounded-full text-white/55 transition-colors hover:bg-white/10 hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-teal-300/40"
        aria-label="Volver a la biblioteca sin detener el temporizador"
        title="Volver a la biblioteca"
      >
        <Music2 className="size-3.5" />
      </button>
    </div>
  );
});
