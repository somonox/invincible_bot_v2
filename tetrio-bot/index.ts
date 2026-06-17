import { Client } from "@haelp/teto";
import { BotWrapper, adapters } from "@haelp/teto/utils";
import path from "path";

// Monkey-patch BotWrapper.frames to space out consecutive movements/rotations with frame gaps
(BotWrapper as any).frames = (engine: any, keys: string[]) => {
  const round = (r: number) => Math.round(r * 10) / 10;
  let running = engine.frame + engine.subframe;
  return keys.flatMap((key) => {
    const mappedKey = key === "dasLeft" ? "moveLeft" : key === "dasRight" ? "moveRight" : key;
    const firstFrame = {
      type: "keydown",
      frame: Math.floor(running),
      data: {
        key: mappedKey,
        subframe: round(running - Math.floor(running)),
      },
    };
    running = round(running + 1.0); // 1 frame duration
    const secondFrame = {
      type: "keyup",
      frame: Math.floor(running),
      data: {
        key: mappedKey,
        subframe: round(running - Math.floor(running)),
      },
    };
    running = round(running + 1.0); // 1 frame gap before the next key
    return [firstFrame, secondFrame];
  });
};

// Monkey-patch BotWrapper.prototype.tick to prevent planning for the same piece multiple times before it locks
BotWrapper.prototype.tick = async function (this: any, engine: any, events: any, data?: any) {
  const fullData = {
    state: undefined,
    play: undefined,
    ...data
  };
  if (events.find((event: any) => event.type === "garbage")) {
    this.adapter.update(engine, fullData.state);
  }

  if (this.lastPieces === undefined) {
    this.lastPieces = engine.stats.pieces;
  }

  if (engine.frame >= this.nextFrame) {
    if (this.needsNewMove) {
      if (engine.stats.pieces > this.lastPieces) {
        this.nextFrame = BotWrapper.nextFrame(engine, this.config.pps);
        this.needsNewMove = false;
      }
    } else {
      const { keys } = await this.adapter.play(engine, fullData.play);
      const frames = BotWrapper.frames(engine, keys);
      this.needsNewMove = true;
      this.lastPieces = engine.stats.pieces;
      return frames;
    }
  }

  return [];
};


const masterClient = await Client.create({
  username: process.env.BOT_USERNAME!,
  password: process.env.BOT_PASSWORD!,
});

console.log(`[4wide-bot] Master client logged in as: ${masterClient.user.username} (ID: ${masterClient.user.id})`);
console.log("[4wide-bot] Master client waiting for room invites...");
masterClient.social.status("online", "menus");

// Friend back anyone who friends the bot
(masterClient as any).on("client.friended", async (friend: { id: string; name: string }) => {
  console.log(`[4wide-bot] Received friend request from ${friend.name} (${friend.id}). Friending back...`);
  await masterClient.social.friend(friend.id).catch((err) => {
    console.error(`[4wide-bot] Failed to friend back ${friend.name}:`, err);
  });
});


const MAX_WORKERS = 10;
let activeWorkersCount = 0;
let defaultPps = 2.0;

masterClient.on("social.invite", async (invite) => {
  const roomid = invite.roomid;
  const sender = invite.sender;

  if (activeWorkersCount >= MAX_WORKERS) {
    console.log(`[4wide-bot] Received invite to room ${roomid} from ${sender}, but worker pool is full (${activeWorkersCount}/${MAX_WORKERS}). Rejecting.`);
    await masterClient.social.dm(sender, `Sorry, all bot worker slots are currently full (${MAX_WORKERS}/${MAX_WORKERS}). Please try again later!`).catch((err) => {
      console.error("[4wide-bot] Failed to send DM to sender:", err);
    });
    return;
  }

  activeWorkersCount++;
  console.log(`[4wide-bot] [Worker Assigned] Joining room ${roomid}. Active workers: ${activeWorkersCount}/${MAX_WORKERS}`);

  runWorkerForRoom(roomid).finally(() => {
    activeWorkersCount--;
    console.log(`[4wide-bot] [Worker Released] Left room ${roomid}. Active workers: ${activeWorkersCount}/${MAX_WORKERS}`);
  });
});

