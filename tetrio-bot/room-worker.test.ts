import { test } from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { ReplayManager } from "@haelp/teto/classes";
import { runRoomWorker } from "./room-worker";
import { REQUIRED_SETTINGS } from "./service-policy";
const pause = () => new Promise((r) => setTimeout(r, 1));
async function until(fn: () => boolean) {
  for (let i = 0; i < 200; i++) {
    if (fn()) return;
    await pause();
  }
  assert.fail("condition did not become true");
}
function fixture(
  host = true,
  settings: any = REQUIRED_SETTINGS,
  overrides: any = {},
) {
  const client: any = new EventEmitter();
  client.user = { id: "bot" };
  client.destroyed = false;
  const calls: any = {
    updates: [],
    chats: [],
    saved: [],
    wrappers: [],
    scopes: [],
    transfers: [],
  };
  const room: any = {
    id: "test",
    owner: host ? "bot" : "host",
    options: { ...settings },
    state: "lobby",
    players: [
      { _id: "bot", bracket: "spectator" },
      { _id: "human", bracket: "player" },
    ],
    replay: null,
  };
  Object.defineProperty(room, "self", { get: () => room.players[0] });
  Object.defineProperty(room, "isHost", { get: () => room.owner === "bot" });
  room.switch = async (bracket: string) => {
    room.self.bracket = bracket;
    client.emit("room.update.bracket", { uid: "bot" });
  };
  room.update = async (...changes: any[]) => {
    calls.updates.push(changes);
    for (const c of changes)
      room.options[c.index.replace("options.", "")] = c.value;
    client.emit("room.update", {});
  };
  room.transferHost = async (id: string) => {
    calls.transfers.push(id);
    room.owner = id;
    client.emit("room.update.host", id);
  };
  room.chat = async (message: string) => {
    calls.chats.push(message);
  };
  client.rooms = {
    join: async () => {
      client.room = room;
      return room;
    },
  };
  client.destroy = async () => {
    client.destroyed = true;
  };
  client.on("game.scope.start", (id: any) => calls.scopes.push(id));
  const shutdown = new AbortController();
  const promise = runRoomWorker("test", "human", {
    adapterPath: "unused",
    defaultPps: 2,
    signal: shutdown.signal,
    replays: {
      save: async (_room: any, data: any, partial: any) => {
        calls.saved.push({ data, partial });
        return "test.ttrm";
      },
    } as any,
    createClient: async () => client,
    createAdapter: () => ({ stop() {} }),
    createWrapper: () => {
      const wrapper: any = {
        config: { pps: 2 },
        stops: 0,
        init: async () => {},
        stop() {
          this.stops++;
        },
        tick: async () => [
          { type: "keydown", frame: 1, data: { key: "hardDrop" } },
        ],
      };
      calls.wrappers.push(wrapper);
      return wrapper;
    },
    ...overrides,
  });
  return { client, room, calls, shutdown, promise };
}
const engine = () => ({
  kickTableName: "SRS-X",
  board: { width: 4, height: 20 },
  misc: { allowed: { spin180: true, hold: true, hardDrop: true } },
  gameOptions: {
    comboTable: "multiplier",
    garbageBlocking: "combo blocking",
    spinBonuses: "all",
  },
  pc: { garbage: 10 },
  handling: { arr: 0, sdf: 41 },
});
function chat(f: any, id: string, content: string) {
  f.client.emit("room.chat", { system: false, user: { _id: id }, content });
}
test("settings change only through authorized setup with bot host permission", async () => {
  for (const host of [true, false]) {
    const f = fixture(host, { ...REQUIRED_SETTINGS, kickset: "SRS+" });
    await until(() => f.calls.chats.length > 0);
    assert.equal(f.room.self.bracket, "spectator");
    assert.equal(f.calls.updates.length, 0);
    if (!host) {
      chat(f, "human", "!setup");
      await pause();
      assert.equal(f.calls.updates.length, 0);
      f.room.owner = "bot";
      f.client.emit("room.update.host", "bot");
      await pause();
      assert.equal(f.calls.updates.length, 0); // Host transfer alone must not edit settings.
    }
    chat(f, "stranger", "!setup");
    await pause();
    assert.equal(f.calls.updates.length, 0);
    f.room.state = "ingame";
    chat(f, "human", "!setup");
    await pause();
    assert.equal(f.calls.updates.length, 0);
    f.room.state = "lobby";
    chat(f, "human", "!setup extra");
    await pause();
    assert.equal(f.calls.updates.length, 0);
    chat(f, "human", "!setup");
    await until(() => f.room.self.bracket === "player");
    assert.equal(f.room.options.kickset, "SRS-X");
    assert.equal(f.calls.updates.length, 1);
    await until(() => f.calls.transfers.length === 1);
    assert.equal(f.room.owner, "human");
    f.room.options.kickset = "SRS+";
    f.client.emit("room.update", {});
    await until(() => f.room.self.bracket === "spectator");
    assert.equal(f.calls.updates.length, 1);
    f.shutdown.abort();
    await f.promise;
  }
});
test("remaining spectators keep the room occupied; last departure stops the round and saves partial replay", async () => {
  const f = fixture();
  await until(() => f.calls.chats.length > 0);
  f.client.emit("client.game.round.start", [() => {}, engine()]);
  await pause();
  f.room.replay = { export: () => ({ version: 1, replay: { rounds: [[]] } }) };
  f.room.players.push({ _id: "spectator", bracket: "spectator" });
  f.room.players = f.room.players.filter((p: any) => p._id !== "human");
  f.client.emit("room.player.remove", "human");
  await pause();
  assert.equal(f.client.destroyed, false);
  f.room.players = f.room.players.filter((p: any) => p._id !== "spectator");
  f.client.emit("room.player.remove", "spectator");
  assert.equal(f.calls.wrappers[0].stops, 1);
  await until(() => f.client.destroyed);
  await f.promise;
  assert.equal(f.calls.saved[0].partial, true);
});

