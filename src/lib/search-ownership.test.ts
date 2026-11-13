import appSource from "../App.tsx?raw";
import ts from "typescript";
import { expect, it } from "vitest";
import { SearchOwnership } from "./search-ownership";

const source = ts.createSourceFile("App.tsx", appSource, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
function fixture() {
  const functions: string[] = [];
  let change = "", clear = "";
  function visit(node: ts.Node) {
    if (ts.isFunctionDeclaration(node) && ["runSearch", "handleSearchKeyDown"].includes(node.name?.text ?? "")) functions.push(node.getText(source));
    if (ts.isJsxAttribute(node) && node.initializer && ts.isJsxExpression(node.initializer)) {
      if (node.name.getText(source) === "onChange" && node.initializer.expression?.getText(source).includes("setQuery")) change = node.initializer.expression.getText(source);
    }
    if (ts.isJsxElement(node) && node.openingElement.attributes.properties.some(p => ts.isJsxAttribute(p) && p.name.getText(source) === "aria-label" && p.initializer?.getText(source) === '"Limpiar búsqueda"')) {
      const attr = node.openingElement.attributes.properties.find(p => ts.isJsxAttribute(p) && p.name.getText(source) === "onClick") as ts.JsxAttribute;
      clear = (attr.initializer as ts.JsxExpression).expression!.getText(source);
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
  expect(change).not.toBe("");
  expect(clear).not.toBe("");
  const state = { query: "old", results: ["retained" as unknown], resultsQuery: "old", selectedIndex: 0 as number | null, error: null as string | null, isSearching: false };
  const pending: { resolve: (value: unknown[]) => void; reject: (error: Error) => void }[] = [];
  const calls: unknown[] = [], played: unknown[] = [];
  const search = (...args: unknown[]) => { calls.push(args); return new Promise((resolve, reject) => pending.push({ resolve, reject })); };
  const names = Object.keys(state);
  const setters = names.map(name => (value: unknown) => { (state as Record<string, unknown>)[name] = value; });
  const body = `${functions.join("\n")}\nreturn { runSearch, key: handleSearchKeyDown, change: ${change}, clear: ${clear} };`;
  const js = ts.transpile(body, { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.None });
  const app = new Function(...names, ...names.map(n => `set${n[0].toUpperCase()}${n.slice(1)}`), "search", "searchOwner", "searchInputRef", "handlePlay", js)(...Object.values(state), ...setters, search, { current: new SearchOwnership(value => { state.isSearching = value; }) }, { current: { focus() {} } }, (track: unknown) => played.push(track));
  return { app, state, pending, calls, played };
}
const failure = () => new Error("remote failed");

for (const action of ["local", "empty", "clear", "Escape", "query", "remote"] as const) {
  it(`${action} invalidates stale success, error and finalizer through App callbacks`, async () => {
    for (const rejects of [false, true]) {
      const { app, state, pending, calls } = fixture();
      const old = app.runSearch("old remote");
      let latest: Promise<void> | undefined;
      if (action === "local") await app.runSearch(" /music/song.MP3 ");
      if (action === "empty") await app.runSearch("  ");
      if (action === "clear") app.clear();
      if (action === "Escape") app.key({ key: "Escape", preventDefault() {} });
      if (action === "query") app.change({ currentTarget: { value: "new query" } });
      if (action === "remote") latest = app.runSearch("new remote");
      const before = structuredClone(state);
      if (rejects) pending[0].reject(failure()); else pending[0].resolve(["stale"]);
      await old;
      expect(state).toEqual(before);
      expect(state.isSearching).toBe(action === "remote");
      expect(calls).toEqual(action === "remote" ? [["old remote", 10], ["new remote", 10]] : [["old remote", 10]]);
      if (latest) { pending[1].resolve(["new"]); await latest; expect(state.results).toEqual(["new"]); expect(state.isSearching).toBe(false); }
      if (action === "local") { expect(state.results).toEqual([{ id: "/music/song.MP3", title: "song", artist: "Archivo Local", duration_seconds: 0, thumbnail_url: "" }]); expect(state.selectedIndex).toBe(0); }
      if (action === "clear" || action === "Escape") expect(state).toMatchObject({ query: "", results: [], resultsQuery: "", selectedIndex: null });
      if (action === "query") expect(state.query).toBe("new query");
    }
  });
}
it("current search retains results while pending, accepts success and resets selection", async () => {
  const { app, state, pending } = fixture();
  const task = app.runSearch("new");
  expect(state).toMatchObject({ results: ["retained"], selectedIndex: 0, error: null, isSearching: true });
  pending[0].resolve(["found"]); await task;
  expect(state).toMatchObject({ results: ["found"], resultsQuery: "new", selectedIndex: null, error: null, isSearching: false });
});
it("current rejection is visible but AbortError is expected; retained results survive", async () => {
  for (const abort of [false, true]) {
    const { app, state, pending } = fixture();
    const task = app.runSearch("new");
    const error = failure(); if (abort) error.name = "AbortError";
    pending[0].reject(error); await task;
    expect(state).toMatchObject({ results: ["retained"], resultsQuery: "old", selectedIndex: 0, error: abort ? null : "Error: remote failed", isSearching: false });
  }
});
it("Enter still plays fresh results", () => {
  const { app, played } = fixture();
  app.key({ key: "Enter", preventDefault() {} });
  expect(played).toEqual(["retained"]);
});
