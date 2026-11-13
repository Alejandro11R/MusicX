export type LyricsRequest<T> = (load: () => Promise<T>, alive: () => boolean, trackId: string) => Promise<void>;

// Shared by both views: the lease must be captured AFTER run begins its generation.
export function requestInView<T>(owner: LyricsOwnership<void>, request: LyricsRequest<T>, load: () => Promise<T>, trackId: string) {
  return owner.run(() => request(load, owner.lease(), trackId), () => {});
}

// App's event/request seam. TrackStarted wins immediately, without waiting for
// getState; subsequent readbacks cannot restore an earlier lyrics identity.
export class LyricsCoordinator<T extends { track_id: string }> {
  private owner: LyricsOwnership<T>;
  private id: string | null = null;
  private observation = 0;
  constructor(private apply: (doc: T | null) => void, private loading: (busy: boolean) => void,
    private fail: (error: unknown) => void = console.error) {
    this.owner = new LyricsOwnership<T>(loading);
  }
  observeTrack(id: string | null) {
    if (this.id !== id) {
      this.observation++;
      this.id = id;
      this.owner.track(id);
      this.apply(null);
    }
  }
  // Capture at IPC initiation, not in a React effect after completion. Poll and
  // reconnect are authoritative; event readbacks may lag TrackStarted in mpv.
  async readState<S extends { current: { id: string } | null }>(
    load: () => Promise<S>, apply: (state: S) => void, authoritative: boolean,
  ) {
    const token = ++this.observation;
    const state = await load();
    if (token !== this.observation) return;
    const id = state.current?.id ?? null;
    if (!authoritative && this.id !== null && id !== this.id) return;
    this.observeTrack(id);
    apply(state);
  }
  matches(id: string) { return this.id === id; }
  startTrack(id: string) {
    this.observation++;
    this.owner.track(null);
    this.id = id;
    this.owner.track(id);
    this.apply(null);
  }
  event(event: { type: "Ready"; data: T } | { type: "Unavailable"; data: { track_id: string } }) {
    if (!this.owner.event(event.data.track_id)) return;
    if (event.type === "Ready") this.apply(event.data);
    this.loading(false);
  }
  request: LyricsRequest<T> = (load, alive, id) => {
    // Rejected stale views must not acquire the global generation or set requested.
    if (id !== this.id || !alive()) return Promise.resolve();
    return this.owner.run(load, doc => {
      if (alive() && doc.track_id === id) this.apply(doc);
    }, error => { if (alive()) this.fail(error); });
  };
  dispose() { this.observation++; this.owner.invalidate(); }
}

// Promise ownership, not cancellation: IPC may finish after its view disappears.
export class LyricsOwnership<T> {
  busy = false;
  private generation = 0;
  private id: string | null = null;
  private requested = false;
  constructor(private changed: (busy: boolean) => void = () => {}) {}

  track(id: string | null) {
    if (this.id === id) return;
    this.invalidate();
    this.id = id;
    this.requested = false;
  }

  invalidate() {
    this.generation++;
    this.busy = false;
    this.changed(false);
  }

  lease() {
    const generation = this.generation;
    return () => generation === this.generation;
  }

  event(id: string) {
    // Events carry requester identity but no request generation. Once a direct
    // request starts, even a same-track event cannot safely replace its result.
    return id === this.id && !this.requested;
  }

  async run(
    load: () => Promise<T>,
    apply: (value: T) => void,
    fail: (error: unknown) => void = console.error,
  ) {
    const generation = ++this.generation;
    this.requested = true;
    this.busy = true;
    this.changed(true);
    try {
      const value = await load();
      if (generation === this.generation) apply(value);
    } catch (error) {
      if (generation === this.generation) fail(error);
    } finally {
      if (generation === this.generation) {
        this.busy = false;
        this.changed(false);
      }
    }
  }
}
