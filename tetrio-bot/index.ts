import { Client } from "@haelp/teto";
import { BotWrapper } from "@haelp/teto/utils";
import path from "path";
import { existsSync } from "node:fs";
import { RoomPool, envInt } from "./service-policy";
import { ReplayStore } from "./replay-store";
import { installBotRuntime } from "./bot-runtime";
import { runRoomWorker } from "./room-worker";

const maxWorkers = envInt(process.env, "BOT_MAX_WORKERS", 20, 1, 32);
const pool = new RoomPool(maxWorkers);
installBotRuntime(BotWrapper);
const replays = new ReplayStore(
  path.resolve((import.meta as any).dir, "../replays"),
  {
    maxFiles: envInt(process.env, "BOT_REPLAY_MAX_FILES", 1000, 1, 10000),
    maxBytes:
      envInt(process.env, "BOT_REPLAY_MAX_MB", 2048, 1, 16384) * 1024 * 1024,
    maxAgeMs: envInt(process.env, "BOT_REPLAY_DAYS", 14, 1, 365) * 86400000,
    maxFileBytes:
      envInt(process.env, "BOT_REPLAY_FILE_MB", 32, 1, 128) * 1024 * 1024,
  },
);
await replays.maintain();
const maintenance = setInterval(() => {
  void replays
    .maintain()
    .catch((error) => console.error("Replay cleanup:", error.message));
}, 3600000);
maintenance.unref();
const shutdown = new AbortController();
const workers = new Set<Promise<void>>();

const adapterPath = process.env.BOT_ADAPTER_PATH
  ? path.resolve(process.env.BOT_ADAPTER_PATH)
  : path.join(
      (import.meta as any).dir,
      "../target/release/triangle-adapter" +
        (process.platform === "win32" ? ".exe" : ""),
    );
for (const name of ["BOT_USERNAME", "BOT_PASSWORD"]) {
  if (!process.env[name])
    throw new Error(
      `Missing ${name}. Set it in tetrio-bot/.env or the service environment.`,
    );
}
if (!existsSync(adapterPath)) {
  throw new Error(
    `Adapter binary not found: ${adapterPath}. Run ./up.sh build first.`,
  );
}

const masterClient = await Client.create({
  username: process.env.BOT_USERNAME!,
  password: process.env.BOT_PASSWORD!,
});

console.log(
  `[4wide-bot] Master client logged in as: ${masterClient.user.username} (ID: ${masterClient.user.id})`,
);
console.log(
  `[4wide-bot] Waiting for invites: ${maxWorkers} rooms, independent worker searches, PPS cap 5.`,
);
masterClient.social.status("online", "menus");

// Friend back anyone who friends the bot
(masterClient as any).on(
  "client.friended",
  async (friend: { id: string; name: string }) => {
    console.log(
      `[4wide-bot] Received friend request event for ${friend.name} (${friend.id})`,
    );

    // Verify if they are already in friends list to avoid redundant API calls
    const isAlreadyFriend = masterClient.social.friends.some(
      (f) => f.id === friend.id,
    );
    if (isAlreadyFriend) {
      console.log(
        `[4wide-bot] ${friend.name} is already in the friends list. Skipping friend back.`,
      );
      return;
    }

    console.log(`[4wide-bot] Attempting to friend back ${friend.name}...`);
    try {
      const result = await masterClient.social.friend(friend.id);
      console.log(
        `[4wide-bot] Friend back result for ${friend.name}: ${result}`,
      );
    } catch (err: any) {
      console.error(
        `[4wide-bot] Error friending back ${friend.name}:`,
        err.message || err,
      );
      if (err.stack) {
        console.error(err.stack);
      }
    }
  },
);

masterClient.on("social.invite", (invite) => {
  if (stopping) return;
  const roomid = invite.roomid.trim().toLowerCase();
  const token = pool.reserve(roomid, invite.sender);
  if (!token) {
    console.log(
      `[4wide-bot] Invite skipped: duplicate room, user limit or full pool (${pool.size}/${maxWorkers}).`,
    );
    return;
  }
  const worker = runRoomWorker(roomid, invite.sender, {
    adapterPath,
    defaultPps: 2,
    replays,
    signal: shutdown.signal,
  }).finally(() => {
    pool.release(roomid, token);
    workers.delete(worker);
  });
  workers.add(worker);
  console.log(`[4wide-bot] Room assigned (${pool.size}/${maxWorkers}).`);
});
let stopping = false;
const stop = async () => {
  if (stopping) return;
  stopping = true;
  clearInterval(maintenance);
  shutdown.abort();
  await Promise.allSettled([...workers]);
  await replays.maintain().catch(() => {});
  await masterClient.destroy().catch(() => {});
};
process.once("SIGTERM", () => {
  void stop();
});
process.once("SIGINT", () => {
  void stop();
});
