import * as L from "leaflet";
import "leaflet/dist/leaflet.css";
import "./styles.css";

import { loadCoverage, type CoverageIndex } from "./coverage";
import { formatLink, parseLocationHash, parseRelease, validReleaseId } from "./links";
import { CoverageTileLayer } from "./tile-layer";
import type {
  Dimension,
  LoadedRelease,
  Manifest,
  Marker,
  ReleasePointer,
} from "./types";

const $ = <T extends Element>(selector: string): T => {
  const element = document.querySelector<T>(selector);
  if (!element) {
    throw new Error(`缺少页面元素 ${selector}`);
  }
  return element;
};

const dimensionNav = $<HTMLElement>("#dimensions");
const releaseNote = $<HTMLParagraphElement>("#release-note");
const markerSelect = $<HTMLSelectElement>("#marker-select");
const markerDetail = $<HTMLElement>("#marker-detail");
const status = $<HTMLParagraphElement>("#status");
const shareButton = $<HTMLButtonElement>("#share");
const coords = $<HTMLElement>("#coords");

let loaded: LoadedRelease | null = null;
let map: L.Map | null = null;
let tileLayer: CoverageTileLayer | null = null;
let markerLayer: L.LayerGroup | null = null;
let currentDimension: Dimension | null = null;
let selectedMarker: Marker | null = null;
const coverageCache = new Map<string, CoverageIndex>();

function setStatus(message: string, isError = false): void {
  status.textContent = message;
  status.classList.toggle("error", isError);
}

function toLatLngBounds(bounds: [number, number, number, number]): L.LatLngBounds {
  const [minX, minZ, maxX, maxZ] = bounds;
  return L.latLngBounds(
    [-maxZ, minX],
    [-minZ, maxX],
  );
}

async function resolveRelease(): Promise<{ release: string; preview: boolean }> {
  const query = new URLSearchParams(window.location.search).get("release");
  const preview = parseRelease(query);
  if (preview) {
    return { release: preview, preview: true };
  }
  const response = await fetch("/current.json", { cache: "no-cache" });
  if (!response.ok) {
    throw new Error("地图尚未发布（/current.json 不存在）");
  }
  const pointer = (await response.json()) as ReleasePointer;
  if (pointer.schema !== 1 || !validReleaseId(pointer.release)) {
    throw new Error("当前版本指针无效");
  }
  return { release: pointer.release, preview: false };
}

async function fetchManifest(base: string): Promise<Manifest> {
  const response = await fetch(`${base}manifest.json`, { cache: "no-cache" });
  if (!response.ok) {
    throw new Error(`清单请求失败：${response.status}`);
  }
  const manifest = (await response.json()) as Manifest;
  if (manifest.schema !== 1) {
    throw new Error("manifest schema 不是 1");
  }
  if (manifest.tileSize !== 256 || manifest.format !== "png") {
    throw new Error("清单的瓦片格式不受支持");
  }
  if (!Array.isArray(manifest.dimensions) || manifest.dimensions.length === 0) {
    throw new Error("清单没有维度");
  }
  for (const dimension of manifest.dimensions) {
    const [minX, minZ, maxX, maxZ] = dimension.bounds;
    if (
      !Number.isFinite(minX) ||
      !Number.isFinite(minZ) ||
      !Number.isFinite(maxX) ||
      !Number.isFinite(maxZ) ||
      maxX <= minX ||
      maxZ <= minZ ||
      dimension.maxLevel < 0
    ) {
      throw new Error(`维度 ${dimension.id} 的范围或层级无效`);
    }
  }
  return manifest;
}

async function fetchMarkers(base: string, manifest: Manifest): Promise<Marker[]> {
  const response = await fetch(`${base}${manifest.markers}`, { cache: "no-cache" });
  if (!response.ok) {
    throw new Error(`标记请求失败：${response.status}`);
  }
  const markers = (await response.json()) as Marker[];
  if (!Array.isArray(markers)) {
    throw new Error("标记文件不是数组");
  }
  const dimensions = new Set(manifest.dimensions.map((dimension) => dimension.id));
  return markers.filter((marker) => {
    return (
      typeof marker.id === "string" &&
      typeof marker.name === "string" &&
      dimensions.has(marker.dimension) &&
      Number.isFinite(marker.x) &&
      Number.isFinite(marker.z) &&
      (marker.y === undefined ||
        marker.y === null ||
        Number.isFinite(marker.y))
    );
  });
}

async function loadRelease(): Promise<LoadedRelease> {
  const { release, preview } = await resolveRelease();
  const base = `/releases/${release}/`;
  const manifest = await fetchManifest(base);
  const markers = await fetchMarkers(base, manifest);
  return { release, preview, manifest, markers };
}

