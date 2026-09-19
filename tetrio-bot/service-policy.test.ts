import { test } from "node:test";
import assert from "node:assert/strict";
import {
  MAX_PPS,
  REQUIRED_SETTINGS,
  roomProblems,
  requiredChanges,
  parsePps,
  RoomPool,
  engineProblems,
} from "./service-policy";
import { SearchLimiter, withTimeout } from "./search-limiter";
import { installBotRuntime } from "./bot-runtime";

test("PPS parser has a hard five PPS cap and rejects partial numbers", () => {
  assert.equal(MAX_PPS, 5);
  for (const text of ["0.1", "2", "5", ".5"])
    assert.equal(parsePps(text), Number(text));
  for (const text of [
    "5.01",
    "10",
    "NaN",
    "Infinity",
    "3oops",
    "1e2",
    "-1",
    "0",
    "",
  ])
    assert.equal(parsePps(text), null);
});
test("required settings enforce SRS-X and retain supported spin variants", () => {
  assert.deepEqual(roomProblems(REQUIRED_SETTINGS), []);
  assert.ok(
    roomProblems({ ...REQUIRED_SETTINGS, kickset: "SRS+" }).includes("kickset"),
  );
  assert.ok(
    roomProblems({ ...REQUIRED_SETTINGS, display_hold: false }).includes(
      "display_hold",
    ),
  );
  assert.equal(
    roomProblems({ ...REQUIRED_SETTINGS, spinbonuses: "all-mini+" }).length,
    0,
  );
  assert.ok(
    requiredChanges({ kickset: "SRS+" }).some(
      (c) => c.index === "options.kickset" && c.value === "SRS-X",
    ),
  );
});
test("worker reservations prevent duplicate rooms, per-user abuse and stale releases", () => {
  const pool = new RoomPool(20, 2);
  const token = pool.reserve("room", "a")!;
  assert.equal(pool.reserve("room", "b"), null);
  pool.reserve("second", "a");
  assert.equal(pool.reserve("third", "a"), null);
  for (let i = 0; i < 18; i++) assert.ok(pool.reserve(`r${i}`, `u${i}`));
  assert.equal(pool.size, 20);
  assert.equal(pool.reserve("overflow", "z"), null);
  pool.release("room", Symbol());
  assert.equal(pool.size, 20);
  pool.release("room", token);
  assert.equal(pool.size, 19);
});
test("search admission is FIFO and abort removes a waiting job", async () => {
  const slots = new SearchLimiter(1, 3);
  const controller = new AbortController();
  let release!: () => void;
  const order: number[] = [];
  const first = slots.run(async () => {
    order.push(1);
    await new Promise<void>((r) => (release = r));
  }, new AbortController().signal);
  const canceled = slots.run(async () => {
    order.push(2);
  }, controller.signal);
  const third = slots.run(async () => {
    order.push(3);
  }, new AbortController().signal);
  controller.abort();
  await assert.rejects(canceled);
  await Promise.resolve();
  release();
  await Promise.all([first, third]);
  assert.deepEqual(order, [1, 3]);
});
test("adapter timeout releases admission so another room can search", async () => {
  const slots = new SearchLimiter(1, 2);
  let killed = false;
  const stuck = slots.run(
    () =>
      withTimeout(new Promise(() => {}), 10, "adapter", () => {
        killed = true;
      }),
    new AbortController().signal,
  );
  const next = slots.run(async () => 42, new AbortController().signal);
  await assert.rejects(stuck, /timed out/);
  assert.equal(await next, 42);
  assert.ok(killed);
});
test("runtime clamps PPS even if wrapper config bypasses chat and sends fresh packet data", async () => {
  class Wrapper {
    static frames: any;
    static nextFrame(_e: any, p: number) {
      assert.ok(p <= 5);
      return 0;
    }
    config = { pps: 99 };
    adapter: any;
    nextFrame = 0;
    lastPieces = 0;
    needsNewMove = false;
    roundSignal = new AbortController().signal;
    abortRound = () => {};
    declare tick: any;
  }
  installBotRuntime(Wrapper, new SearchLimiter(1, 2));
  const wrapper = new Wrapper();
  let captured: any;
  wrapper.adapter = {
    update: (_e: any, data: any) => {
      captured = data;
    },
    play: async () => ({ keys: ["hardDrop"] }),
  };
  const engine = {
    frame: 1,
    subframe: 0,
    stats: { pieces: 0 },
    garbageQueue: {
      queue: [{ amount: 4, frame: 1 }],
      options: { garbage: { speed: 20 }, cap: { max: 8 } },
    },
    dynamic: { garbageCap: { get: () => 8 } },
  };
  const frames = await wrapper.tick(engine, []);
  assert.equal(wrapper.config.pps, 5);
  assert.equal(captured.garbageContext.packets[0].amount, 4);
  assert.equal(frames.length, 2);
  const firstDrop = frames[0].frame + frames[0].data.subframe;
  engine.stats.pieces++;
  engine.frame++;
  await wrapper.tick(engine, []); // Observe the previous lock.
  const next = await wrapper.tick(engine, []); // Catch-up scheduling must not burst.
  assert.ok(next[0].frame + next[0].data.subframe - firstDrop >= 12);
});
