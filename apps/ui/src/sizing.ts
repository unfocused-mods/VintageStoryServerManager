export function memoryEstimate(players: number, mods: string): number {
  // Vintage Story's published estimate plus explicit agent/OS and mod headroom.
  return (
    1 +
    players * 0.3 +
    0.5 +
    (mods === "heavy" ? 2 : mods === "light" ? 0.5 : 0)
  );
}
