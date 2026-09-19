import { test } from "node:test";
import assert from "node:assert/strict";
import {
  mkdtemp,
  readdir,
  readFile,
  writeFile,
  utimes,
  rm,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { ReplayStore } from "./replay-store";
test("replays retain native JSON, unique names and bounded files under concurrent saves", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "invincible-replay-test-"));
  try {
    const store = new ReplayStore(dir, {
      maxFiles: 2,
      maxBytes: 10000,
      maxAgeMs: 86400000,
      maxFileBytes: 1000,
    });
    const replay = { version: 1, replay: { rounds: [[]] } };
    await writeFile(path.join(dir, "unmanaged.ttrm"), "keep");
    await Promise.all([
      store.save("../../room", replay),
      store.save("room", replay),
      store.save("room", replay, true),
    ]);
    const files = (await readdir(dir)).filter((f) =>
      f.startsWith("invincible-"),
    );
    assert.equal(files.length, 2);
    for (const file of files) {
      assert.deepEqual(
        JSON.parse(await readFile(path.join(dir, file), "utf8")),
        replay,
      );
      assert.ok(!file.includes("/"));
    }
    assert.equal(
      await readFile(path.join(dir, "unmanaged.ttrm"), "utf8"),
      "keep",
    );
    await assert.rejects(store.save("room", { large: "x".repeat(2000) }));
    for (const file of files)
      await utimes(path.join(dir, file), new Date(0), new Date(0));
    await store.maintain();
    assert.deepEqual(await readdir(dir), ["unmanaged.ttrm"]);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
test("replay byte quota evicts old managed files before accepting the next", async () => {
  const dir = await mkdtemp(path.join(tmpdir(), "invincible-replay-test-"));
  try {
    const store = new ReplayStore(dir, {
      maxFiles: 20,
      maxBytes: 100,
      maxAgeMs: 86400000,
      maxFileBytes: 100,
    });
    await store.save("room", { data: "x".repeat(60) });
    await store.save("room", { data: "y".repeat(60) });
    assert.equal((await readdir(dir)).length, 1);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
