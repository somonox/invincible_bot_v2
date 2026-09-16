// Pure snapshot helper: no account, network, or mutable engine references.
export function garbageContext(engine: any, pps: number, lastInputFrames = 12) {
  const speed = Math.max(0, Number(engine.garbageQueue.options.garbage.speed) || 0);
  return {
    packets: engine.garbageQueue.queue.map((packet: any) => ({
      amount: packet.amount,
      readyIn: Math.max(0, Math.ceil(packet.frame + speed - engine.frame)),
    })),
    // The wrapper waits a PPS interval after lock, then executes the next inputs.
    // The last path's input duration estimates the next lock; refreshed each move.
    nextLockFrames: Math.max(2, Math.ceil(lastInputFrames)),
    framesPerPiece: Math.max(1, Math.ceil(60 / Math.max(0.01, pps) + lastInputFrames)),
    cap: Math.max(0, Math.floor(Math.min(engine.dynamic.garbageCap.get(), engine.garbageQueue.options.cap.max))),
  };
}
