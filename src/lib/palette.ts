import { useState, useEffect } from "react";

export interface AlbumPalette {
  primary: string;
  secondary: string;
  ambient: string;
}

const DEFAULT_PALETTE: AlbumPalette = {
  primary: "rgba(99, 102, 241, 0.45)", // Indigo
  secondary: "rgba(168, 85, 247, 0.35)", // Purple
  ambient: "rgba(236, 72, 153, 0.25)", // Pink
};

const paletteCache = new Map<string, AlbumPalette>();

/**
 * Extrae una paleta de 3 colores ambientales desde una imagen mediante un canvas miniatura (16x16).
 * Rápido, no bloqueante (< 2ms) y sin dependencias pesadas externas.
 */
export async function extractPaletteFromImage(src: string): Promise<AlbumPalette> {
  if (paletteCache.has(src)) {
    return paletteCache.get(src)!;
  }

  return new Promise((resolve) => {
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.onload = () => {
      try {
        const canvas = document.createElement("canvas");
        canvas.width = 16;
        canvas.height = 16;
        const ctx = canvas.getContext("2d", { willReadFrequently: true });
        if (!ctx) {
          resolve(DEFAULT_PALETTE);
          return;
        }

        ctx.drawImage(img, 0, 0, 16, 16);
        const data = ctx.getImageData(0, 0, 16, 16).data;

        let r = 0, g = 0, b = 0, count = 0;
        let rMax = 0, gMax = 0, bMax = 0;
        let maxSaturation = -1;

        for (let i = 0; i < data.length; i += 4) {
          const pr = data[i];
          const pg = data[i + 1];
          const pb = data[i + 2];
          const pa = data[i + 3];

          if (pa < 128) continue;

          // Ignorar extremos muy oscuros o casi blancos
          const lum = 0.299 * pr + 0.587 * pg + 0.114 * pb;
          if (lum < 25 || lum > 240) continue;

          r += pr;
          g += pg;
          b += pb;
          count++;

          const max = Math.max(pr, pg, pb);
          const min = Math.min(pr, pg, pb);
          const sat = max === 0 ? 0 : (max - min) / max;

          if (sat > maxSaturation) {
            maxSaturation = sat;
            rMax = pr;
            gMax = pg;
            bMax = pb;
          }
        }

        if (count === 0) {
          resolve(DEFAULT_PALETTE);
          return;
        }

        const avgR = Math.round(r / count);
        const avgG = Math.round(g / count);
        const avgB = Math.round(b / count);

        const palette: AlbumPalette = {
          primary: `rgba(${rMax}, ${gMax}, ${bMax}, 0.5)`,
          secondary: `rgba(${avgR}, ${avgG}, ${avgB}, 0.38)`,
          ambient: `rgba(${Math.min(255, avgR + 40)}, ${Math.min(255, avgG + 20)}, ${Math.max(0, avgB - 30)}, 0.28)`,
        };

        paletteCache.set(src, palette);
        resolve(palette);
      } catch {
        resolve(DEFAULT_PALETTE);
      }
    };

    img.onerror = () => resolve(DEFAULT_PALETTE);
    img.src = src;
  });
}

export function useAlbumPalette(imageUrl?: string | null): AlbumPalette {
  const [palette, setPalette] = useState<AlbumPalette>(DEFAULT_PALETTE);

  useEffect(() => {
    if (!imageUrl) {
      setPalette(DEFAULT_PALETTE);
      return;
    }

    let active = true;
    extractPaletteFromImage(imageUrl).then((result) => {
      if (active) setPalette(result);
    });

    return () => {
      active = false;
    };
  }, [imageUrl]);

  return palette;
}
