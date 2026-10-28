import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

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
  looping: boolean;
  /** false mientras el backend está reconectando con mpv. */
  connected: boolean;
}

export interface HealthStatus {
  mpv: boolean;
  yt_dlp: boolean;
}

export interface QueueItem {
  id: string;
  track: SearchResult;
}

// Orchestrator events. The backend serializes them with
// #[serde(tag = "type", content = "data")].
export type OrchestratorEvent =
  | { type: "TrackStarted"; data: SearchResult }
  | { type: "TrackEnded"; data: { reason: string } }
  | { type: "PlaybackError"; data: string }
  | { type: "StateChanged"; data: PlaybackStatus }
  | { type: "ConnectionChanged"; data: { connected: boolean } }
  | { type: "QueueUpdated"; data: QueueItem[] }
  | { type: "Progress"; data: { position_seconds: number; duration_seconds: number } };

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

export function seek(positionSeconds: number): Promise<void> {
  return invoke("seek", { positionSeconds });
}

export function setLoop(enabled: boolean): Promise<void> {
  return invoke("set_loop", { enabled });
}

export function nextTrack(): Promise<void> {
  return invoke("next");
}

export function previousTrack(): Promise<void> {
  return invoke("previous");
}

export function getQueue(): Promise<QueueItem[]> {
  return invoke("get_queue");
}

export function addToQueue(track: SearchResult): Promise<QueueItem> {
  return invoke("add_to_queue", { track });
}

export function addToQueueNext(track: SearchResult): Promise<QueueItem> {
  return invoke("add_to_queue_next", { track });
}

export function removeFromQueue(itemId: string): Promise<boolean> {
  return invoke("remove_from_queue", { itemId });
}

export function clearQueue(): Promise<void> {
  return invoke("clear_queue");
}

export function playQueueIndex(index: number): Promise<void> {
  return invoke("play_queue_index", { index });
}

export function getState(): Promise<PlayerState> {
  return invoke("state");
}

export function health(): Promise<HealthStatus> {
  return invoke("health");
}

export function quit(): Promise<void> {
  return invoke("quit");
}

/// Subscribe to orchestrator events (player-state-changed, track-ended, etc.)
export function subscribeEvents(): Promise<void> {
  return invoke("subscribe_events");
}

/// Listen for orchestrator events from the backend
export function listenOrchestratorEvents(
  callback: (event: OrchestratorEvent) => void
): Promise<UnlistenFn> {
  return listen<OrchestratorEvent>("orchestrator-event", (e) => callback(e.payload));
}