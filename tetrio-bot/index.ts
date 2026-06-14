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


const client = await Client.create({
  username: process.env.BOT_USERNAME!,
  password: process.env.BOT_PASSWORD!,
});

console.log(`[4wide-bot] Logged in as: ${client.user.username} (ID: ${client.user.id})`);
console.log("[4wide-bot] Waiting for room invite...");
// Set initial online status
client.social.status("online", "menus");

// Persistent round start listener
client.on("client.game.round.start", async ([tick, engine]) => {
  if (client.room?.self?.bracket !== "player") {
    console.log("[4wide-bot] Game started, but bot is in spectator bracket. Ignoring.");
    return;
  }
  console.log("[4wide-bot] Round started!");
  client.social.status("online", "lobby_ig:X-PRIV");

  const adapter = new adapters.IO({
    path: path.join((import.meta as any).dir, "../target/release/triangle-adapter"),
    verbose: false,
  });

  const wrapper = new BotWrapper(adapter, {
    pps: 2,
  });

  const initPromise = wrapper.init(engine);

  let isReady = false;
  initPromise.then(() => {
    isReady = true;
  });

  tick(async ({ engine, events }) => {
    if (!isReady) {
      return { keys: [] };
    }
    adapter.update(engine);
    return {
      keys: await wrapper.tick(engine, events),
    };
  });

  await client.wait("client.game.over");
  console.log("[4wide-bot] Round over.");
  client.social.status("online", "lobby:X-PRIV");
  wrapper.stop();
});

// Main loop for joining rooms
while (true) {
  try {
    const { roomid } = await client.wait("social.invite");
    console.log(`[4wide-bot] Invited to room ${roomid}, joining...`);

    const room = await client.rooms.join(roomid);
    console.log(`[4wide-bot] Joined room: ${room.name} (${room.id})`);
    client.social.status("online", "lobby:X-PRIV");

    const checkRoomConfigAndBracket = async () => {
      if (!client.room) return;
      const currentRoom = client.room;
      const boardWidth = currentRoom.options?.boardwidth;
      const is4Wide = boardWidth === 4;
      const selfPlayer = currentRoom.players.find((p) => p._id === client.user.id);
      const currentBracket = selfPlayer?.bracket;

      if (is4Wide) {
        if (currentBracket !== "player") {
          console.log("[4wide-bot] Room is 4-wide. Switching to player bracket.");
          await currentRoom.switch("player").catch((err) => {
            console.error("[4wide-bot] Failed to switch to player bracket:", err);
          });
        }
      } else {
        if (currentBracket !== "spectator") {
          console.log(`[4wide-bot] Room board width is ${boardWidth || 4} (not 4-wide). Switching to spectator bracket.`);
          await currentRoom.switch("spectator").catch((err) => {
            console.error("[4wide-bot] Failed to switch to spectator bracket:", err);
          });
          await currentRoom.chat("This bot only plays in 4-wide rooms. Spectating until room is set to 4-wide.").catch(() => {});
        }
      }
    };

    // Check config immediately upon joining
    await checkRoomConfigAndBracket();

    // Listen to updates
    const onRoomUpdate = async () => {
      await checkRoomConfigAndBracket();
    };

    const onRoomUpdateBracket = async (data: { uid: string }) => {
      if (data.uid === client.user.id) {
        await checkRoomConfigAndBracket();
      }
    };

    client.on("room.update", onRoomUpdate);
    client.on("room.update.bracket", onRoomUpdateBracket);

    // Wait until the bot leaves the room (or gets kicked)
    await client.wait("room.leave");
    console.log("[4wide-bot] Left room. Waiting for next invite...");
    client.social.status("online", "menus");

    // Clean up event listeners for this room
    client.off("room.update", onRoomUpdate);
    client.off("room.update.bracket", onRoomUpdateBracket);

  } catch (err) {
    console.error("[4wide-bot] Error in room lifecycle:", err);
    client.social.status("online", "menus");
    // Wait a bit before retrying/waiting again to avoid spamming
    await new Promise((resolve) => setTimeout(resolve, 5000));
  }
}
