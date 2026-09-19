import { Client } from "@haelp/teto";
import { BotWrapper, adapters } from "@haelp/teto/utils";
import {
  engineProblems,
  parsePps,
  requiredChanges,
  roomProblems,
} from "./service-policy";
import { withTimeout } from "./search-limiter";
import type { ReplayStore } from "./replay-store";

export interface WorkerOptions {
  adapterPath: string;
  defaultPps: number;
  idleMs: number;
  replays: ReplayStore;
  signal: AbortSignal;
  createClient?: () => Promise<any>;
  createAdapter?: () => any;
  createWrapper?: (adapter: any, pps: number) => any;
}
export async function runRoomWorker(
  roomid: string,
  inviter: string,
  options: WorkerOptions,
) {
  let client: any,
    room: any,
    round: any = null;
  let closing = false,
    faulted = false,
    roomPps = options.defaultPps;
  let lastActivity = Date.now(),
    lastConfigAttempt = 0;
  let idleTimer: ReturnType<typeof setInterval> | undefined;
  let finish!: () => void;
  const left = new Promise<void>((resolve) => {
    finish = () => {
      closing = true;
      resolve();
    };
  });
  const notices = new Map<string, number>();
  const saved = new WeakSet<object>();
  let replayBytes = 0,
    replayDisabled = false;
  const scopes = new Set<number>();
  const stopRound = (target = round) => {
    if (!target || target.stopped) return;
    target.stopped = true;
    target.controller.abort();
    try {
      target.wrapper.stop();
    } catch {
      try {
        target.adapter.stop();
      } catch {}
    }
    // The SDK removes listeners on stop; late pipe/spawn errors still need a sink.
    target.adapter.process?.on("error", () => {});
    target.adapter.process?.stdin?.on("error", () => {});
    // Escalate only this owned child; no process-name matching.
    const timer = setTimeout(() => {
      if (target.adapter.process?.exitCode === null)
        target.adapter.process.kill("SIGKILL");
    }, 500);
    timer.unref?.();
    if (round === target) round = null;
  };
  const notice = async (key: string, message: string) => {
    if (closing || !room || Date.now() - (notices.get(key) ?? 0) < 5000) return;
    notices.set(key, Date.now());
    await withTimeout(
      Promise.resolve(room.chat(message)),
      5000,
      "Room chat",
    ).catch(() => {});
  };
  const saveReplay = (partial = false) => {
    const manager = room?.replay;
    if (!manager || saved.has(manager)) return;
    let replay: any;
    try {
      replay = manager.export();
    } catch (error: any) {
      console.error(
        `[Worker-${roomid}] Replay export failed: ${error.message}`,
      );
      return;
    }
    if (!replay?.replay?.rounds?.length) return;
    saved.add(manager);
    try {
      void options.replays
        .save(roomid, replay, partial)
        .then((file) => console.log(`[Worker-${roomid}] Replay saved: ${file}`))
        .catch((error) =>
          console.error(
            `[Worker-${roomid}] Replay save failed: ${error.message}`,
          ),
        );
    } catch (error: any) {
      console.error(`[Worker-${roomid}] Replay save failed: ${error.message}`);
    }
  };
  const stopScopes = () => {
    if (client) for (const id of scopes) client.emit("game.scope.end", id);
    scopes.clear();
  };
  const suspend = async (message: string) => {
    faulted = true;
    stopRound();
    if (room)
      await withTimeout(
        Promise.resolve(room.switch("spectator")),
        5000,
        "Spectator switch",
      ).catch(() => {});
    await notice("suspended", message);
  };
  let checking = false,
    checkAgain = false;
  const reconcile = async () => {
    if (checking) {
      checkAgain = true;
      return;
    }
    checking = true;
    try {
      do {
        checkAgain = false;
        if (closing || !client?.room) return;
        room = client.room;
        if (!room.players.some((p: any) => p._id !== client.user.id)) {
          finish();
          return;
        }
        let problems = roomProblems(room.options ?? {});
        if (problems.length && room.self?.bracket !== "spectator") {
          stopRound();
          await withTimeout(
            Promise.resolve(room.switch("spectator")),
            5000,
            "Spectator switch",
          );
        }
        if (room.isHost && Date.now() - lastConfigAttempt >= 5000) {
          const changes = requiredChanges(room.options ?? {});
          if (changes.length) {
            lastConfigAttempt = Date.now();
            await withTimeout(
              Promise.resolve(room.update(...changes)),
              5000,
              "Room settings",
            ).catch((error) =>
              console.error(`[Worker-${roomid}] Settings: ${error.message}`),
            );
          }
        }
        problems = roomProblems(room.options ?? {});
        const bracket = problems.length || faulted ? "spectator" : "player";
        if (room.self?.bracket !== bracket)
          await withTimeout(
            Promise.resolve(room.switch(bracket)),
            5000,
            "Bracket switch",
          );
        if (problems.length)
          await notice(
            "settings",
            `Bot requires SRS-X, 4x20, hold/180/hard drop, multiplier and combo blocking. Fix: ${problems.join(", ")}. Give the bot host to apply settings automatically.`,
          );
      } while (checkAgain && !closing);
    } finally {
      checking = false;
    }
  };
  const abort = () => {
    stopRound();
    finish();
  };
  options.signal.addEventListener("abort", abort, { once: true });
  try {
    if (options.signal.aborted) return;
    const connect = (
      options.createClient ??
      (() =>
        Client.create({
          username: process.env.BOT_USERNAME!,
          password: process.env.BOT_PASSWORD!,
        }))
    )();
    client = await withTimeout(
      connect.then(async (c) => {
        if (closing) {
          await c.destroy();
          throw new Error("Worker closed during connect");
        }
        return c;
      }),
      15000,
      "Worker connect",
      finish,
    );
    const on = (event: string, handler: (data: any) => unknown) =>
      client.on(event, (data: any) => {
        if (closing) return;
        try {
          Promise.resolve(handler(data)).catch((error) => {
            console.error(`[Worker-${roomid}] ${event}: ${error.message}`);
            stopRound();
            finish();
          });
        } catch (error: any) {
          console.error(`[Worker-${roomid}] ${event}: ${error.message}`);
          stopRound();
          finish();
        }
      });
    on("room.leave", finish);
    on("room.kick", finish);
    on("client.dead", finish);
    room = await withTimeout(
      client.rooms.join(roomid),
      10000,
      "Room join",
      finish,
    );
    if (closing) return;
    on("room.update", reconcile);
    on("room.update.host", reconcile);
    on("room.update.bracket", reconcile);
    on("room.player.remove", reconcile);
    on("room.player.add", () => {
      lastActivity = Date.now();
      return reconcile();
    });
    on("room.chat", async (chat: any) => {
      if (chat.system || chat.user._id === client.user.id) return;
      const parts = chat.content.trim().split(/\s+/);
      const command = parts[0].toLowerCase();
      if (!["!pps", "!bot", "!leave"].includes(command)) return;
      if (command === "!bot") {
        await notice(
          "status",
          `4wide bot: SRS-X, PPS ${roomPps}/5. Matches are saved as .ttrm replays. !pps <0.1-5>, !leave (host/inviter).`,
        );
        return;
      }
      if (chat.user._id !== room.owner && chat.user._id !== inviter) {
        await notice(
          "permission",
          "Only the room host or bot inviter can change PPS or remove the bot.",
        );
        return;
      }
      lastActivity = Date.now();
      if (command === "!leave") {
        finish();
        return;
      }
      if (parts.length === 1) {
        await notice("pps", `Current PPS: ${roomPps} (maximum 5).`);
        return;
      }
      const value = parts.length === 2 ? parsePps(parts[1]) : null;
      if (value === null) {
        await notice("pps", "Invalid PPS. Use a number between 0.1 and 5.");
        return;
      }
      roomPps = value;
      if (round) round.wrapper.config.pps = value;
      await notice("pps", `PPS updated to ${value} (maximum 5).`);
    });
    // Room's own handler has already initialized ReplayManager by this event.
    // Subscribe to raw replay streams without replaying every opponent engine.
    on("game.ready", (data: any) => {
      lastActivity = Date.now();
      scopes.clear();
      if (data.isNew) {
        replayBytes = 0;
        replayDisabled = false;
      }
      if (replayDisabled) return;
      for (const player of data.players)
        if (player.userid !== client.user.id) {
          scopes.add(player.gameid);
          client.emit("game.scope.start", player.gameid);
        }
    });
    on("game.replay", (data: any) => {
      if (replayDisabled) return;
      replayBytes += Buffer.byteLength(JSON.stringify(data.frames));
      if (replayBytes > options.replays.limits.maxFileBytes * 0.75) {
        saveReplay(true);
        replayDisabled = true;
        stopScopes();
        room.replay = null;
        console.warn(
          `[Worker-${roomid}] Replay recording stopped at per-match size limit.`,
        );
      }
    });
    on("client.game.end", () => {
      saveReplay(false);
      stopScopes();
      faulted = false;
      lastActivity = Date.now();
      void reconcile().catch(finish);
    });
    on("client.game.abort", () => {
      saveReplay(true);
      stopScopes();
      faulted = false;
    });
    on("client.game.over", () => stopRound());
    on("client.game.round.start", ([tick, engine]: any[]) => {
      lastActivity = Date.now();
      stopRound();
      // Self.init emits this event during Game construction. Use the passed
      // engine, not client.game (which may still refer to the previous round).
      if (room.self?.bracket !== "player" || faulted) return;
      const problems = engineProblems(engine);
      if (problems.length) {
        void suspend(`Round blocked: unsupported ${problems.join(", ")}.`);
        return;
      }
      const adapter =
        options.createAdapter?.() ??
        new adapters.IO({
          path: options.adapterPath,
          verbose: false,
          env: { BOT_LOG_MOVES: process.env.BOT_LOG_MOVES ?? "0" },
        });
      const wrapper =
        options.createWrapper?.(adapter, roomPps) ??
        new BotWrapper(adapter, { pps: roomPps });
      const current = {
        adapter,
        wrapper,
        controller: new AbortController(),
        ready: false,
        stopped: false,
      };
      round = current;
      const failRound = (message: string) => {
        if (current.stopped || closing || round !== current) return;
        void suspend(message);
      };
      wrapper.roundSignal = current.controller.signal;
      wrapper.abortRound = () =>
        failRound("Bot search timed out; spectating until the next match.");
      // IO assigns process asynchronously. Attach before the assignment returns,
      // including spawn failures before the adapter sends its initial info message.
      let child = adapter.process;
      const watchChild = (process: any) => {
        if (!process) return;
        process.on("error", () =>
          failRound("Bot adapter failed; spectating until the next match."),
        );
        process.stdin?.on("error", () =>
          failRound(
            "Bot adapter pipe failed; spectating until the next match.",
          ),
        );
        process.once("exit", () =>
          failRound("Bot adapter exited; spectating until the next match."),
        );
        if (current.stopped || closing) {
          try {
            adapter.stop();
          } catch {}
          process.on("error", () => {});
          process.stdin?.on("error", () => {});
        }
      };
      Object.defineProperty(adapter, "process", {
        configurable: true,
        get: () => child,
        set: (value) => {
          child = value;
          watchChild(value);
        },
      });
      watchChild(child);
      tick(async ({ engine, events }: any) => {
        if (!current.ready || current.stopped || round !== current || closing)
          return { keys: [] };
        try {
          return { keys: await wrapper.tick(engine, events) };
        } catch (error: any) {
          failRound(`Bot paused after a search failure: ${error.message}`);
          return { keys: [] };
        }
      });
      void withTimeout(
        Promise.resolve().then(() => wrapper.init(engine)),
        5000,
        "Adapter initialization",
        () =>
          failRound(
            "Bot adapter initialization timed out; spectating until the next match.",
          ),
      )
        .then(() => {
          if (current.stopped || closing) {
            try {
              adapter.stop();
            } catch {}
            return;
          }
          current.ready = true;
        })
        .catch((error) =>
          failRound(`Bot initialization failed: ${error.message}`),
        );
    });
    await reconcile();
    await notice(
      "welcome",
      "Bot connected: SRS-X required, PPS limit 5. Matches with this bot are saved as replays. !bot for help.",
    );
    idleTimer = setInterval(
      () => {
        if (closing) return;
        if (
          room.state !== "ingame" &&
          Date.now() - lastActivity >= options.idleMs
        )
          finish();
      },
      Math.min(30000, options.idleMs),
    );
    await left;
  } catch (error: any) {
    console.error(`[Worker-${roomid}] ${error.message}`);
  } finally {
    closing = true;
    clearInterval(idleTimer);
    options.signal.removeEventListener("abort", abort);
    stopRound();
    saveReplay(true);
    stopScopes();
    if (client)
      await withTimeout(
        Promise.resolve(client.destroy()),
        5000,
        "Client cleanup",
      ).catch(() => {});
  }
}
