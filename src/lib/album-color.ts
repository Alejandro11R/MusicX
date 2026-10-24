/**
 * Utilidades para obtener un color de acento a partir de la portada del álbum.
 *
 * Intenta extraer el color promedio de la imagen vía canvas. Si la imagen está
 * bloqueada por CORS (tainted canvas) o falla la carga, cae en un color
 * determinístico derivado del hash de la URL, así siempre hay un tono
 * coherente por track sin depender de la red.
 */

const PALETTE_HUES = [180, 160, 280, 200, 320]; // teal, emerald, purple, cyan, magenta

export function stringToColor(imageUrl: string): string {
  let hash = 0;
  for (let i = 0; i < imageUrl.length; i++) {
    hash = imageUrl.charCodeAt(i) + ((hash << 5) - hash);
    hash = hash & hash; // convierte a 32-bit int
  }
  const h = Math.abs(hash) % PALETTE_HUES.length;
  return `hsl(${PALETTE_HUES[h]}, 70%, 55%)`;
}

export function extractDominantColor(imageUrl: string): Promise<string> {
  return new Promise((resolve) => {
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.onload = () => {
      try {
        const canvas = document.createElement("canvas");
        canvas.width = 32;
        canvas.height = 32;
        const ctx = canvas.getContext("2d");
        if (!ctx) {
          resolve(stringToColor(imageUrl));
          return;
        }
        ctx.drawImage(img, 0, 0, 32, 32);
        const data = ctx.getImageData(0, 0, 32, 32).data;

        let r = 0;
        let g = 0;
        let b = 0;
        let count = 0;
        for (let i = 0; i < data.length; i += 4) {
          if (data[i + 3] > 128) {
            // ignora píxeles casi transparentes
            r += data[i];
            g += data[i + 1];
            b += data[i + 2];
            count += 1;
          }
        }

        if (count > 0) {
          r = Math.round(r / count);
          g = Math.round(g / count);
          b = Math.round(b / count);
          resolve(`rgb(${r},${g},${b})`);
        } else {
          resolve(stringToColor(imageUrl));
        }
      } catch {
        // canvas tainted por CORS u otro error de seguridad
        resolve(stringToColor(imageUrl));
      }
    };
    img.onerror = () => resolve(stringToColor(imageUrl));
    img.src = imageUrl;
  });
}
