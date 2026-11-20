#!/usr/bin/env python3
"""Generate a small synthetic Minecraft-like snapshot for TongCraft Map.

This is not a real Minecraft save and does not claim compatibility with any
specific vanilla version. It exercises the reader's 1.18+ root `sections`
shape, modern dimension folders, negative chunk/tile coordinates, water depth,
a Nether roof ignored by `below_y`, and a missing End chunk.

Output is intended for local checks and CI fixtures; keep it outside the repo
or under an ignored directory.
"""

from __future__ import annotations

import argparse
import gzip
import os
import struct
import zlib
from collections import defaultdict
from pathlib import Path

DATA_VERSION = 3955
AIR = "minecraft:air"


def u16(value: str) -> bytes:
    data = value.encode("utf-8")
    return struct.pack(">H", len(data)) + data


def tag_byte(name: str, value: int) -> bytes:
    return b"\x01" + u16(name) + struct.pack(">b", value)


def tag_int(name: str, value: int) -> bytes:
    return b"\x03" + u16(name) + struct.pack(">i", value)


def tag_string(name: str, value: str) -> bytes:
    return b"\x08" + u16(name) + u16(value)


def tag_compound(name: str, payload: bytes) -> bytes:
    return b"\x0a" + u16(name) + payload + b"\x00"


def tag_list_compounds(name: str, payloads: list[bytes]) -> bytes:
    body = b"".join(payload + b"\x00" for payload in payloads)
    return b"\x09" + u16(name) + b"\x0a" + struct.pack(">i", len(payloads)) + body


def tag_long_array(name: str, values: list[int]) -> bytes:
    body = b"".join(struct.pack(">q", value) for value in values)
    return b"\x0c" + u16(name) + struct.pack(">i", len(values)) + body


def root_compound(entries: bytes) -> bytes:
    return b"\x0a\x00\x00" + entries + b"\x00"


