import {
  mkdir,
  readdir,
  stat,
  unlink,
  writeFile,
  rename,
} from "node:fs/promises";
import path from "node:path";
import { randomUUID } from "node:crypto";
export interface ReplayLimits {
  maxFiles: number;
  maxBytes: number;
  maxAgeMs: number;
  maxFileBytes: number;
}
export class ReplayStore {
  private tail: Promise<unknown> = Promise.resolve();
  private pendingBytes = 0;
  constructor(
    readonly directory: string,
    readonly limits: ReplayLimits,
  ) {}
  async prune(incoming = 0) {
    await mkdir(this.directory, { recursive: true, mode: 0o700 });
    const files = [];
    for (const entry of await readdir(this.directory, {
      withFileTypes: true,
    })) {
      if (
        !entry.isFile() ||
        !/^invincible-[a-zA-Z0-9_.-]+\.ttrm$/.test(entry.name)
      )
        continue;
      const file = path.join(this.directory, entry.name);
      const info = await stat(file);
      files.push({ file, size: info.size, time: info.mtimeMs });
    }
    files.sort((a, b) => a.time - b.time);
    let bytes = files.reduce((s, f) => s + f.size, 0),
      count = files.length;
    for (const f of files) {
      if (
        Date.now() - f.time > this.limits.maxAgeMs ||
        bytes + incoming > this.limits.maxBytes ||
        count + (incoming > 0 ? 1 : 0) > this.limits.maxFiles
      ) {
        await unlink(f.file);
        bytes -= f.size;
        count--;
      }
    }
  }
  maintain() {
    const job = this.tail.then(() => this.prune());
    this.tail = job.catch(() => {});
    return job;
  }
  save(room: string, replay: unknown, partial = false): Promise<string> {
    // Snapshot now: later rounds may mutate the SDK replay object.
    const json = JSON.stringify(replay);
    const bytes = Buffer.byteLength(json);
    if (
      bytes > this.limits.maxFileBytes ||
      bytes > this.limits.maxBytes ||
      this.pendingBytes + bytes > 128 * 1024 * 1024
    )
      return Promise.reject(new Error("Replay size/queue limit exceeded"));
    this.pendingBytes += bytes;
    const safeRoom = room.replace(/[^a-zA-Z0-9_-]/g, "_").slice(0, 32);
    const name = `invincible-${Date.now()}-${safeRoom}-${randomUUID()}${partial ? ".partial" : ""}.ttrm`;
    const job = this.tail
      .then(async () => {
        await this.prune(bytes);
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
      })
      .finally(() => {
        this.pendingBytes -= bytes;
      });
    this.tail = job.catch(() => {});
    return job;
  }
}
