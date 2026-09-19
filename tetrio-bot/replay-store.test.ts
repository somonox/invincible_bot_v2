import { test } from "node:test";
import assert from "node:assert/strict";
import {
  mkdtemp,
  readdir,
  readFile,
  writeFile,
  utimes,
  rm,
  stat,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { ReplayStore } from "./replay-store";
test("replays keep old files above the former count cap and snapshot concurrent saves", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "invincible-replay-test-"));
  try {
    const store = new ReplayStore(dir);
    await Promise.all(
      Array.from({ length: 1001 }, async (_, i) => {
        const file = path.join(dir, `invincible-old-${i}.ttrm`);
        await writeFile(file, "keep");
        await utimes(file, new Date(0), new Date(0));
      }),
    );
    const replay = { version: 1, replay: { rounds: [[]] } };
    const first = store.save("../../room", replay);
    replay.version = 2;
    const files = await Promise.all([
      first,
      store.save("room", replay),
      store.save("room", replay, true),
    ]);
    await store.flush();
    await store.prepare();
    assert.equal((await readdir(dir)).length, 1004);
    assert.equal(new Set(files).size, 3);
    assert.equal(JSON.parse(await readFile(files[0], "utf8")).version, 1);
    assert.equal(JSON.parse(await readFile(files[1], "utf8")).version, 2);
    assert.ok(files[2].endsWith(".partial.ttrm"));
    for (const file of files) assert.equal(path.dirname(file), dir);
    assert.equal(
      await readFile(path.join(dir, "invincible-old-0.ttrm"), "utf8"),
      "keep",
    );
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
test("replays larger than the former per-file cap are saved in full", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "invincible-replay-test-"));
  try {
    const store = new ReplayStore(dir);
    const replay = { data: "x".repeat(33 * 1024 * 1024) };
    const file = await store.save("room", replay);
    await store.flush();
    assert.equal(
      (await stat(file)).size,
      Buffer.byteLength(JSON.stringify(replay)),
    );
    assert.deepEqual(await readdir(dir), [path.basename(file)]);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