def section_payload(y: int, blocks: dict[tuple[int, int, int], str]) -> bytes:
    palette = sorted({AIR, *blocks.values()})
    if palette == [AIR]:
        return b""
    palette_payload = []
    for name in palette:
        palette_payload.append(tag_string("Name", name))
    bits = 4
    values = [0] * 4096
    index_by_name = {name: index for index, name in enumerate(palette)}
    for (local_x, local_z, local_y), name in blocks.items():
        index = (local_y * 16 + local_z) * 16 + local_x
        values[index] = index_by_name[name]
    entries_per_long = 64 // bits
    longs = [0] * ((4096 + entries_per_long - 1) // entries_per_long)
    for index, value in enumerate(values):
        long_index = index // entries_per_long
        offset = (index % entries_per_long) * bits
        longs[long_index] |= value << offset
    block_states = tag_list_compounds("palette", palette_payload)
    block_states += tag_long_array("data", longs)
    payload = tag_byte("Y", y)
    payload += tag_compound("block_states", block_states)
    return payload


def chunk_nbt(
    chunk_x: int,
    chunk_z: int,
    sections: dict[int, dict[tuple[int, int, int], str]],
) -> bytes:
    entries = tag_int("DataVersion", DATA_VERSION)
    entries += tag_string("Status", "minecraft:full")
    entries += tag_int("xPos", chunk_x)
    entries += tag_int("zPos", chunk_z)
    section_payloads = []
    for section_y in sorted(sections):
        payload = section_payload(section_y, sections[section_y])
        if payload:
            section_payloads.append(payload)
    entries += tag_list_compounds("sections", section_payloads)
    return root_compound(entries)


def write_region(
    path: Path,
    chunks: dict[tuple[int, int], bytes],
    external: set[tuple[int, int]] | None = None,
    gzip_chunks: set[tuple[int, int]] | None = None,
) -> None:
    external = external or set()
    gzip_chunks = gzip_chunks or set()
    header = bytearray(8192)
    bodies = []
    sector = 2
    by_local = {}
    for (chunk_x, chunk_z), nbt in chunks.items():
        by_local[(chunk_x % 32, chunk_z % 32)] = (chunk_x, chunk_z, nbt)
    for (local_x, local_z), (chunk_x, chunk_z, nbt) in sorted(by_local.items()):
        if (chunk_x, chunk_z) in external:
            compressed = zlib.compress(nbt, 6)
            path.parent.mkdir(parents=True, exist_ok=True)
            (path.parent / f"c.{chunk_x}.{chunk_z}.mcc").write_bytes(compressed)
            blob = struct.pack(">I", 1) + b"\x82"
        else:
            if (chunk_x, chunk_z) in gzip_chunks:
                compressed = gzip.compress(nbt, 6)
                compression = b"\x01"
            else:
                compressed = zlib.compress(nbt, 6)
                compression = b"\x02"
            blob = struct.pack(">I", len(compressed) + 1) + compression + compressed
        sectors = (len(blob) + 4095) // 4096
        if sectors > 255:
            raise ValueError("chunk too large for one region slot")
        entry = (sector << 8) | sectors
        index = local_x + local_z * 32
        struct.pack_into(">I", header, index * 4, entry)
        bodies.append((sector, blob, sectors))
        sector += sectors
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("wb") as handle:
        handle.write(header)
        for start, blob, sectors in bodies:
            assert start * 4096 == handle.tell() or start * 4096 > handle.tell()
            padding = start * 4096 - handle.tell()
            if padding > 0:
                handle.write(b"\x00" * padding)
            handle.write(blob)
            handle.write(b"\x00" * (sectors * 4096 - len(blob)))


def overworld_sections(chunk_x: int, chunk_z: int) -> dict[int, dict]:
    sections = defaultdict(dict)
    for local_z in range(16):
        for local_x in range(16):
            world_x = chunk_x * 16 + local_x
            world_z = chunk_z * 16 + local_z
            if world_x < 0 and world_z < 0:
                ground = 60
                top = "minecraft:sand"
                water_top = 63
            elif (world_x - 10) ** 2 + (world_z - 10) ** 2 < 18:
                ground = 72
                top = "minecraft:snow_block"
                water_top = 0
            else:
                ground = 64
                top = "minecraft:grass_block"
                water_top = 0
            if 4 <= world_x <= 6 and 4 <= world_z <= 6:
                ground = 65
                top = "minecraft:stone"
                water_top = 0
            for y in range(ground - 2, ground + 1):
                material = "minecraft:stone" if y < ground - 1 else "minecraft:dirt"
                if y == ground:
                    material = top
                sections[y // 16][(local_x, local_z, y % 16)] = material
            for y in range(ground + 1, water_top + 1):
                sections[y // 16][(local_x, local_z, y % 16)] = "minecraft:water"
    # A small tree and a deep negative-Y marker block.
    if (chunk_x, chunk_z) == (0, 0):
        sections[4][(2, 2, 1)] = "minecraft:oak_log"
        sections[4][(2, 2, 2)] = "minecraft:oak_log"
        sections[4][(2, 2, 3)] = "minecraft:oak_log"
        for dx in (-1, 0, 1):
            for dz in (-1, 0, 1):
                sections[4][(2 + dx, 2 + dz, 4)] = "minecraft:oak_leaves"
        sections[4][(2, 2, 5)] = "minecraft:oak_leaves"
        sections[4][(7, 7, 1)] = "testmod:unknown_block"
    if (chunk_x, chunk_z) == (-1, -1):
        sections[-1][(0, 0, 0)] = "minecraft:stone"
    return sections


def nether_sections(chunk_x: int, chunk_z: int) -> dict[int, dict]:
    sections = defaultdict(dict)
    for local_z in range(16):
        for local_x in range(16):
            world_x = chunk_x * 16 + local_x
            world_z = chunk_z * 16 + local_z
            top = "minecraft:netherrack"
            if (world_x + world_z) % 9 == 0:
                top = "minecraft:soul_sand"
            if (world_x - 5) ** 2 + (world_z - 5) ** 2 < 4:
                top = "minecraft:lava"
            sections[2][(local_x, local_z, 8)] = "minecraft:netherrack"
            sections[2][(local_x, local_z, 9)] = top
            sections[7][(local_x, local_z, 15)] = "minecraft:bedrock"
    return sections


def end_sections() -> dict[int, dict]:
    sections = defaultdict(dict)
    for local_z in range(16):
        for local_x in range(16):
            sections[3][(local_x, local_z, 12)] = "minecraft:end_stone"
    sections[3][(0, 0, 12)] = "minecraft:obsidian"
    for local_y in range(13, 16):
        sections[3][(0, 0, local_y)] = "minecraft:purpur_block"
    sections[4][(0, 0, 0)] = "minecraft:purpur_block"
    return sections


def write_dimension(
    root: Path,
    relative: str,
    chunks: dict[tuple[int, int], bytes],
    external: set[tuple[int, int]] | None = None,
    gzip_chunks: set[tuple[int, int]] | None = None,
) -> None:
    regions = defaultdict(dict)
    for (chunk_x, chunk_z), nbt in chunks.items():
        region_x = chunk_x // 32
        region_z = chunk_z // 32
        regions[(region_x, region_z)][(chunk_x, chunk_z)] = nbt
    for (region_x, region_z), region_chunks in regions.items():
        write_region(
            root / relative / f"r.{region_x}.{region_z}.mca",
            region_chunks,
            external=external,
            gzip_chunks=gzip_chunks,
        )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--out",
        type=Path,
        default=Path(".local-tests/synthetic"),
        help="output directory (snapshot, config, markers, output)",
    )
    args = parser.parse_args()
    out: Path = args.out.resolve()
    snapshot = out / "snapshot"
    if out.exists():
        raise SystemExit(f"refusing to overwrite existing directory: {out}")

    overworld = {
        (x, z): chunk_nbt(x, z, overworld_sections(x, z))
        for x, z in [(-1, -1), (0, -1), (-1, 0), (0, 0), (20, 20)]
    }
    nether = {
        (x, z): chunk_nbt(x, z, nether_sections(x, z))
        for x, z in [(-1, 0), (0, 0)]
    }
    end = {(0, 0): chunk_nbt(0, 0, end_sections())}

    write_dimension(snapshot, "dimensions/minecraft/overworld/region", overworld)
    write_dimension(
        snapshot,
        "dimensions/minecraft/the_nether/region",
        nether,
        gzip_chunks={(-1, 0)},
    )
    write_dimension(
        snapshot,
        "dimensions/minecraft/the_end/region",
        end,
        external={(0, 0)},
    )

    markers = [
        {
            "id": "demo-overworld-machine",
            "dimension": "world",
            "name": "测试机器",
            "x": 5,
            "y": 65,
            "z": 5,
            "products": ["测试产物"],
        },
        {
            "id": "demo-nether-machine",
            "dimension": "world_nether",
            "name": "下界测试点",
            "x": 3,
            "y": 42,
            "z": 3,
            "products": [],
        },
        {
            "id": "demo-end-machine",
            "dimension": "world_the_end",
            "name": "末地测试点",
            "x": 0,
            "y": 60,
            "z": 0,
            "products": [],
        },
    ]
    import json

    (out / "markers.json").write_text(
        json.dumps(markers, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    repo_root = Path(__file__).resolve().parents[1]
    style_path = os.path.relpath(repo_root / "styles" / "default.toml", out)
    config = f"""version = 1
style = "{style_path.replace(os.sep, '/')}"
markers = "markers.json"

[output]
dir = "output"

[render]
threads = 4

[dimensions.world]
layer = "surface"

[dimensions.world_nether]
layer = {{ below_y = 100 }}

[dimensions.world_the_end]
layer = "surface"
"""
    (out / "map.toml").write_text(config, encoding="utf-8")
    print(f"synthetic snapshot: {snapshot}")
    print(f"config: {out / 'map.toml'}")
    print(f"markers: {out / 'markers.json'}")


if __name__ == "__main__":
    main()
