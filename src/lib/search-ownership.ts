/** Owns UI publication, not transport cancellation: search IPC is not abortable. */
export class SearchOwnership {
  private generation = 0;

  constructor(private readonly setBusy: (busy: boolean) => void) {}

  invalidate() {
    this.generation++;
    this.setBusy(false);
  }

  begin() {
    this.invalidate();
    const generation = this.generation;
    return () => generation === this.generation;
  }
}
