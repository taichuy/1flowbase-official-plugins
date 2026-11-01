export const PALACE_GRID_POSITIONS: Record<
  string,
  { gridRow: number; gridColumn: number; x: number; y: number }
> = {
  巳: { gridRow: 1, gridColumn: 1, x: 12.5, y: 12.5 },
  午: { gridRow: 1, gridColumn: 2, x: 37.5, y: 12.5 },
  未: { gridRow: 1, gridColumn: 3, x: 62.5, y: 12.5 },
  申: { gridRow: 1, gridColumn: 4, x: 87.5, y: 12.5 },
  酉: { gridRow: 2, gridColumn: 4, x: 87.5, y: 37.5 },
  戌: { gridRow: 3, gridColumn: 4, x: 87.5, y: 62.5 },
  亥: { gridRow: 4, gridColumn: 4, x: 87.5, y: 87.5 },
  子: { gridRow: 4, gridColumn: 3, x: 62.5, y: 87.5 },
  丑: { gridRow: 4, gridColumn: 2, x: 37.5, y: 87.5 },
  寅: { gridRow: 4, gridColumn: 1, x: 12.5, y: 87.5 },
  卯: { gridRow: 3, gridColumn: 1, x: 12.5, y: 62.5 },
  辰: { gridRow: 2, gridColumn: 1, x: 12.5, y: 37.5 },
};
