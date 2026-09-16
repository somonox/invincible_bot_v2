import { test } from "node:test";
import assert from "node:assert/strict";
import { garbageContext } from "./garbage-context";

test("snapshots ready and future packets without assuming confirmation", () => {
  const engine = {
    frame: 100,
    garbageQueue: { options: { garbage: { speed: 30 }, cap: { max: 8 } },
      queue: [{ amount: 2, frame: 50, confirmed: true }, { amount: 7, frame: 90, confirmed: false }] },
    dynamic: { garbageCap: { get: () => 6.7 } },
  };
  const snapshot = garbageContext(engine, 2, 10);
  assert.deepEqual(snapshot, {packets: [{amount: 2, readyIn: 0}, {amount: 7, readyIn: 20}],
    framesPerPiece: 40, nextLockFrames: 10, cap: 6});
  engine.frame = 120;
  engine.garbageQueue.queue[1].amount = 3;
  assert.equal(snapshot.packets[1].amount, 7);
  assert.equal(garbageContext(engine, 4, 10).packets[1].readyIn, 0);
  assert.equal(garbageContext(engine, 4, 10).framesPerPiece, 25);
});

test("empty queues remain empty and the absolute garbage cap is respected", () => {
  const engine = { frame: 0, garbageQueue: {options: {garbage: {speed: 0}, cap: {max: 8}}, queue: []},
    dynamic: {garbageCap: {get: () => 20}} };
  assert.deepEqual(garbageContext(engine, 2).packets, []);
  assert.equal(garbageContext(engine, 2).cap, 8);
});
