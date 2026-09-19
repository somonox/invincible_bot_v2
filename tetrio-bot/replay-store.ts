import { mkdir, unlink, writeFile, rename } from "node:fs/promises";
import path from "node:path";
import { randomUUID } from "node:crypto";
export class ReplayStore {
  private tail: Promise<unknown> = Promise.resolve();
  constructor(readonly directory: string) {}
  prepare() {
    return mkdir(this.directory, { recursive: true, mode: 0o700 });
  }
  flush() {
    return this.tail;
  }
  save(room: string, replay: unknown, partial = false): Promise<string> {
    // Snapshot now: later rounds may mutate the SDK replay object.
    const json = JSON.stringify(replay);
    const safeRoom = room.replace(/[^a-zA-Z0-9_-]/g, "_").slice(0, 32);
    const name = `invincible-${Date.now()}-${safeRoom}-${randomUUID()}${partial ? ".partial" : ""}.ttrm`;
    const job = this.tail.then(async () => {
      await this.prepare();
      const file = path.join(this.directory, name),
        temporary = file + ".tmp";
      try {
        await writeFile(temporary, json, { flag: "wx", mode: 0o600 });
        await rename(temporary, file);
      } catch (error) {
        await unlink(temporary).catch(() => {});
        throw error;
      }
      return file;
    });
    this.tail = job.catch(() => {});
    return job;
  }
}