test("full round engine is checked even when lobby options appear compatible", async () => {
  const f = fixture();
  await until(() => f.room.self.bracket === "player");
  f.client.emit("client.game.round.start", [
    () => {},
    { ...engine(), kickTableName: "SRS+" },
  ]);
  await until(() => f.room.self.bracket === "spectator");
  assert.equal(f.calls.wrappers.length, 0);
  f.shutdown.abort();
  await f.promise;
});
test("PPS commands are strict and scoped to host/inviter; round cleanup and native replay save", async () => {
  const f = fixture();
  await until(() => f.room.self.bracket === "player");
  let tick: any;
  f.client.emit("client.game.round.start", [
    (fn: any) => (tick = fn),
    engine(),
  ]);
  await until(() => f.calls.wrappers.length === 1);
  await pause();
  const chat = (id: string, text: string) =>
    f.client.emit("room.chat", {
      system: false,
      user: { _id: id },
      content: text,
    });
  chat("stranger", "!pps 5");
  chat("human", "!pps 10");
  chat("human", "!pps 4bad");
  assert.equal(f.calls.wrappers[0].config.pps, 2);
  chat("human", "!pps 5");
  await until(() => f.calls.wrappers[0].config.pps === 5);
  assert.equal((await tick({ engine: engine(), events: [] })).keys.length, 1);
  f.room.replay = { export: () => ({ version: 1, replay: { rounds: [[]] } }) };
  f.client.emit("game.ready", {
    isNew: true,
    players: [
      { userid: "bot", gameid: 1 },
      { userid: "human", gameid: 2 },
    ],
  });
  assert.deepEqual(f.calls.scopes, [2]);
  f.client.emit("client.game.over", { reason: "finish" });
  assert.equal(f.calls.saved.length, 0);
  assert.equal(f.calls.wrappers[0].stops, 1);
  f.client.emit("client.game.end", {});
  await until(() => f.calls.saved.length === 1);
  assert.equal(f.calls.saved[0].partial, false);
  f.shutdown.abort();
  await f.promise;
  assert.equal(f.calls.saved.length, 1);
});
test("room leave before join settles cannot strand a worker slot", async () => {
  const client: any = new EventEmitter();
  client.destroy = async () => {};
  client.rooms = {
    join: async () => {
      client.emit("room.leave");
      return {};
    },
  };
  await runRoomWorker("test", "human", {
    adapterPath: "unused",
    defaultPps: 2,
    signal: new AbortController().signal,
    replays: { save: async () => "" } as any,
    createClient: async () => client,
  });
});

