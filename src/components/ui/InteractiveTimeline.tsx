"use client";

import { useState, useRef, memo } from "react";
import { Slider } from "@/components/ui/slider";
import { formatDuration } from "@/lib/playback";
import { cn } from "@/lib/utils";

export interface InteractiveTimelineProps {
  position: number;
  duration: number;
  onSeekPreview: (position: number) => void;
  onSeekCommit: (position: number) => void;
  className?: string;
  showLabels?: boolean;
}

export const InteractiveTimeline = memo(function InteractiveTimeline({
  position,
  duration,
  onSeekPreview,
  onSeekCommit,
  className,
  showLabels = true,
}: InteractiveTimelineProps) {
  const [hoverTime, setHoverTime] = useState<number | null>(null);
  const [hoverX, setHoverX] = useState<number>(0);
  const containerRef = useRef<HTMLDivElement>(null);

  const safeDur = Math.max(duration, 1);

  const handleMouseMove = (e: React.MouseEvent<HTMLDivElement>) => {
    if (!containerRef.current || duration <= 0) return;
    const rect = containerRef.current.getBoundingClientRect();
    const x = Math.max(0, Math.min(e.clientX - rect.left, rect.width));
    const ratio = x / rect.width;
    const targetSecs = ratio * duration;
    setHoverX(x);
    setHoverTime(targetSecs);
  };

  const handleMouseLeave = () => {
    setHoverTime(null);
  };

  return (
    <div className={cn("flex items-center gap-2", className)}>
      {showLabels && (
        <span className="w-8 shrink-0 text-right text-[10px] font-mono tabular-nums text-white/50">
          {formatDuration(position)}
        </span>
      )}

      <div
        ref={containerRef}
        onMouseMove={handleMouseMove}
        onMouseLeave={handleMouseLeave}
        className="relative flex-1 flex items-center py-2 cursor-pointer group"
      >
        {/* Tooltip flotante con timestamp predictivo */}
        {hoverTime !== null && (
          <div
            className="pointer-events-none absolute -top-5 -translate-x-1/2 z-30 px-2 py-0.5 rounded-full bg-neutral-900/95 border border-white/20 text-[10px] font-mono font-medium text-teal-300 shadow-xl backdrop-blur-md animate-in fade-in zoom-in-95 duration-100 flex items-center gap-1"
            style={{ left: `${hoverX}px` }}
          >
            <span>{formatDuration(hoverTime)}</span>
          </div>
        )}

        <Slider
          className="w-full cursor-pointer"
          value={[position]}
          max={safeDur}
          step={1}
          variant="glass"
          onValueChange={([val]) => onSeekPreview(val)}
          onValueCommit={([val]) => onSeekCommit(val)}
          aria-label="Línea de tiempo de reproducción"
          aria-valuetext={`${formatDuration(position)} de ${formatDuration(duration)}`}
        />
      </div>

      {showLabels && (
        <span className="w-8 shrink-0 text-[10px] font-mono tabular-nums text-white/50">
          {formatDuration(duration)}
        </span>
      )}
    </div>
  );
});

export default InteractiveTimeline;
