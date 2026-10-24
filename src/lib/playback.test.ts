import { describe, expect, it } from "vitest";
import {
  clamp,
  formatDuration,
  safeDuration,
  safePosition,
  seekTarget,
} from "./playback";

describe("clamp", () => {
  it("respeta el rango", () => {
    expect(clamp(5, 0, 10)).toBe(5);
    expect(clamp(-5, 0, 10)).toBe(0);
    expect(clamp(15, 0, 10)).toBe(10);
  });

  it("usa el fallback para valores no finitos", () => {
    expect(clamp(Number.NaN, 0, 10, 3)).toBe(3);
    expect(clamp(Number.POSITIVE_INFINITY, 0, 10, 3)).toBe(3);
  });

  it("no explota si max < min", () => {
    expect(clamp(5, 10, 0)).toBe(10);
  });
});

describe("safeDuration", () => {
  it("devuelve 0 para duraciones inválidas", () => {
    expect(safeDuration(0)).toBe(0);
    expect(safeDuration(-10)).toBe(0);
    expect(safeDuration(Number.NaN)).toBe(0);
    expect(safeDuration(Number.POSITIVE_INFINITY)).toBe(0);
  });

  it("conserva duraciones válidas", () => {
    expect(safeDuration(187.4)).toBe(187.4);
  });
});

describe("safePosition", () => {
  it("acota la posición a la duración conocida", () => {
    expect(safePosition(30, 100)).toBe(30);
    expect(safePosition(-5, 100)).toBe(0);
    expect(safePosition(150, 100)).toBe(100);
  });

  it("sin duración solo evita negativos", () => {
    expect(safePosition(150, 0)).toBe(150);
    expect(safePosition(-1, 0)).toBe(0);
  });

  it("sanea posiciones no finitas", () => {
    // Una posición no finita es un bug del backend: la tratamos como 0 en
    // vez de dejar el slider en un estado inválido.
    expect(safePosition(Number.NaN, 100)).toBe(0);
    expect(safePosition(Number.POSITIVE_INFINITY, 100)).toBe(0);
    expect(safePosition(Number.NEGATIVE_INFINITY, 100)).toBe(0);
  });
});

describe("seekTarget", () => {
  it("suma el delta dentro de la duración", () => {
    expect(seekTarget(10, 5, 100)).toBe(15);
    expect(seekTarget(10, -5, 100)).toBe(5);
  });

  it("no baja de 0 ni pasa de la duración", () => {
    expect(seekTarget(2, -10, 100)).toBe(0);
    expect(seekTarget(98, 10, 100)).toBe(100);
  });

  it("sin duración conocida no permite negativos", () => {
    expect(seekTarget(2, -10, 0)).toBe(0);
    expect(seekTarget(2, 10, 0)).toBe(12);
  });

  it("trata una posición no finita como 0", () => {
    expect(seekTarget(Number.NaN, 5, 100)).toBe(5);
  });
});

describe("formatDuration", () => {
  it("formatea minutos y segundos con padding", () => {
    expect(formatDuration(0)).toBe("0:00");
    expect(formatDuration(9)).toBe("0:09");
    expect(formatDuration(59.9)).toBe("0:59");
    expect(formatDuration(60)).toBe("1:00");
    expect(formatDuration(187)).toBe("3:07");
    expect(formatDuration(3599)).toBe("59:59");
  });

  it("usa horas para audios largos", () => {
    expect(formatDuration(3600)).toBe("1:00:00");
    expect(formatDuration(3661)).toBe("1:01:01");
  });

  it("sanea valores inválidos", () => {
    expect(formatDuration(-1)).toBe("0:00");
    expect(formatDuration(Number.NaN)).toBe("0:00");
    expect(formatDuration(Number.POSITIVE_INFINITY)).toBe("0:00");
  });
});
