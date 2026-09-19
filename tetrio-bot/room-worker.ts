import { Client } from "@haelp/teto";
import { BotWrapper, adapters } from "@haelp/teto/utils";
import {
  engineProblems,
  parsePps,
  requiredChanges,
  roomProblems,
  roomProblemDetails,
} from "./service-policy";
import { withTimeout } from "./timeout";
import type { ReplayStore } from "./replay-store";

export interface WorkerOptions {
  adapterPath: string;
  defaultPps: number;
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
  let applyingSettings = false;
  let lastStatus = "";
  let pauseReason = "";
  let finish!: () => void;
  const left = new Promise<void>((resolve) => {
    finish = () => {
      closing = true;
      resolve();
    };
  });
  const notices = new Map<string, number>();
  const saved = new WeakSet<object>();
  let observedOwner: string | undefined;
  let returnHost: string | undefined;
  let selfReplayGameId: number | undefined;
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
    console.log(`[Worker-${roomid}] ${message}`);
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
    pauseReason = message;
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
        const status = `${room.state}; bracket=${room.self?.bracket}; ${roomProblemDetails(room.options ?? {}).join("; ") || (faulted ? pauseReason : "settings ready")}`;
        if (status !== lastStatus) {
          lastStatus = status;
          console.log(`[Worker-${roomid}] ${status}`);
        }
        if (problems.length && room.self?.bracket !== "spectator") {
          stopRound();
          await withTimeout(
            Promise.resolve(room.switch("spectator")),
            5000,
            "Spectator switch",
          );
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
            `Bot requires SRS-X and a 4x26 board. Fix: ${roomProblemDetails(room.options ?? {}).join("; ")}. Give the bot host, then use !setup to apply settings.`,
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
    observedOwner = room.owner;
    on("room.update.host", (owner: string) => {
      if (owner === client.user.id && observedOwner !== owner)
        returnHost = observedOwner;
      observedOwner = owner;
      return reconcile();
    });
    on("room.update.bracket", reconcile);
    // Do not queue empty-room departure behind pending chat or settings requests.
    on("room.player.remove", () => {
      if (!room.players.some((p: any) => p._id !== client.user.id)) {
        stopRound();
        finish();
        return;
      }
      return reconcile();
    });
    on("room.player.add", () => {
      return reconcile();
    });
    on("room.chat", async (chat: any) => {
      if (chat.system || chat.user._id === client.user.id) return;
      const parts = chat.content.trim().split(/\s+/);
      const command = parts[0].toLowerCase();
      if (!["!pps", "!bot", "!leave", "!setup"].includes(command)) return;
      if (command === "!bot") {
        const problems = roomProblemDetails(room.options ?? {});
        const status = problems.length
          ? `Blocked: ${problems.join("; ")}. Give the bot host and use !setup.`
          : faulted
            ? pauseReason
            : round?.ready
              ? "Playing."
              : "Ready; waiting for the next round.";
        await notice(
          "status",
          `4wide bot: ${status} SRS-X, PPS ${roomPps}/5. Matches are saved as .ttrm replays. !setup applies required settings and returns host (bot needs host). !pps <0.1-5>, !leave (host/inviter).`,
        );
        return;
      }
      if (
        chat.user._id !== room.owner &&
        chat.user._id !== inviter &&
        !(command === "!setup" && room.isHost && chat.user._id === returnHost)
      ) {
        await notice(
          "permission",
          "Only the room host or bot inviter can use control commands.",
        );
        return;
      }
      if (command === "!setup") {
        if (parts.length !== 1) {
          await notice("setup", "Use !setup with no arguments.");
          return;
        }
        if (!room.isHost) {
          await notice(
            "setup",
            "Give the bot host, then use !setup to apply the required settings.",
          );
          return;
        }
        if (room.state === "ingame") {
          await notice(
            "setup",
            "Use !setup in the lobby after the match ends.",
          );
          return;
        }
        if (applyingSettings) return;
        applyingSettings = true;
        try {
          const changes = requiredChanges(room.options ?? {});
          if (changes.length)
            await withTimeout(
              Promise.resolve(room.update(...changes)),
              5000,
              "Room settings",
            );
          await reconcile();
          const problems = roomProblems(room.options ?? {});
          if (problems.length) {
            await notice(
              "setup",
              `Settings still required: ${problems.join(", ")}.`,
            );
            return;
          }
          if (closing) return;
          const recipient = room.players.some((p: any) => p._id === returnHost)
            ? returnHost
            : chat.user._id;
          let returned = false;
          if (
            room.isHost &&
            room.players.some((p: any) => p._id === recipient)
          ) {
            await withTimeout(
              Promise.resolve(room.transferHost(recipient)),
              5000,
              "Host return",
            );
            returned = room.owner === recipient;
          }
          await notice(
            "setup",
            `Settings ready: SRS-X and a 4x26 board. ${returned ? "Host returned." : "Host return was not confirmed; check the room host."}`,
          );
        } catch (error: any) {
          await notice(
            "setup",
            `Setup or host return failed: ${error.message}. Check the room host and retry !setup.`,
          );
        } finally {
          applyingSettings = false;
        }
        return;
      }
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
    // Client.on("game.replay") sees incoming opponent frames only. Self's
    // actual start/full/key/IGE frames travel through Ribbon's send path.
    // Pipe them into storage directly; emitting game.replay again would send
    // duplicate gameplay to the server instead of notifying a local listener.
    on("client.ribbon.send", ({ command, data }: any) => {
      if (
        command !== "game.replay" ||
        selfReplayGameId === undefined ||
        data?.gameid !== selfReplayGameId
      )
        return;
      room.replay?.pipe(data);
    });
    // Room's own handler has already initialized ReplayManager by this event.
    // Subscribe to raw replay streams without replaying every opponent engine.
    on("game.ready", (data: any) => {
      selfReplayGameId = data.players.find(
        (p: any) => p.userid === client.user.id,
      )?.gameid;
      scopes.clear();
      for (const player of data.players)
        if (player.userid !== client.user.id) {
          scopes.add(player.gameid);
          client.emit("game.scope.start", player.gameid);
        }
    });
    on("client.game.end", () => {
      selfReplayGameId = undefined;
      saveReplay(false);
      stopScopes();
      faulted = false;
      void reconcile().catch(finish);
    });
    on("client.game.abort", () => {
      selfReplayGameId = undefined;
      saveReplay(true);
      stopScopes();
      faulted = false;
    });
    on("client.game.over", () => stopRound());
    on("client.game.round.start", ([tick, engine]: any[]) => {
      stopRound();
      // Self.init emits this event during Game construction. Use the passed
      // engine, not client.game (which may still refer to the previous round).
      if (room.self?.bracket !== "player" || faulted) {
        console.log(
          `[Worker-${roomid}] Round skipped: bracket=${room.self?.bracket}, ${pauseReason || roomProblemDetails(room.options ?? {}).join("; ")}`,
        );
        return;
      }
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
          console.log(
            `[Worker-${roomid}] Adapter ready for ${engine.board.width}x${engine.board.height}.`,
          );
        })
        .catch((error) =>
          failRound(`Bot initialization failed: ${error.message}`),
        );
    });
    await reconcile();
    await notice(
      "welcome",
      "Bot connected: SRS-X required, PPS limit 5. Give the bot host and use !setup for required settings. Matches with this bot are saved as replays. !bot for help. For bug reports or suggestions, please DM a6a6_.",
    );
    await left;
  } catch (error: any) {
    console.error(`[Worker-${roomid}] ${error.message}`);
  } finally {
    closing = true;
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
