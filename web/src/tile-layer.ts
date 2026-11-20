import * as L from "leaflet";

import type { CoverageIndex } from "./coverage";

const TRANSPARENT =
  "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==";

export class CoverageTileLayer extends L.TileLayer {
  private readonly template: string;
  private readonly coverage: CoverageIndex;

  constructor(template: string, coverage: CoverageIndex) {
    super(template, {
      tileSize: 256,
      noWrap: true,
      keepBuffer: 2,
      updateWhenZooming: false,
      maxNativeZoom: 0,
      className: "world-tile",
    });
    this.template = template;
    this.coverage = coverage;
  }

  getTileUrl(coords: L.Coords): string {
    const level = Math.max(0, Math.round(-coords.z));
    const tx = coords.x;
    const tz = coords.y;
    if (!this.coverage.has(level, tx, tz)) {
      return TRANSPARENT;
    }
    return this.template
      .replace("{level}", String(level))
      .replace("{tx}", String(tx))
      .replace("{tz}", String(tz));
  }
}
