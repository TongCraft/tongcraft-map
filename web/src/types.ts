export interface Manifest {
  schema: number;
  release: string;
  tileSize: number;
  format: string;
  markers: string;
  dimensions: Dimension[];
}

export interface Dimension {
  id: string;
  bounds: [number, number, number, number];
  maxLevel: number;
  coverage: string;
  tileTemplate: string;
}

export interface Marker {
  id: string;
  dimension: string;
  name: string;
  x: number;
  y?: number | null;
  z: number;
  products?: string[];
}

export interface ReleasePointer {
  schema: number;
  release: string;
}

export interface CoverageFile {
  schema: number;
  dimension: string;
  levels: Record<string, Record<string, [number, number][]>>;
}

export interface ParsedLink {
  dimension: string;
  x: number;
  y: number;
  z: number;
}

export interface LoadedRelease {
  release: string;
  preview: boolean;
  manifest: Manifest;
  markers: Marker[];
}
