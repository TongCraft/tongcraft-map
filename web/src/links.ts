import type { ParsedLink } from "./types";

const RELEASE_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/;
const MAX_HASH_LENGTH = 200;

export function validReleaseId(value: string): boolean {
  return RELEASE_PATTERN.test(value) && value !== "." && value !== "..";
}

export function parseLocationHash(
  hash: string,
  dimensions: ReadonlySet<string>,
): ParsedLink | null {
  if (!hash.startsWith("#") || hash.length > MAX_HASH_LENGTH) {
    return null;
  }
  const parts = hash.slice(1).split(":");
  if (parts.length < 4) {
    return null;
  }
  const dimension = parts[0];
  if (!dimensions.has(dimension)) {
    return null;
  }
  const x = Number(parts[1]);
  const y = Number(parts[2]);
  const z = Number(parts[3]);
  if (!Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(z)) {
    return null;
  }
  return { dimension, x, y, z };
}

export function formatLink(
  dimension: string,
  x: number,
  y: number,
  z: number,
): string {
  const clean = (value: number) =>
    Number.isInteger(value) ? String(value) : value.toFixed(3);
  return `#${dimension}:${clean(x)}:${clean(y)}:${clean(z)}:200:0:0:0:1:flat`;
}

export function parseRelease(value: unknown): string | null {
  if (typeof value !== "string" || !validReleaseId(value)) {
    return null;
  }
  return value;
}