test("late initialization and search failures cannot stop the next round", async () => {
  for (const stage of ["init", "tick"]) {
    let reject!: (error: Error) => void;
    const wrappers: any[] = [];
    const f = fixture(true, REQUIRED_SETTINGS, {
      createWrapper: () => {
        const pending = new Promise((_resolve, r) => (reject = r));
        // Second round succeeds; the first fails only after it has been replaced.
        const w: any = {
          config: { pps: 2 },
          stop() {},
          init: () =>
            wrappers.length === 1 && stage === "init"
              ? pending
              : Promise.resolve(),
          tick: () => pending,
        };
        wrappers.push(w);
        return w;
      },
    });
    await until(() => f.room.self.bracket === "player");
    let tick: any;
    f.client.emit("client.game.round.start", [
      (fn: any) => (tick = fn),
      engine(),
    ]);
    await pause();
    const rejectOld = reject;
    const pendingTick =
      stage === "tick"
        ? tick({ engine: engine(), events: [] })
        : Promise.resolve();
    f.client.emit("client.game.round.start", [() => {}, engine()]);
    await pause();
    rejectOld(new Error("old round failed"));
    await pendingTick;
    await pause();
    assert.equal(f.room.self.bracket, "player");
    assert.equal(wrappers.length, 2);
    f.shutdown.abort();
    await f.promise;
  }
});
test("spawn errors before adapter info are contained without evicting occupied rooms", async () => {
  let adapter: any;
  const f = fixture(true, REQUIRED_SETTINGS, {
    createAdapter: () => (adapter = { stop() {} }),
    createWrapper: () => ({
      config: { pps: 2 },
      stop() {},
      init: async () => {
        const child: any = new EventEmitter();
        child.stdin = new EventEmitter();
        adapter.process = child;
        child.emit("error", new Error("spawn failed"));
        throw new Error("no info");
      },
    }),
  });
  await until(() => f.room.self.bracket === "player");
  f.client.emit("client.game.round.start", [() => {}, engine()]);
  await until(() => f.room.self.bracket === "spectator");
  assert.equal(f.client.destroyed, false);
  f.shutdown.abort();
  await f.promise;
  assert.ok(f.client.destroyed);
});

