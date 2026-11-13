import appSource from "../App.tsx?raw";
import ts from "typescript";
import { expect, it, vi } from "vitest";
import { FocusOwnership } from "./action-ownership";

const source = ts.createSourceFile("App.tsx", appSource, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
function deferred() {
  let resolve!: (value?: any) => void, reject!: (error: Error) => void;
  const promise = new Promise<any>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function fixture() {
  const callbacks: string[] = [];
  let listener = "";
  function visit(node: ts.Node) {
    if (ts.isVariableDeclaration(node) && ["handleAddToQueue", "handleSelectFocusDuration", "handleToggleAutoPause"].includes(node.name.getText(source))) callbacks.push(node.getText(source));
    if (ts.isCallExpression(node) && node.expression.getText(source) === "listenFocusEvents") listener = node.arguments[0].getText(source);
    ts.forEachChild(node, visit);
  }
  visit(source);
  const state = { focus: { phase: "Focus", is_running: true, remaining_seconds: 42, total_seconds: 1500, auto_pause_on_break: false }, error: null as string | null, toast: null as any };
  let queue = [{ id: "original", track: { id: "media", title: "Song" } }];
  const setters: ReturnType<typeof deferred>[] = [], reads: ReturnType<typeof deferred>[] = [];
  const removeFromQueue = vi.fn(async (id: string) => { queue = queue.filter(item => item.id !== id); });
  const addToQueue = vi.fn(async () => queue[0]);
  const setFocusState = (value: any) => { state.focus = typeof value === "function" ? value(state.focus) : value; };
  const set = vi.fn(() => { const d = deferred(); setters.push(d); return d.promise; });
  const getFocusState = vi.fn(() => { const d = deferred(); reads.push(d); return d.promise; });
  const owner = { current: null as any };
  const body = `${callbacks.map(text => `const ${text};`).join("\n")}\nreturn { handleAddToQueue, handleSelectFocusDuration, handleToggleAutoPause, event: ${listener} };`;
  const app = new Function("useCallback", "addToQueue", "getQueue", "removeFromQueue", "setToast", "setError", "setFocusState", "setFocusDurations", "setFocusAutoPause", "getFocusState", "focusOwner", ts.transpile(body, { target: ts.ScriptTarget.ES2020 }))(
    (fn: any) => fn, addToQueue, async () => queue, removeFromQueue, (v: any) => { state.toast = v; }, (v: string) => { state.error = v; }, setFocusState, set, set, getFocusState, owner,
  );
  return { app, state, setters, reads, set, getFocusState, removeFromQueue, addToQueue, owner, setFocusState, queue: () => queue, replace: (items: typeof queue) => { queue = items; } };
}
async function ownedFixture() {
  const f = fixture();
  f.owner.current = new FocusOwnership(f.setFocusState);
  return f;
}
for (const removed of [false, true]) it(`undo targets returned identity with duplicates; already removed=${removed}`, async () => {
  const f = fixture(), stopPropagation = vi.fn();
  await f.app.handleAddToQueue({ id: "media", title: "Song" }, { stopPropagation });
  expect(stopPropagation).toHaveBeenCalledOnce();
  expect(f.state.toast).toMatchObject({ text: "Añadida a la cola: Song", id: expect.any(String) });
  f.replace([...(removed ? [] : f.queue()), { id: "duplicate", track: { id: "media", title: "Song" } }]);
  await f.state.toast.onUndo();
  expect(f.removeFromQueue.mock.calls).toEqual(removed ? [] : [["original"]]);
  expect(f.queue().map(item => item.id)).toEqual(["duplicate"]);
  expect(f.state.error).toBeNull();
});
it("add failure preserves queue and does not publish success", async () => {
  const f = fixture(); f.addToQueue.mockRejectedValueOnce(new Error("add failed"));
  await f.app.handleAddToQueue({}, { stopPropagation() {} });
  expect(f.state.error).toBe("Error: add failed"); expect(f.state.toast).toBeNull();
  expect(f.queue().map(item => item.id)).toEqual(["original"]); expect(f.removeFromQueue).not.toHaveBeenCalled();
});
for (const phase of ["Focus", "ShortBreak", "LongBreak"]) for (const running of [true, false]) it(`duration readback preserves ${phase} running=${running}`, async () => {
  const f = await ownedFixture(); Object.assign(f.state.focus, { phase, is_running: running });
  const before = { ...f.state.focus };
  const task = f.app.handleSelectFocusDuration(50, 10);
  expect(f.set).toHaveBeenCalledWith(3000, 600, 1200);
  expect(f.state.focus).toEqual(before);
  f.setters[0].resolve(); await Promise.resolve();
  expect(f.state.focus).toEqual(before); expect(f.getFocusState).toHaveBeenCalledOnce();
  const authoritative = { ...before, auto_pause_on_break: true };
  f.reads[0].resolve(authoritative); await task;
  expect(f.state.focus).toEqual(authoritative); expect(f.state.error).toBeNull(); expect(f.state.toast).toBeNull();
});
it("auto pause uses authoritative readback and current failure remains visible", async () => {
  const f = await ownedFixture(), before = { ...f.state.focus };
  const task = f.app.handleToggleAutoPause(true); f.setters[0].resolve(); await Promise.resolve();
  expect(f.set).toHaveBeenCalledWith(true); expect(f.state.focus).toEqual(before);
  f.reads[0].resolve({ ...before, auto_pause_on_break: true }); await task;
  expect(f.state.focus.auto_pause_on_break).toBe(true);
  const failed = f.app.handleToggleAutoPause(false); f.setters[1].reject(new Error("denied")); await failed;
  expect(f.state.focus.auto_pause_on_break).toBe(true); expect(f.state.error).toBe("Error: denied"); expect(f.reads).toHaveLength(1);
});
it("new intent and actual focus listener invalidate late setters, errors and readbacks", async () => {
  const f = await ownedFixture();
  const old = f.app.handleSelectFocusDuration(50, 10);
  const latest = f.app.handleToggleAutoPause(true);
  f.setters[0].resolve(); await old; expect(f.reads).toHaveLength(0);
  f.setters[1].resolve(); await Promise.resolve();
  const event = { ...f.state.focus, remaining_seconds: 39, auto_pause_on_break: true };
  f.app.event(event); f.reads[0].resolve({ ...event, remaining_seconds: 999 }); await latest;
  expect(f.state.focus).toEqual(event);
  const staleFailure = f.app.handleToggleAutoPause(false);
  const current = f.app.handleSelectFocusDuration(25, 5);
  f.setters[2].reject(new Error("old")); await staleFailure; expect(f.state.error).toBeNull();
  f.app.event(event); f.setters[3].resolve(); await current;
  expect(f.reads).toHaveLength(1); expect(f.state.focus).toEqual(event);
});
it("initial read cannot overwrite a newer event or settings intent", async () => {
  for (const event of [true, false]) {
    const f = await ownedFixture(), pending = deferred();
    const read = f.owner.current.read(() => pending.promise);
    const before = { ...f.state.focus };
    if (event) f.app.event(before); else f.owner.current.begin();
    pending.resolve({ ...before, remaining_seconds: 999 }); await read;
    expect(f.state.focus).toEqual(before);
  }
});
it("current readback failure preserves display and reports error", async () => {
  const f = await ownedFixture(), before = { ...f.state.focus };
  const task = f.app.handleSelectFocusDuration(25, 5);
  f.setters[0].resolve(); await Promise.resolve();
  f.reads[0].reject(new Error("read failed")); await task;
  expect(f.state.focus).toEqual(before); expect(f.state.error).toBe("Error: read failed");
  expect(f.state.toast).toBeNull();
});
it("normal undo removes the added item; undo rejection only logs", async () => {
  const f = fixture();
  await f.app.handleAddToQueue({ title: "Song" }, { stopPropagation() {} });
  const toast = f.state.toast;
  const log = vi.spyOn(console, "error").mockImplementation(() => {});
  try {
    f.removeFromQueue.mockRejectedValueOnce(new Error("remove failed"));
    await toast.onUndo();
    expect(log).toHaveBeenCalledWith("Error al deshacer:", expect.any(Error));
    expect(f.queue()).toHaveLength(1); expect(f.state.error).toBeNull(); expect(f.state.toast).toBe(toast);
    await toast.onUndo(); expect(f.queue()).toEqual([]);
    expect(f.removeFromQueue.mock.calls).toEqual([["original"], ["original"]]);
  } finally { log.mockRestore(); }
});
