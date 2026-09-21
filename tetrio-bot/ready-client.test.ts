import { test } from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { ReadyClient } from "./ready-client";

class FakeClient extends EventEmitter {
  disconnected = false;
  destroyed = 0;
  async destroy() {
    this.destroyed++;
  }
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
async function until(condition: () => boolean) {
  for (let i = 0; i < 200; i++) {
    if (condition()) return;
    await new Promise((resolve) => setTimeout(resolve, 2));
  }
  assert.fail("condition did not become true");
}
function fixture(connectMs = 1000) {
  const connections: ReturnType<typeof deferred<FakeClient>>[] = [];
  const messages: string[] = [];
  const pool = new ReadyClient(
    () => {
      const next = deferred<FakeClient>();
      connections.push(next);
      return next.promise;
    },
    (message) => messages.push(message),
    connectMs,
    5,
  );
  return { pool, connections, messages };
}

test("ready connection is immediate while its replacement is still connecting", async () => {
  const { pool, connections, messages } = fixture();
  try {
    await until(() => connections.length === 1);
    const client = new FakeClient();
    connections[0].resolve(client);
    await until(() => messages.some((m) => m.includes("ready in")));
    assert.equal(await pool.take(), client);
    await until(() => connections.length === 2);
    assert.equal(client.listenerCount("client.dead"), 0);
    const spare = new FakeClient();
    connections[1].resolve(spare);
    await until(() => spare.listenerCount("client.dead") === 1);
    await pool.close();
    assert.equal(client.destroyed, 0);
    assert.equal(spare.destroyed, 1);
  } finally {
    await pool.close();
  }
});

test("simultaneous invitations claim distinct connections and retain one spare", async () => {
  const { pool, connections } = fixture();
  try {
    const first = pool.take();
    const second = pool.take();
    await until(() => connections.length === 3);
    const clients = [new FakeClient(), new FakeClient(), new FakeClient()];
    connections.forEach((pending, i) => pending.resolve(clients[i]));
    assert.equal(await first, clients[0]);
    assert.equal(await second, clients[1]);
    await pool.close();
    assert.deepEqual(
      clients.map((c) => c.destroyed),
      [0, 0, 1],
    );
  } finally {
    await pool.close();
  }
});

test("dead spare is discarded and replaced after backoff", async () => {
  const { pool, connections, messages } = fixture();
  try {
    await until(() => connections.length === 1);
    const dead = new FakeClient();
    connections[0].resolve(dead);
    await until(() => messages.length > 0);
    dead.disconnected = true;
    dead.emit("client.dead");
    await until(() => connections.length === 2);
    assert.equal(dead.destroyed, 1);
    const live = new FakeClient();
    connections[1].resolve(live);
    assert.equal(await pool.take(), live);
  } finally {
    await pool.close();
  }
});

test("failed and timed-out spares retry; a late connection is destroyed", async () => {
  const { pool, connections } = fixture(30);
  try {
    await until(() => connections.length === 1);
    connections[0].reject(new Error("offline"));
    await until(() => connections.length === 2);
    await until(() => connections.length === 3);
    const late = new FakeClient();
    connections[1].resolve(late);
    await until(() => late.destroyed === 1);
    const fresh = new FakeClient();
    connections[2].resolve(fresh);
    assert.equal(await pool.take(), fresh);
  } finally {
    await pool.close();
  }
});

test("shutdown disposes a pending spare without starting another connection", async () => {
  const { pool, connections } = fixture();
  await until(() => connections.length === 1);
  await pool.close();
  const late = new FakeClient();
  connections[0].resolve(late);
  await until(() => late.destroyed === 1);
  await assert.rejects(pool.take(), /closed/);
  assert.equal(connections.length, 1);
});

test("shutdown cleans up a claimed connection that has not finished connecting", async () => {
  const { pool, connections } = fixture();
  const taken = pool.take();
  const rejection = assert.rejects(taken, /closed/);
  await until(() => connections.length === 2);
  await pool.close();
  const clients = [new FakeClient(), new FakeClient()];
  connections.forEach((pending, i) => pending.resolve(clients[i]));
  await rejection;
  await until(() => clients.every((c) => c.destroyed === 1));
});
