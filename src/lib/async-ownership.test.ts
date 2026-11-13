import { describe, expect, it } from "vitest";
import { LyricsCoordinator, LyricsOwnership, requestInView } from "./async-ownership";

const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};

describe("lyrics ownership", () => {
  it("drops old track success, failure and finalizer without clearing newer busy", async () => {
    const owner = new LyricsOwnership<string>();
    owner.track("a");
    const old = deferred<string>();
    const updates: string[] = [];
    const errors: unknown[] = [];
    const first = owner.run(() => old.promise, updates.push.bind(updates), errors.push.bind(errors));
    owner.track("b");
    const next = deferred<string>();
    const second = owner.run(() => next.promise, updates.push.bind(updates), errors.push.bind(errors));
    old.reject("stale");
    await first;
    expect([updates, errors, owner.busy]).toEqual([[], [], true]);
    next.resolve("b");
    await second;
    expect([updates, errors, owner.busy]).toEqual([["b"], [], false]);
    const late = deferred<string>();
    const third = owner.run(() => late.promise, updates.push.bind(updates), errors.push.bind(errors));
    owner.track("c");
    late.resolve("old b");
    await third;
    expect(updates).toEqual(["b"]);
  });

  it("manual same-track intent wins over automatic responses and ambiguous events", async () => {
    const owner = new LyricsOwnership<string>();
    owner.track("a");
    expect(owner.event("a")).toBe(true);
    expect(owner.event("b")).toBe(false);
    const automatic = deferred<string>();
    const updates: string[] = [];
    const first = owner.run(() => automatic.promise, value => updates.push(value));
    expect(owner.event("a")).toBe(false);
    await owner.run(async () => "manual", value => updates.push(value));
    automatic.resolve("automatic");
    await first;
    expect(updates).toEqual(["manual"]);
    expect(owner.event("a")).toBe(false);
  });

  it("closing a view drops candidates, selection, errors and finalizers", async () => {
    for (const reject of [false, true]) {
      const owner = new LyricsOwnership<string>();
      owner.track("a");
      const pending = deferred<string>();
      const updates: string[] = [];
      const errors: unknown[] = [];
      const task = owner.run(() => pending.promise, value => updates.push(value), error => errors.push(error));
      owner.invalidate();
      if (reject) pending.reject("closed"); else pending.resolve("candidate or selection");
      await task;
      expect([updates, errors, owner.busy]).toEqual([[], [], false]);
    }
  });

  it("retains failures as failures and permits explicit retry", async () => {
    const owner = new LyricsOwnership<string>();
    owner.track("a");
    const updates: string[] = [];
    const errors: unknown[] = [];
    await owner.run(async () => { throw "offline"; }, value => updates.push(value), error => errors.push(error));
    expect([updates, errors, owner.busy]).toEqual([[], ["offline"], false]);
    await owner.run(async () => "retry", value => updates.push(value));
    expect([updates, errors, owner.busy]).toEqual([["retry"], ["offline"], false]);
  });
});

// These are the coordinator callbacks used by App, and the requestInView seam
// used by both refresh/select handlers. No React/IPC facsimile is substituted.
const seam = () => {
  const docs: ({ track_id: string; text: string } | null)[] = [];
  const errors: unknown[] = [];
  const busy: boolean[] = [];
  const app = new LyricsCoordinator<{ track_id: string; text: string }>(
    doc => docs.push(doc), value => busy.push(value), error => errors.push(error),
  );
  app.startTrack("a");
  docs.length = 0;
  return { app, docs, errors, busy };
};
const doc = (track_id = "a", text = "lyrics") => ({ track_id, text });

