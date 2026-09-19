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
