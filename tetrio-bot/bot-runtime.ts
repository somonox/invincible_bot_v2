import { garbageContext } from "./garbage-context";
import { MAX_PPS } from "./service-policy";
import { withTimeout } from "./timeout";
export function installBotRuntime(BotWrapper: any) {
  // Monkey-patch BotWrapper.frames to space out consecutive movements/rotations with frame gaps
  BotWrapper.frames = (engine: any, keys: string[]) => {
    const round = (r: number) => Math.round(r * 10) / 10;
    let running = engine.frame + engine.subframe;
    return keys.flatMap((key) => {
      const mappedKey =
        key === "dasLeft" ? "moveLeft" : key === "dasRight" ? "moveRight" : key;
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
  BotWrapper.prototype.tick = async function (
    this: any,
    engine: any,
    events: any,
    data?: any,
  ) {
    const fullData = {
      state: undefined,
      play: undefined,
      ...data,
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
          this.config.pps = Math.max(
            0.1,
            Math.min(MAX_PPS, Number(this.config.pps) || 2),
          );
          this.nextFrame = BotWrapper.nextFrame(engine, this.config.pps);
          this.needsNewMove = false;
        }
      } else {
        // Always refresh immediately before planning, including packets arriving
        // while waiting for the PPS timer or for the previous piece to lock.
        this.config.pps = Math.max(
          0.1,
          Math.min(MAX_PPS, Number(this.config.pps) || 2),
        );
        if (this.roundSignal.aborted) return [];
        this.adapter.update(engine, {
          ...fullData.state,
          garbageContext: garbageContext(
            engine,
            this.config.pps,
            this.lastInputFrames,
          ),
        });
        const { keys } = await withTimeout<{ keys: string[] }>(
          this.adapter.play(engine, fullData.play),
          1500,
          "Adapter move",
          () => this.abortRound(),
        );
        this.lastInputFrames = keys.length * 2;
        if (this.roundSignal.aborted) return [];
        const frames = BotWrapper.frames(engine, keys);
        // Average-PPS scheduling can catch up in a burst after lag or !pps changes.
        // Bound actual consecutive lock inputs as well, including unequal paths.
        const drop = frames.find(
          (frame: any) =>
            frame.type === "keydown" && frame.data.key === "hardDrop",
        );
        if (drop) {
          const dropAt = drop.frame + drop.data.subframe;
          const delay = Math.max(
            0,
            (this.lastDropAt ?? -Infinity) + 60 / this.config.pps - dropAt,
          );
          if (delay > 0)
            for (const frame of frames) {
              const at =
                Math.round((frame.frame + frame.data.subframe + delay) * 10) /
                10;
              frame.frame = Math.floor(at);
              frame.data.subframe = Math.round((at - frame.frame) * 10) / 10;
            }
          this.lastDropAt = drop.frame + drop.data.subframe;
        }
        this.needsNewMove = true;
        this.lastPieces = engine.stats.pieces;
        return frames;
      }
    }

    return [];
  };
}
