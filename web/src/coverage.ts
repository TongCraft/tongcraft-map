import type { CoverageFile } from "./types";

export class CoverageIndex {
  private readonly runs = new Map<string, Array<[number, number]>>();

  constructor(file: CoverageFile) {
    if (file.schema !== 1) {
      throw new Error("coverage schema 不是 1");
    }
    for (const [level, rows] of Object.entries(file.levels)) {
      for (const [tz, row] of Object.entries(rows)) {
        this.runs.set(`${level}:${tz}`, row);
      }
    }
  }

  has(level: number, tx: number, tz: number): boolean {
    const row = this.runs.get(`${level}:${tz}`);
    if (!row) {
      return false;
    }
    return row.some(([start, end]) => tx >= start && tx < end);
  }
}

export async function loadCoverage(
  base: string,
  path: string,
  dimension: string,
): Promise<CoverageIndex> {
  const response = await fetch(`${base}${path}`, { cache: "no-cache" });
  if (!response.ok) {
    throw new Error(`coverage 请求失败：${response.status}`);
  }
  const file = (await response.json()) as CoverageFile;
  if (file.dimension !== dimension) {
    throw new Error(`coverage 维度 ${file.dimension} 与 ${dimension} 不一致`);
  }
  return new CoverageIndex(file);
}
