export function withTimeout<T>(
  promise: Promise<T>,
  ms: number,
  label: string,
  onTimeout = () => {},
) {
  let timer: ReturnType<typeof setTimeout>;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      try {
        onTimeout();
      } catch {}
      reject(new Error(`${label} timed out`));
    }, ms);
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}
export class SearchLimiter {
  private active = 0;
  private queue: (() => void)[] = [];
  constructor(
    readonly limit: number,
    readonly maxQueued: number,
  ) {}
  run<T>(task: () => Promise<T>, signal: AbortSignal): Promise<T> {
    if (signal.aborted) return Promise.reject(new Error("Round ended"));
    if (this.active >= this.limit && this.queue.length >= this.maxQueued)
      return Promise.reject(new Error("Search queue full"));
    return new Promise<T>((resolve, reject) => {
      const cancel = () => {
        const i = this.queue.indexOf(start);
        if (i >= 0) this.queue.splice(i, 1);
        reject(new Error("Round ended"));
      };
      const start = () => {
        signal.removeEventListener("abort", cancel);
        if (signal.aborted) {
          reject(new Error("Round ended"));
          return;
        }
        this.active++;
        Promise.resolve()
          .then(task)
          .then(resolve, reject)
          .finally(() => {
            this.active--;
            this.queue.shift()?.();
          });
      };
      if (this.active < this.limit) start();
      else {
        this.queue.push(start);
        signal.addEventListener("abort", cancel, { once: true });
      }
    });
  }
}
