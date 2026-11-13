import { expect, it } from "vitest";
import { LyricsCoordinator, LyricsOwnership, requestInView } from "./async-ownership";

const doc = (track_id: string) => ({ track_id });
const setup = () => {
  const docs: ({ track_id: string } | null)[] = [];
  const busy: boolean[] = [];
  const errors: unknown[] = [];
  const app = new LyricsCoordinator<{ track_id: string }>(d => docs.push(d), b => busy.push(b), e => errors.push(e));
  return { app, docs, busy, errors };
};

it("App state-read seam distinguishes event readback from authoritative poll intent", async () => {
  const { app, docs, errors } = setup();
  const states: string[] = [];
  const apply = (state: { current: { id: string } | null }) => states.push(state.current?.id ?? "none");
  app.observeTrack("a");
  let resolve!: (state: { current: { id: string } }) => void;
  const pending = new Promise<{ current: { id: string } }>(yes => { resolve = yes; });
  const oldPoll = app.readState(() => pending, apply, true);
  app.startTrack("b");
  resolve({ current: { id: "a" } });
  await oldPoll;
  // Even a read initiated by TrackStarted itself can see the old mpv state.
  await app.readState(async () => ({ current: { id: "a" } }), apply, false);
  expect(app.matches("b")).toBe(true);
  expect(states).toEqual([]);
  app.event({ type: "Ready", data: doc("b") });
  expect(docs).toEqual([null, null, doc("b")]);
  // A later poll/reconnect is a new authoritative observation, not that readback.
  await app.readState(async () => ({ current: { id: "c" } }), apply, true);
  app.event({ type: "Ready", data: doc("c") });
  expect(states).toEqual(["c"]);
  expect(docs).toEqual([null, null, doc("b"), null, doc("c")]);
  expect(errors).toEqual([]);
});

it("authoritative B observation without TrackStarted accepts BReady", () => {
  const { app, docs, busy, errors } = setup();
  app.observeTrack("a");
  app.observeTrack("b");
  app.event({ type: "Ready", data: doc("b") });
  expect(app.matches("b")).toBe(true);
  expect(docs).toEqual([null, null, doc("b")]);
  expect(busy[busy.length - 1]).toBe(false);
  expect(errors).toEqual([]);
});

it("stale displayed A request cannot suppress BReady", async () => {
  const { app, docs, busy, errors } = setup();
  app.startTrack("b");
  const before = [...busy];
  const view = new LyricsOwnership<void>();
  view.track("a");
  let loads = 0;
  await requestInView(view, app.request, async () => { loads++; return doc("a"); }, "a");
  expect(busy).toEqual(before);
  app.event({ type: "Ready", data: doc("b") });
  expect(docs).toEqual([null, doc("b")]);
  expect(loads).toBe(0);
  expect(errors).toEqual([]);
  expect(view.busy).toBe(false);
});

it("pending B cannot be superseded by stale displayed A", async () => {
  const { app, docs, busy, errors } = setup();
  app.startTrack("b");
  let resolve!: (value: { track_id: string }) => void;
  const pending = new Promise<{ track_id: string }>(yes => { resolve = yes; });
  const task = app.request(() => pending, () => true, "b");
  const before = [...busy];
  const view = new LyricsOwnership<void>();
  view.track("a");
  let loads = 0;
  await requestInView(view, app.request, async () => { loads++; return doc("a"); }, "a");
  expect(busy).toEqual(before);
  resolve(doc("b"));
  await task;
  expect(docs).toEqual([null, doc("b")]);
  expect(loads).toBe(0);
  expect(errors).toEqual([]);
  expect(busy[busy.length - 1]).toBe(false);
});