test("setup returns host to the previous owner even when they are not the inviter", async () => {
  const f = fixture(false, { ...REQUIRED_SETTINGS, kickset: "SRS+" });
  f.room.players.push({ _id: "host", bracket: "spectator" });
  await until(() => f.calls.chats.length > 0);
  f.room.owner = "bot";
  f.client.emit("room.update.host", "bot");
  await pause();
  assert.equal(f.calls.updates.length, 0);
  assert.equal(f.calls.transfers.length, 0);
  chat(f, "host", "!setup");
  await until(() => f.calls.transfers.length === 1);
  assert.equal(f.room.options.kickset, "SRS-X");
  assert.equal(f.room.owner, "host");
  f.shutdown.abort();
  await f.promise;
});
test("failed setup keeps host and allows retry without closing the worker", async () => {
  const f = fixture(true, { ...REQUIRED_SETTINGS, kickset: "SRS+" });
  await until(() => f.calls.chats.length > 0);
  const update = f.room.update;
  f.room.update = async () => {
    throw new Error("settings rejected");
  };
  chat(f, "human", "!setup");
  await pause();
  assert.equal(f.calls.transfers.length, 0);
  assert.equal(f.client.destroyed, false);
  f.room.update = update;
  chat(f, "human", "!setup");
  await until(() => f.calls.transfers.length === 1);
  f.shutdown.abort();
  await f.promise;
});
test("large replay streams stay attached through match end", async () => {
  const f = fixture();
  await until(() => f.calls.chats.length > 0);
  const manager = { export: () => ({ version: 1, replay: { rounds: [[]] } }) };
  f.room.replay = manager;
  f.client.emit("game.replay", {
    frames: [{ data: "x".repeat(33 * 1024 * 1024) }],
  });
  assert.equal(f.room.replay, manager);
  assert.equal(f.calls.saved.length, 0);
  f.client.emit("client.game.end", {});
  await until(() => f.calls.saved.length === 1);
  assert.equal(f.calls.saved[0].partial, false);
  f.shutdown.abort();
  await f.promise;
});

test("real SDK ReplayManager records outgoing self frames and incoming opponent frames in separate rounds", async () => {
  const f = fixture();
  await until(() => f.calls.chats.length > 0);
  const players = (offset: number) => [
    {
      userid: "bot",
      gameid: 1 + offset,
      naturalorder: 0,
      options: { username: "bot" },
    },
    {
      userid: "human",
      gameid: 2 + offset,
      naturalorder: 1,
      options: { username: "human" },
    },
  ];
  const manager = new ReplayManager(
    players(0) as any,
    [
      { _id: "bot", username: "bot" },
      { _id: "human", username: "human" },
    ] as any,
  );
  f.room.replay = manager;
  // The SDK's Room subscribes only to incoming game.replay; model that direction.
  f.client.on("game.replay", (data: any) => manager.pipe(data));
  for (const offset of [0, 10]) {
    manager.addRound(players(offset) as any);
    f.client.emit("game.ready", {
      isNew: offset === 0,
      players: players(offset),
    });
    const ownFrames = [
      { type: "start", frame: 0, data: {} },
      { type: "full", frame: 0, data: {} },
      { type: "keydown", frame: 5, data: { key: "hardDrop", subframe: 0 } },
      { type: "keyup", frame: 6, data: { key: "hardDrop", subframe: 0 } },
      { type: "ige", frame: 7, data: { id: 42 } },
    ];
    f.client.emit("client.ribbon.send", {
      command: "game.replay",
      data: { gameid: 1 + offset, provisioned: 12, frames: ownFrames },
    });
    f.client.emit("game.replay", {
      gameid: 2 + offset,
      frames: [
        { type: "keydown", frame: 8, data: { key: "hardDrop", subframe: 0 } },
      ],
    });
    f.client.emit("client.ribbon.send", {
      command: "room.chat",
      data: { content: "not gameplay" },
    });
    f.client.emit("client.ribbon.send", {
      command: "game.replay",
      data: { gameid: 999, frames: ownFrames },
    });
    const round = manager.export().replay.rounds.at(-1)!;
    assert.deepEqual(round[0].replay.events, ownFrames);
    assert.equal(round[0].replay.frames, 7);
    assert.equal(round[1].replay.events.length, 1);
  }
  manager.end({ self: "bot" });
  f.client.emit("client.game.end", {});
  await until(() => f.calls.saved.length === 1);
  const saved = f.calls.saved[0].data;
  assert.equal(saved.replay.rounds.length, 2);
  for (const round of saved.replay.rounds) {
    assert.equal(
      round[0].replay.events.filter((e: any) => e.type === "keydown").length,
      1,
    );
    assert.equal(
      round[1].replay.events.filter((e: any) => e.type === "keydown").length,
      1,
    );
  }
  f.shutdown.abort();
  await f.promise;
});