async function runWorkerForRoom(roomid: string) {
  let client: Client | null = null;
  let wrapper: any = null;
  let roomPps = defaultPps;

  try {
    // 1. Create a new client connection for this room worker
    client = await Client.create({
      username: process.env.BOT_USERNAME!,
      password: process.env.BOT_PASSWORD!,
    });

    client.social.status("online", "lobby:X-PRIV");

    // 2. Join the room
    const room = await client.rooms.join(roomid);
    console.log(`[Worker-${roomid}] Joined room: ${room.name} (${room.id})`);
    client.social.status("online", "lobby:X-PRIV");

    const checkRoomConfigAndBracket = async () => {
      if (!client || !client.room) return;
      const currentRoom = client.room;
      const boardWidth = currentRoom.options?.boardwidth;
      const is4Wide = boardWidth === 4;
      const selfPlayer = currentRoom.players.find((p) => p._id === client!.user.id);
      const currentBracket = selfPlayer?.bracket;

      if (is4Wide) {
        if (currentBracket !== "player") {
          console.log(`[Worker-${roomid}] Room is 4-wide. Switching to player bracket.`);
          await currentRoom.switch("player").catch((err) => {
            console.error(`[Worker-${roomid}] Failed to switch to player bracket:`, err);
          });
        }
      } else {
        if (currentBracket !== "spectator") {
          console.log(`[Worker-${roomid}] Room board width is ${boardWidth || 4} (not 4-wide). Switching to spectator bracket.`);
          await currentRoom.switch("spectator").catch((err) => {
            console.error(`[Worker-${roomid}] Failed to switch to spectator bracket:`, err);
          });
          await currentRoom.chat("This bot only plays in 4-wide rooms. Spectating until room is set to 4-wide.").catch(() => {});
        }
      }
    };

    const checkRoomEmptyAndLeave = async () => {
      if (!client || !client.room) return;
      const otherPlayers = client.room.players.filter((p) => p._id !== client!.user.id);
      if (otherPlayers.length === 0) {
        console.log(`[Worker-${roomid}] No other players left in the room. Leaving room...`);
        await client.room.leave().catch((err) => {
          console.error(`[Worker-${roomid}] Failed to leave room:`, err);
        });
      }
    };

    // Check config immediately upon joining
    await checkRoomConfigAndBracket();
    await checkRoomEmptyAndLeave();

    const onRoomUpdate = async () => {
      await checkRoomConfigAndBracket();
      await checkRoomEmptyAndLeave();
    };

    const onRoomUpdateBracket = async (data: { uid: string }) => {
      if (client && data.uid === client.user.id) {
        await checkRoomConfigAndBracket();
      }
    };

    client.on("room.update", onRoomUpdate);
    client.on("room.update.bracket", onRoomUpdateBracket);
    client.on("room.player.remove", () => {
      setTimeout(checkRoomEmptyAndLeave, 0);
    });

    // Listen to room chat for command handling
    client.on("room.chat", async (chat) => {
      if (chat.system) return;
      if (!client || chat.user._id === client.user.id) return;

      const content = chat.content.trim();
      if (content.toLowerCase().startsWith("!pps")) {
        const parts = content.split(/\s+/);
        if (parts.length >= 2) {
          const ppsVal = parseFloat(parts[1]);
          if (!isNaN(ppsVal) && ppsVal >= 0.1 && ppsVal <= 10.0) {
            roomPps = ppsVal;
            if (wrapper) {
              wrapper.config.pps = ppsVal;
            }
            await client.room?.chat(`PPS updated to ${ppsVal}`).catch((err) => {
              console.error(`[Worker-${roomid}] Failed to send chat message:`, err);
            });
            console.log(`[Worker-${roomid}] PPS updated to ${ppsVal} by ${chat.user.username}`);
          } else {
            await client.room?.chat("Invalid PPS value. Please enter a number between 0.1 and 10.0.").catch(() => {});
          }
        } else {
          await client.room?.chat(`Current PPS is ${roomPps}`).catch(() => {});
        }
      }
    });

    // Persistent round start listener
    client.on("client.game.round.start", async ([tick, engine]) => {
      if (!client || client.room?.self?.bracket !== "player") {
        console.log(`[Worker-${roomid}] Game started, but bot is in spectator bracket. Ignoring.`);
        return;
      }
      console.log(`[Worker-${roomid}] Round started!`);
      client.social.status("online", "lobby_ig:X-PRIV");

      const adapter = new adapters.IO({
        path: path.join((import.meta as any).dir, "../target/release/triangle-adapter"),
        verbose: false,
      });

      wrapper = new BotWrapper(adapter, {
        pps: roomPps,
      });

      const initPromise = wrapper.init(engine);

      let isReady = false;
      initPromise.then(() => {
        isReady = true;
      });

      tick(async ({ engine, events }) => {
        if (!isReady || !wrapper) {
          return { keys: [] };
        }
        adapter.update(engine);
        return {
          keys: await wrapper.tick(engine, events),
        };
      });

      await client.wait("client.game.over");
      console.log(`[Worker-${roomid}] Round over.`);
      if (client) {
        client.social.status("online", "lobby:X-PRIV");
      }
      if (wrapper) {
        wrapper.stop();
        wrapper = null;
      }
    });

    // Wait until the bot leaves the room (or gets kicked)
    await client.wait("room.leave");
    console.log(`[Worker-${roomid}] Left room.`);

  } catch (err) {
    console.error(`[Worker-${roomid}] Error in room lifecycle:`, err);
  } finally {
    if (wrapper) {
      try {
        wrapper.stop();
      } catch {}
    }
    if (client) {
      await client.destroy().catch(() => {});
    }
  }
}
