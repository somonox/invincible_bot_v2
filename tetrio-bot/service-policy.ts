export const MAX_PPS = 5;
export const REQUIRED_SETTINGS: Record<string, string | number | boolean> = {
  boardwidth: 4,
  boardheight: 26,
  kickset: "SRS-X",
  allow180: true,
  display_hold: true,
  allow_harddrop: true,
  combotable: "multiplier",
  garbageblocking: "combo blocking",
  allclears: true,
  allclear_garbage: 10,
  room_handling: false,
};
// Triangle 4.2.7 room messages omit unchanged defaults. The actual round engine
// is checked independently before any adapter is started.
const DEFAULTS = {
  ...REQUIRED_SETTINGS,
  boardwidth: 10,
  boardheight: 20,
  kickset: "SRS+",
  spinbonuses: "T-spins",
};
const SPINS = new Set(["all", "all-mini+", "T-spins"]);
export function roomProblems(options: Record<string, any>) {
  const effective = { ...DEFAULTS, ...options };
  const problems = Object.entries(REQUIRED_SETTINGS)
    .filter(([k, v]) => effective[k as keyof typeof effective] !== v)
    .map(([k]) => k);
  if (!SPINS.has(effective.spinbonuses)) problems.push("spinbonuses");
  return problems;
}
export function roomProblemDetails(options: Record<string, any>) {
  const effective = { ...DEFAULTS, ...options };
  return roomProblems(options).map((key) =>
    key === "spinbonuses"
      ? `spinbonuses=${effective.spinbonuses} (use all, all-mini+ or T-spins)`
      : `${key}=${effective[key as keyof typeof effective]} (required ${REQUIRED_SETTINGS[key]})`,
  );
}
export function requiredChanges(options: Record<string, any>) {
  const changes = Object.entries(REQUIRED_SETTINGS)
    .filter(([k, v]) => options[k] !== v)
    .map(([k, value]) => ({ index: `options.${k}`, value }));
  if (options.spinbonuses !== undefined && !SPINS.has(options.spinbonuses))
    changes.push({ index: "options.spinbonuses", value: "all" });
  return changes;
}
export function engineProblems(e: any): string[] {
  const bad: string[] = [];
  if (e.kickTableName !== "SRS-X") bad.push("kickset");
  if (e.board.width !== 4 || e.board.height !== 26) bad.push("board size");
  if (
    !e.misc.allowed.spin180 ||
    !e.misc.allowed.hold ||
    !e.misc.allowed.hardDrop
  )
    bad.push("180/hold/hard drop");
  if (e.gameOptions.comboTable !== "multiplier") bad.push("combotable");
  if (e.gameOptions.garbageBlocking !== "combo blocking")
    bad.push("garbageblocking");
  if (!SPINS.has(e.gameOptions.spinBonuses)) bad.push("spinbonuses");
  if (!e.pc || e.pc.garbage !== 10) bad.push("PC bonus");
  if (e.handling.sdf !== 41 || e.handling.arr !== 0) bad.push("handling");
  return bad;
}
export function parsePps(text: string): number | null {
  if (!/^(?:\d+(?:\.\d*)?|\.\d+)$/.test(text)) return null;
  const value = Number(text);
  return Number.isFinite(value) && value >= 0.1 && value <= MAX_PPS
    ? value
    : null;
}
export function envInt(
  env: NodeJS.ProcessEnv,
  key: string,
  fallback: number,
  min: number,
  max: number,
) {
  if (env[key] === undefined) return fallback;
  const n = Number(env[key]);
  if (!Number.isSafeInteger(n) || n < min || n > max)
    throw new Error(`${key} must be an integer from ${min} to ${max}`);
  return n;
}
export class RoomPool {
  private rooms = new Map<string, { user: string; token: symbol }>();
  constructor(
    readonly limit = 20,
    readonly perUser = 2,
  ) {}
  reserve(room: string, user: string): symbol | null {
    if (
      this.rooms.has(room) ||
      this.rooms.size >= this.limit ||
      [...this.rooms.values()].filter((v) => v.user === user).length >=
        this.perUser
    )
      return null;
    const token = Symbol(room);
    this.rooms.set(room, { user, token });
    return token;
  }
  release(room: string, token: symbol) {
    if (this.rooms.get(room)?.token === token) this.rooms.delete(room);
  }
  get size() {
    return this.rooms.size;
  }
}
