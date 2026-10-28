import { describe, it, expect } from "vitest";
import { findActiveLineIndex, getWordProgress, formatLyricTime } from "./lyrics";
import { LyricLine } from "./tauri";

describe("lyrics helper functions", () => {
  const sampleLines: LyricLine[] = [
    { time_ms: 1000, text: "First line" },
    { time_ms: 5000, text: "Second line" },
    { time_ms: 10000, text: "Third line" },
    { time_ms: 20000, text: "Fourth line" },
  ];

  describe("findActiveLineIndex", () => {
    it("returns -1 when lines is empty", () => {
      expect(findActiveLineIndex([], 5)).toBe(-1);
    });

    it("returns -1 before the first line starts", () => {
      expect(findActiveLineIndex(sampleLines, 0.5)).toBe(-1);
    });

    it("finds the exact start of the first line", () => {
      expect(findActiveLineIndex(sampleLines, 1.0)).toBe(0);
    });

    it("maintains current line between timestamps", () => {
      expect(findActiveLineIndex(sampleLines, 2.5)).toBe(0);
      expect(findActiveLineIndex(sampleLines, 4.99)).toBe(0);
    });

    it("advances to second line at 5.0s", () => {
      expect(findActiveLineIndex(sampleLines, 5.0)).toBe(1);
    });

    it("remains on last line after final timestamp", () => {
      expect(findActiveLineIndex(sampleLines, 25.0)).toBe(3);
    });

    it("respects positive offset (advances lyric earlier)", () => {
      // Current position 0.8s + 300ms offset = 1.1s -> line 0 active
      expect(findActiveLineIndex(sampleLines, 0.8, 300)).toBe(0);
    });

    it("respects negative offset (delays lyric)", () => {
      // Current position 1.1s - 200ms offset = 0.9s -> before line 0
      expect(findActiveLineIndex(sampleLines, 1.1, -200)).toBe(-1);
    });
  });

  describe("getWordProgress", () => {
    const word = { text: "Hello", time_ms: 1000, end_ms: 2000 };

    it("returns 0 before word starts", () => {
      expect(getWordProgress(word, 500)).toBe(0);
    });

    it("returns 0.5 halfway through", () => {
      expect(getWordProgress(word, 1500)).toBe(0.5);
    });

    it("returns 1 at and after end_ms", () => {
      expect(getWordProgress(word, 2000)).toBe(1);
      expect(getWordProgress(word, 2500)).toBe(1);
    });
  });

  describe("formatLyricTime", () => {
    it("formats 0 as 00:00", () => {
      expect(formatLyricTime(0)).toBe("00:00");
    });

    it("formats 65 seconds as 01:05", () => {
      expect(formatLyricTime(65)).toBe("01:05");
    });

    it("formats 720 seconds as 12:00", () => {
      expect(formatLyricTime(720)).toBe("12:00");
    });
  });
});