function renderDimensions(): void {
  if (!loaded) {
    return;
  }
  dimensionNav.replaceChildren();
  for (const dimension of loaded.manifest.dimensions) {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = dimension.id;
    button.dataset.dimension = dimension.id;
    button.className = "dimension";
    button.addEventListener("click", () => {
      void selectDimension(dimension.id);
    });
    dimensionNav.append(button);
  }
}

function updateDimensionButtons(): void {
  for (const button of dimensionNav.querySelectorAll<HTMLButtonElement>(".dimension")) {
    const active = button.dataset.dimension === currentDimension?.id;
    button.classList.toggle("active", active);
    button.setAttribute("aria-pressed", String(active));
  }
}

function populateMarkerSelect(): void {
  if (!loaded) {
    return;
  }
  markerSelect.replaceChildren();
  const placeholder = document.createElement("option");
  placeholder.value = "";
  placeholder.textContent = "选择一台机器…";
  markerSelect.append(placeholder);
  for (const marker of loaded.markers) {
    const option = document.createElement("option");
    option.value = marker.id;
    const y = marker.y === undefined || marker.y === null ? "—" : String(marker.y);
    option.textContent = `${marker.name} · ${marker.dimension} · ${marker.x},${y},${marker.z}`;
    markerSelect.append(option);
  }
}

function markerLink(marker: Marker): string {
  const y = marker.y === undefined || marker.y === null ? 64 : marker.y;
  return formatLink(marker.dimension, marker.x, y, marker.z);
}

function replaceHash(link: string): void {
  const url = new URL(window.location.href);
  url.hash = link;
  window.history.replaceState(null, "", url);
}

async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    setStatus("链接已复制");
    return;
  } catch {
    const area = document.createElement("textarea");
    area.value = text;
    area.setAttribute("readonly", "");
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.append(area);
    area.select();
    const ok = document.execCommand("copy");
    area.remove();
    setStatus(ok ? "链接已复制" : "复制失败，请手动复制地址栏", !ok);
  }
}

function showMarker(marker: Marker | null): void {
  selectedMarker = marker;
  markerDetail.replaceChildren();
  if (!marker) {
    const hint = document.createElement("p");
    hint.className = "muted";
    hint.textContent = "选择机器后可查看坐标和产物。";
    markerDetail.append(hint);
    markerSelect.value = "";
    return;
  }
  const title = document.createElement("h2");
  title.textContent = marker.name;
  const position = document.createElement("p");
  const y = marker.y === undefined || marker.y === null ? "—" : String(marker.y);
  position.textContent = `X ${marker.x} · Y ${y} · Z ${marker.z}`;
  position.className = "coordinate";
  markerDetail.append(title, position);
  if (marker.products && marker.products.length > 0) {
    const label = document.createElement("p");
    label.className = "muted small";
    label.textContent = "产物";
    const list = document.createElement("ul");
    for (const product of marker.products) {
      const item = document.createElement("li");
      item.textContent = product;
      list.append(item);
    }
    markerDetail.append(label, list);
  }
  markerSelect.value = marker.id;
}

function markerIcon(marker: Marker): L.CircleMarker {
  const circle = L.circleMarker([-marker.z, marker.x], {
    radius: 7,
    weight: 2,
    color: "#F6F2E7",
    fillColor: "#A65028",
    fillOpacity: 1,
  });
  circle.bindTooltip(marker.name, { direction: "top" });
  circle.on("click", () => {
    showMarker(marker);
    replaceHash(markerLink(marker));
  });
  return circle;
}

function buildMarkerLayer(): L.LayerGroup {
  const layer = L.layerGroup();
  if (!loaded || !currentDimension) {
    return layer;
  }
  for (const marker of loaded.markers) {
    if (marker.dimension === currentDimension.id) {
      layer.addLayer(markerIcon(marker));
    }
  }
  return layer;
}

async function coverageFor(dimension: Dimension): Promise<CoverageIndex> {
  if (!loaded) {
    throw new Error("地图尚未加载");
  }
  const cached = coverageCache.get(dimension.id);
  if (cached) {
    return cached;
  }
  const coverage = await loadCoverage(
    `/releases/${loaded.release}/`,
    dimension.coverage,
    dimension.id,
  );
  coverageCache.set(dimension.id, coverage);
  return coverage;
}

async function applyView(dimension: Dimension, focus?: { x: number; z: number }): Promise<void> {
  if (!map) {
    return;
  }
  const bounds = toLatLngBounds(dimension.bounds);
  map.setMinZoom(-dimension.maxLevel);
  if (focus) {
    const zoom = Math.min(4, map.getMaxZoom());
    map.setView([-focus.z, focus.x], zoom, { animate: false });
  } else {
    map.fitBounds(bounds, { padding: [32, 32], maxZoom: 4, animate: false });
  }
}

