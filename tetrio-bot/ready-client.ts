import { withTimeout } from "./timeout";

interface Connection {
  disconnected: boolean;
  on(event: "client.dead", listener: () => void): unknown;
  off(event: "client.dead", listener: () => void): unknown;
  destroy(): Promise<unknown>;
}
interface Slot<C> {
  promise: Promise<C>;
  client?: C;
  discarded: boolean;
  dead?: () => void;
}

/** One roomless, authenticated spare. A claimed connection belongs to its worker. */
export class ReadyClient<C extends Connection> {
  private spare?: Slot<C>;
  private retry?: ReturnType<typeof setTimeout>;
  private closed = false;

  constructor(
    private readonly create: () => Promise<C>,
    private readonly log: (message: string) => void = console.log,
    private readonly connectMs = 15000,
    private readonly retryMs = 5000,
  ) {
    this.prepare();
  }

  private async dispose(client: C) {
    await withTimeout(
      Promise.resolve().then(() => client.destroy()),
      5000,
      "Spare cleanup",
    ).catch(() => {});
  }

  private schedule() {
    if (this.closed || this.retry || this.spare) return;
    this.retry = setTimeout(() => {
      this.retry = undefined;
      this.prepare();
    }, this.retryMs);
    this.retry.unref?.();
  }

  private connect(): Slot<C> {
    const slot = { discarded: false } as Slot<C>;
    const started = performance.now();
    const pending = Promise.resolve()
      .then(this.create)
      .then(async (client) => {
        if (slot.discarded || this.closed || client.disconnected) {
          await this.dispose(client);
          throw new Error("Connection closed before use");
        }
        slot.client = client;
        if (this.spare === slot) {
          slot.dead = () => {
            if (this.spare !== slot) return;
            this.spare = undefined;
            slot.discarded = true;
            client.off("client.dead", slot.dead!);
            void this.dispose(client);
            this.log(
              "[4wide-bot] Spare disconnected; preparing a replacement.",
            );
            this.schedule();
          };
          client.on("client.dead", slot.dead);
          this.log(
            `[4wide-bot] Spare connection ready in ${Math.round(performance.now() - started)} ms.`,
          );
        }
        return client;
      });
    slot.promise = withTimeout(
      pending,
      this.connectMs,
      "Worker connection",
      () => {
        slot.discarded = true;
      },
    );
    // Observe failures even when nobody has claimed the spare yet.
    void slot.promise.catch((error) => {
      slot.discarded = true;
      if (this.spare === slot) {
        this.spare = undefined;
        this.log(
          `[4wide-bot] Spare connection failed: ${error instanceof Error ? error.message : String(error)}`,
        );
        this.schedule();
      }
    });
    return slot;
  }

  private prepare() {
    if (!this.closed && !this.spare) this.spare = this.connect();
  }

  async take(): Promise<C> {
    if (this.closed) throw new Error("Connection supplier is closed");
    if (this.retry) clearTimeout(this.retry);
    this.retry = undefined;
    const slot = this.spare ?? this.connect();
    // Transfer ownership before awaiting, so simultaneous invites cannot share it.
    this.spare = undefined;
    if (slot.client && slot.dead) slot.client.off("client.dead", slot.dead);
    this.prepare();
    const client = await slot.promise;
    if (this.closed || client.disconnected) {
      await this.dispose(client);
      throw new Error("Connection closed before assignment");
    }
    return client;
  }

  async close() {
    this.closed = true;
    if (this.retry) clearTimeout(this.retry);
    this.retry = undefined;
    const slot = this.spare;
    this.spare = undefined;
    if (!slot) return;
    slot.discarded = true;
    if (slot.client) {
      if (slot.dead) slot.client.off("client.dead", slot.dead);
      await this.dispose(slot.client);
    }
    // A pending connection disposes itself if it resolves after shutdown.
  }
}
