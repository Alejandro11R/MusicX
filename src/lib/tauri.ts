import { invoke } from "@tauri-apps/api/core";

// Field names deliberately mirror the Rust structs (snake_case) exactly —
// there's no serde rename on the backend, so this is what actually crosses
// the IPC boundary. Renaming to camelCase here would just add a mapping
// layer with no behavior.

export type PlaybackStatus = "Stopped" | "Loading" | "Playing" | "Paused";

export interface SearchResult {
  id: string;
  title: string;
  artist: string | null;
  duration_seconds: number;
  thumbnail_url: string;
}

export interface PlayerState {
  status: PlaybackStatus;
  current: SearchResult | null;
  position_seconds: number;
  duration_seconds: number;
  volume: number;
}

export interface HealthStatus {
  mpv: boolean;
  yt_dlp: boolean;
}

export function search(query: string, limit: number): Promise<SearchResult[]> {
  return invoke("search", { query, limit });
}

export function play(track: SearchResult): Promise<void> {
  return invoke("play", { track });
}

export function pause(): Promise<void> {
  return invoke("pause");
}

export function resume(): Promise<void> {
  return invoke("resume");
}

export function stop(): Promise<void> {
  return invoke("stop");
}

export function setVolume(level: number): Promise<void> {
  return invoke("set_volume", { level });
}

export function getState(): Promise<PlayerState> {
  return invoke("state");
}

export function health(): Promise<HealthStatus> {
  return invoke("health");
}
