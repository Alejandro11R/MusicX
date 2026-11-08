"use client";

import { useState, useRef } from "react";
import { X, Copy, Check, Share2, Sparkles, Music2, Download } from "lucide-react";
import type { SearchResult, LyricLine } from "@/lib/tauri";
import { cn } from "@/lib/utils";

interface LyricCardModalProps {
  isOpen: boolean;
  onClose: () => void;
  track: SearchResult | null;
  lines: LyricLine[];
  activeLineIndex: number;
  palette: { primary: string; secondary: string; ambient: string };
}

export function LyricCardModal({
  isOpen,
  onClose,
  track,
  lines,
  activeLineIndex,
  palette,
}: LyricCardModalProps) {
  const [selectedIndices, setSelectedIndices] = useState<number[]>(() => {
    if (lines.length === 0) return [];
    const start = Math.max(0, activeLineIndex >= 0 ? activeLineIndex : 0);
    const end = Math.min(lines.length, start + 3);
    return Array.from({ length: end - start }, (_, i) => start + i);
  });
  const [copiedText, setCopiedText] = useState(false);
  const [copiedImage, setCopiedImage] = useState(false);
  const cardRef = useRef<HTMLDivElement>(null);

  if (!isOpen || !track) return null;

  const toggleLine = (index: number) => {
    setSelectedIndices((prev) => {
      if (prev.includes(index)) {
        if (prev.length === 1) return prev; // Mantener al menos una
        return prev.filter((i) => i !== index);
      } else {
        if (prev.length >= 6) return prev; // Máximo 6 líneas
        return [...prev, index].sort((a, b) => a - b);
      }
    });
  };

  const handleCopyText = async () => {
    const selectedText = selectedIndices
      .map((idx) => lines[idx]?.text)
      .filter(Boolean)
      .join("\n");

    const fullShare = `"${selectedText}"\n\n🎵 ${track.title} — ${track.artist ?? "Cadence"}`;
    try {
      await navigator.clipboard.writeText(fullShare);
      setCopiedText(true);
      setTimeout(() => setCopiedText(false), 2000);
    } catch (e) {
      console.error("Error al copiar texto:", e);
    }
  };

  const handleCopyImage = async () => {
    try {
      const canvas = document.createElement("canvas");
      canvas.width = 1080;
      canvas.height = 1350;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;

      // 1. Fondo degradado ambiental
      const grad = ctx.createLinearGradient(0, 0, 1080, 1350);
      grad.addColorStop(0, "#09090b");
      grad.addColorStop(0.5, palette.primary || "#18181b");
      grad.addColorStop(1, "#000000");
      ctx.fillStyle = grad;
      ctx.fillRect(0, 0, 1080, 1350);

      // 2. Viñeta oscura central
      ctx.fillStyle = "rgba(0, 0, 0, 0.45)";
      ctx.roundRect(80, 80, 920, 1190, 40);
      ctx.fill();
      ctx.strokeStyle = "rgba(255, 255, 255, 0.15)";
      ctx.lineWidth = 2;
      ctx.stroke();

      // 3. Encabezado de la canción
      ctx.fillStyle = "#ffffff";
      ctx.font = "bold 38px system-ui, -apple-system, sans-serif";
      ctx.fillText(track.title.slice(0, 40), 140, 200);

      ctx.fillStyle = "rgba(255, 255, 255, 0.65)";
      ctx.font = "28px system-ui, -apple-system, sans-serif";
      ctx.fillText(track.artist ?? "Cadence Player", 140, 250);

      // Línea divisoria
      ctx.strokeStyle = "rgba(255, 255, 255, 0.1)";
      ctx.beginPath();
      ctx.moveTo(140, 300);
      ctx.lineTo(940, 300);
      ctx.stroke();

      // 4. Versos de la letra
      ctx.fillStyle = "#ffffff";
      ctx.font = "bold 44px system-ui, -apple-system, sans-serif";
      let startY = 430;
      selectedIndices.forEach((idx) => {
        const text = lines[idx]?.text;
        if (text) {
          ctx.fillText(`“ ${text} ”`, 140, startY);
          startY += 100;
        }
      });

      // 5. Pie con marca de agua Cadence
      ctx.fillStyle = "rgba(45, 212, 191, 0.9)";
      ctx.font = "600 24px system-ui, -apple-system, sans-serif";
      ctx.fillText("✦ Cadence Music Player", 140, 1200);

      canvas.toBlob(async (blob) => {
        if (!blob) return;
        try {
          await navigator.clipboard.write([
            new ClipboardItem({ "image/png": blob }),
          ]);
          setCopiedImage(true);
          setTimeout(() => setCopiedImage(false), 2500);
        } catch {
          // Fallback descarga
          const url = URL.createObjectURL(blob);
          const a = document.createElement("a");
          a.href = url;
          a.download = `cadence-${track.title.slice(0, 15)}.png`;
          a.click();
          URL.revokeObjectURL(url);
          setCopiedImage(true);
          setTimeout(() => setCopiedImage(false), 2500);
        }
      });
    } catch (err) {
      console.error("Error al exportar imagen:", err);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/80 backdrop-blur-xl animate-in fade-in duration-150"
      onClick={onClose}
    >
      <div
        className="relative w-full max-w-lg rounded-3xl bg-neutral-950/95 border border-white/20 p-6 shadow-2xl flex flex-col gap-5 text-white animate-in zoom-in-95 duration-150 overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Cabecera */}
        <div className="flex items-center justify-between border-b border-white/10 pb-3">
          <div className="flex items-center gap-2">
            <Share2 className="size-4 text-teal-300" />
            <h3 className="text-sm font-semibold">Generar Lyric Card</h3>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="p-1 rounded-full text-white/50 hover:text-white hover:bg-white/10"
          >
            <X className="size-4" />
          </button>
        </div>

        {/* Tarjeta Visual de Previsualización */}
        <div
          ref={cardRef}
          className="relative rounded-2xl p-6 border border-white/20 shadow-2xl flex flex-col justify-between min-h-[220px] overflow-hidden"
          style={{
            background: `linear-gradient(135deg, rgba(20,20,25,0.9) 0%, ${palette.primary || "rgba(45,212,191,0.2)"} 60%, rgba(10,10,12,0.95) 100%)`,
          }}
        >
          {/* Brillo especular */}
          <div className="absolute inset-0 bg-gradient-to-b from-white/10 via-transparent to-black/40 pointer-events-none" />

          {/* Info Track */}
          <div className="relative z-10 flex items-center gap-3">
            {track.thumbnail_url ? (
              <img
                src={track.thumbnail_url}
                alt=""
                className="size-11 rounded-lg object-cover shadow-md border border-white/20"
              />
            ) : (
              <div className="size-11 rounded-lg bg-white/10 flex items-center justify-center">
                <Music2 className="size-5 text-teal-300" />
              </div>
            )}
            <div className="min-w-0">
              <p className="text-sm font-bold truncate text-white">{track.title}</p>
              <p className="text-xs text-white/70 truncate">{track.artist ?? "Cadence"}</p>
            </div>
          </div>

          {/* Versos seleccionados */}
          <div className="relative z-10 py-5 space-y-2">
            {selectedIndices.map((idx) => (
              <p
                key={idx}
                className="text-base sm:text-lg font-bold tracking-tight text-white/95 leading-snug drop-shadow"
              >
                “{lines[idx]?.text}”
              </p>
            ))}
          </div>

          {/* Watermark Cadence */}
          <div className="relative z-10 flex items-center justify-between text-[11px] text-white/50 pt-2 border-t border-white/10">
            <span className="flex items-center gap-1.5 text-teal-300 font-medium">
              <Sparkles className="size-3" />
              Cadence
            </span>
            <span>Toca versos abajo para personalizar</span>
          </div>
        </div>

        {/* Selector de versos */}
        <div className="space-y-1.5">
          <label className="text-[11px] font-medium text-white/60 uppercase tracking-wider">
            Versos incluidos ({selectedIndices.length}/6):
          </label>
          <div className="max-h-36 overflow-y-auto space-y-1 pr-1 scrollbar-thin scrollbar-thumb-white/20">
            {lines.map((line, idx) => {
              const isSelected = selectedIndices.includes(idx);
              return (
                <button
                  key={idx}
                  type="button"
                  onClick={() => toggleLine(idx)}
                  className={cn(
                    "w-full text-left text-xs px-3 py-1.5 rounded-lg border transition-all cursor-pointer truncate",
                    isSelected
                      ? "bg-teal-500/20 border-teal-400/40 text-teal-200 font-medium"
                      : "bg-white/5 border-white/5 text-white/60 hover:bg-white/10 hover:text-white"
                  )}
                >
                  {line.text}
                </button>
              );
            })}
          </div>
        </div>

        {/* Botones de acción */}
        <div className="flex items-center gap-2 pt-2 border-t border-white/10">
          <button
            type="button"
            onClick={handleCopyImage}
            className="flex-1 flex items-center justify-center gap-2 py-2.5 px-4 rounded-xl bg-teal-400 hover:bg-teal-300 text-neutral-950 font-semibold text-xs active:scale-95 transition-all shadow-lg cursor-pointer"
          >
            {copiedImage ? (
              <>
                <Check className="size-4" />
                <span>¡Copiada Imagen!</span>
              </>
            ) : (
              <>
                <Download className="size-4" />
                <span>Copiar / Guardar Imagen</span>
              </>
            )}
          </button>

          <button
            type="button"
            onClick={handleCopyText}
            className="flex items-center justify-center gap-2 py-2.5 px-4 rounded-xl bg-white/10 hover:bg-white/20 border border-white/15 text-white font-medium text-xs active:scale-95 transition-all cursor-pointer"
          >
            {copiedText ? (
              <>
                <Check className="size-4 text-teal-300" />
                <span>Copiado</span>
              </>
            ) : (
              <>
                <Copy className="size-4" />
                <span>Texto</span>
              </>
            )}
          </button>
        </div>
      </div>
    </div>
  );
}

export default LyricCardModal;