describe("production App/view coordination seams", () => {
  it("rejects direct requester identity mismatch", async () => {
    const { app, docs, busy } = seam();
    await app.request(async () => doc("b"), () => true, "a");
    expect(docs).toEqual([]);
    expect(busy[busy.length - 1]).toBe(false);
  });
  it("rejects foreign Ready and Unavailable events without changing loading", () => {
    const { app, docs, busy } = seam();
    const before = [...busy];
    app.event({ type: "Ready", data: doc("b") });
    app.event({ type: "Unavailable", data: { track_id: "b" } });
    expect([docs, busy]).toEqual([[], before]);
  });
  it("retains initial event-driven loading success", () => {
    const { app, docs, busy } = seam();
    app.event({ type: "Ready", data: doc() });
    expect(docs).toEqual([doc()]);
    expect(busy[busy.length - 1]).toBe(false);
  });
  it("TrackStarted invalidates immediately, before getState resolves", async () => {
    const { app, docs } = seam();
    const old = deferred<ReturnType<typeof doc>>();
    const state = deferred<string>();
    const task = app.request(() => old.promise, () => true, "a");
    const readback = app.readState(async () => ({ current: { id: await state.promise } }), () => {}, true);
    app.startTrack("b");
    old.resolve(doc());
    await task;
    expect(docs).toEqual([null]);
    state.resolve("a");
    await readback;
    expect(app.matches("b")).toBe(true);
    app.event({ type: "Ready", data: doc("b") });
    expect(docs).toEqual([null, doc("b")]);
  });
  it("manual selection supersedes same-track automatic success and events", async () => {
    const { app, docs } = seam();
    const old = deferred<ReturnType<typeof doc>>();
    const automatic = app.request(() => old.promise, () => true, "a");
    const view = new LyricsOwnership<void>();
    await requestInView(view, app.request, async () => doc("a", "selected"), "a");
    app.event({ type: "Ready", data: doc("a", "ambiguous") });
    app.event({ type: "Unavailable", data: { track_id: "a" } });
    old.resolve(doc());
    await automatic;
    expect(docs).toEqual([doc("a", "selected")]);
  });
  it("late errors and finalizers leave the newer request busy", async () => {
    const { app, docs, errors, busy } = seam();
    const old = deferred<ReturnType<typeof doc>>();
    const next = deferred<ReturnType<typeof doc>>();
    const first = app.request(() => old.promise, () => true, "a");
    const second = app.request(() => next.promise, () => true, "a");
    old.reject("old error");
    await first;
    expect([docs, errors, busy[busy.length - 1]]).toEqual([[], [], true]);
    next.resolve(doc());
    await second;
    expect([docs, errors, busy[busy.length - 1]]).toEqual([[doc()], [], false]);
  });
  for (const lifecycle of ["close", "unmount", "track change"] as const) {
    for (const outcome of ["success", "failure"] as const) {
      it(`${lifecycle} suppresses selection ${outcome} through both production owners`, async () => {
        const { app, docs, errors } = seam();
        const view = new LyricsOwnership<void>();
        view.track("a");
        const pending = deferred<ReturnType<typeof doc>>();
        const task = requestInView(view, app.request, () => pending.promise, "a");
        if (lifecycle === "track change") { view.track("b"); app.startTrack("b"); }
        else view.invalidate();
        if (outcome === "success") pending.resolve(doc()); else pending.reject("late");
        await task;
        expect(docs).toEqual(lifecycle === "track change" ? [null] : []);
        expect(errors).toEqual([]);
        expect(view.busy).toBe(false);
      });
      it(`${lifecycle} suppresses candidate ${outcome}`, async () => {
        const candidates: string[][] = [];
        const errors: unknown[] = [];
        const search = new LyricsOwnership<string[]>();
        search.track("a");
        const pending = deferred<string[]>();
        const task = search.run(() => pending.promise, value => candidates.push(value), error => errors.push(error));
        if (lifecycle === "track change") search.track("b"); else search.invalidate();
        if (outcome === "success") pending.resolve(["old"]); else pending.reject("late");
        await task;
        expect([candidates, errors, search.busy]).toEqual([[], [], false]);
      });
    }
  }
  it("transient failure remains failure; explicit refresh succeeds without empty document", async () => {
    const { app, docs, errors, busy } = seam();
    const view = new LyricsOwnership<void>();
    await requestInView(view, app.request, async () => { throw "offline"; }, "a");
    expect([docs, errors, busy[busy.length - 1], view.busy]).toEqual([[], ["offline"], false, false]);
    await requestInView(view, app.request, async () => doc(), "a");
    expect([docs, errors, busy[busy.length - 1], view.busy]).toEqual([[doc()], ["offline"], false, false]);
  });
});
