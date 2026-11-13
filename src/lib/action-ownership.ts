import type { FocusState } from "./tauri";

/** Settings/readbacks publish only while neither intent nor events have advanced. */
export class FocusOwnership {
  private generation = 0;
  constructor(private readonly publish: (state: FocusState) => void) {}

  begin() {
    const generation = ++this.generation;
    return () => generation === this.generation;
  }

  event(state: FocusState) {
    this.generation++;
    this.publish(state);
  }

  async read(load: () => Promise<FocusState>, owns = this.begin()) {
    if (!owns()) return;
    const state = await load();
    if (owns()) this.publish(state);
  }
}