async function selectDimension(
  id: string,
  focus?: { x: number; z: number },
): Promise<void> {
  if (!loaded) {
    return;
  }
  const dimension = loaded.manifest.dimensions.find((candidate) => candidate.id === id);
  if (!dimension) {
    setStatus(`未知维度：${id}`, true);
    return;
  }
  currentDimension = dimension;
  updateDimensionButtons();
  markerSelect.value = "";
  showMarker(null);
  try {
    const coverage = await coverageFor(dimension);
    if (map) {
      if (tileLayer) {
        map.removeLayer(tileLayer);
      }
      if (markerLayer) {
        map.removeLayer(markerLayer);
      }
      const template = `/releases/${loaded.release}/${dimension.tileTemplate}`;
      tileLayer = new CoverageTileLayer(template, coverage);
      tileLayer.options.minZoom = -dimension.maxLevel;
      tileLayer.options.maxZoom = 5;
      tileLayer.options.maxNativeZoom = 0;
      tileLayer.options.minNativeZoom = -dimension.maxLevel;
      tileLayer.on("tileerror", () => {
        setStatus("瓦片加载失败：清单声明了该瓦片，但文件不可用", true);
      });
      tileLayer.addTo(map);
      markerLayer = buildMarkerLayer();
      markerLayer.addTo(map);
      await applyView(dimension, focus);
      setStatus(focus ? `已定位到 ${id}` : `已切换到 ${id}`);
    }
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error), true);
  }
}

function handleHash(): boolean {
  if (!loaded) {
    return false;
  }
  const dimensions = new Set(
    loaded.manifest.dimensions.map((dimension) => dimension.id),
  );
  const parsed = parseLocationHash(window.location.hash, dimensions);
  if (!parsed) {
    if (window.location.hash) {
      setStatus("链接无效，已回到维度概览", true);
    }
    return false;
  }
  void selectDimension(parsed.dimension, { x: parsed.x, z: parsed.z });
  return true;
}

function initializeMap(): void {
  map = L.map("map", {
    crs: L.CRS.Simple,
    minZoom: 0,
    maxZoom: 5,
    zoomSnap: 1,
    zoomDelta: 1,
    attributionControl: false,
  });
  map.on("mousemove", (event: L.LeafletMouseEvent) => {
    const x = Math.floor(event.latlng.lng);
    const z = Math.floor(-event.latlng.lat);
    coords.textContent = `X ${x} · Z ${z}`;
  });
  map.on("moveend", () => {
    if (!map) {
      return;
    }
    const center = map.getCenter();
    coords.textContent = `X ${Math.round(center.lng)} · Z ${Math.round(-center.lat)}`;
  });
}

function wireControls(): void {
  markerSelect.addEventListener("change", () => {
    if (!loaded) {
      return;
    }
    const marker = loaded.markers.find((candidate) => candidate.id === markerSelect.value);
    if (!marker) {
      showMarker(null);
      return;
    }
    showMarker(marker);
    void selectDimension(marker.dimension, { x: marker.x, z: marker.z }).then(() => {
      showMarker(marker);
      const link = markerLink(marker);
      replaceHash(link);
    });
  });

  shareButton.addEventListener("click", () => {
    if (!map || !currentDimension) {
      setStatus("地图尚未加载", true);
      return;
    }
    let x: number;
    let y = 64;
    let z: number;
    if (selectedMarker && selectedMarker.dimension === currentDimension.id) {
      x = selectedMarker.x;
      y = selectedMarker.y ?? 64;
      z = selectedMarker.z;
    } else {
      const center = map.getCenter();
      x = Math.round(center.lng);
      z = Math.round(-center.lat);
    }
    const link = formatLink(currentDimension.id, x, y, z);
    replaceHash(link);
    void copyText(`${window.location.origin}${window.location.pathname}${window.location.search}${link}`);
  });

  window.addEventListener("hashchange", () => {
    handleHash();
  });
}

async function main(): Promise<void> {
  try {
    loaded = await loadRelease();
    releaseNote.textContent = loaded.preview
      ? `预览版本 ${loaded.release}（未发布）`
      : `已发布版本 ${loaded.release}`;
    renderDimensions();
    populateMarkerSelect();
    initializeMap();
    wireControls();

    const dimensions = new Set(
      loaded.manifest.dimensions.map((dimension) => dimension.id),
    );
    const parsed = parseLocationHash(window.location.hash, dimensions);
    const invalidHash = Boolean(window.location.hash) && !parsed;
    if (parsed) {
      await selectDimension(parsed.dimension, { x: parsed.x, z: parsed.z });
    } else {
      await selectDimension(loaded.manifest.dimensions[0].id);
    }
    if (invalidHash) {
      setStatus("链接无效，已回到维度概览", true);
    }
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    releaseNote.textContent = "地图不可用";
    setStatus(message, true);
  }
}

void main();
