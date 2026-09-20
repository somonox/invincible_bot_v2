import { test } from "node:test";
import assert from "node:assert/strict";
import {
  MAX_PPS,
  REQUIRED_SETTINGS,
  roomProblems,
  requiredChanges,
  parsePps,
  RoomPool,
} from "./service-policy";
import { withTimeout } from "./timeout";
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
  assert.equal(REQUIRED_SETTINGS.boardheight, 26);
  assert.ok(
    roomProblems({ ...REQUIRED_SETTINGS, boardheight: 20 }).includes(
      "boardheight",
    ),
  );
  assert.ok(
    roomProblems({ ...REQUIRED_SETTINGS, kickset: "SRS+" }).includes("kickset"),
  );
  assert.deepEqual(Object.keys(REQUIRED_SETTINGS).sort(), [
    "boardheight",
    "boardwidth",
    "kickset",
  ]);
  assert.deepEqual(
    requiredChanges({
      ...REQUIRED_SETTINGS,
      allclear_garbage: 5,
      spinbonuses: "handheld",
      combotable: "none",
      garbageblocking: "none",
      room_handling: true,
    }),
    [],
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
  const pool = new RoomPool();
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
test("an adapter timeout still invokes owned process cleanup", async () => {
  let killed = false;
  await assert.rejects(
    withTimeout(new Promise(() => {}), 10, "adapter", () => {
      killed = true;
    }),
    /timed out/,
  );
  assert.ok(killed);
});
test("workers start searches independently without a shared three-search gate", async () => {
  class Wrapper {
    static frames: any;
    static nextFrame() {
      return 0;
    }
    config = { pps: 5 };
    nextFrame = 0;
    adapter: any;
    roundSignal = new AbortController().signal;
    abortRound = () => {};
    declare tick: any;
  }
  installBotRuntime(Wrapper);
  let started = 0;
  const releases: ((value: any) => void)[] = [];
  const jobs = Array.from({ length: 5 }, () => {
    const w = new Wrapper();
    w.adapter = {
      update() {},
      play: () => {
        started++;
        return new Promise((r) => releases.push(r));
      },
    };
    return w.tick(
      {
        frame: 1,
        subframe: 0,
        stats: { pieces: 0 },
        garbageQueue: {
          queue: [],
          options: { garbage: { speed: 20 }, cap: { max: 8 } },
        },
        dynamic: { garbageCap: { get: () => 8 } },
      },
      [],
    );
  });
  try {
    assert.equal(started, 5);
  } finally {
    for (const release of releases) release({ keys: ["hardDrop"] });
    await Promise.all(jobs);
  }
});

test("runtime clamps PPS even if wrapper config bypasses chat and sends fresh packet data", async () => {
  class Wrapper {
    static frames: any;
    static nextFrame(_e: any, p: number) {
      assert.ok(p <= 5);
      return 0;
    }
    config = { pps: 99 };
    expertMode = false;
    adapter: any;
    nextFrame = 0;
    lastPieces = 0;
    needsNewMove = false;
    roundSignal = new AbortController().signal;
    abortRound = () => {};
    declare tick: any;
  }
  installBotRuntime(Wrapper);
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
  assert.equal(captured.expertMode, false);
  assert.equal(captured.garbageContext.packets[0].amount, 4);
  assert.equal(frames.length, 2);
  const firstDrop = frames[0].frame + frames[0].data.subframe;
  engine.stats.pieces++;
  engine.frame++;
  await wrapper.tick(engine, []); // Observe the previous lock.
  wrapper.expertMode = true;
  const next = await wrapper.tick(engine, []); // Catch-up scheduling must not burst.
  assert.equal(captured.expertMode, true);
  assert.ok(next[0].frame + next[0].data.subframe - firstDrop >= 12);
  engine.stats.pieces++;
  engine.frame++;
  await wrapper.tick(engine, []);
  wrapper.expertMode = false;
  await wrapper.tick(engine, [], { state: { expertMode: true } });
  assert.equal(captured.expertMode, false);
});
