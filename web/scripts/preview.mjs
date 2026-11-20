#!/usr/bin/env node
import { createReadStream, existsSync, statSync } from "node:fs";
import { createServer } from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const webRoot = path.resolve(here, "..");
const repoRoot = path.resolve(webRoot, "..");
const distRoot = path.join(webRoot, "dist");

function parseArgs(argv) {
  const options = {
    host: "127.0.0.1",
    port: 4173,
    mapRoot: path.join(repoRoot, "output"),
  };
  for (let index = 0; index < argv.length; index += 1) {
    const value = argv[index];
    if (value === "--host") {
      options.host = argv[++index];
    } else if (value === "--port") {
      options.port = Number(argv[++index]);
    } else if (value === "--map-root") {
      options.mapRoot = path.resolve(argv[++index]);
    } else if (value === "--help") {
      console.log(
        "usage: node scripts/preview.mjs [--host 127.0.0.1] [--port 4173] [--map-root output]",
      );
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${value}`);
    }
  }
  if (!Number.isInteger(options.port) || options.port < 1 || options.port > 65535) {
    throw new Error("invalid port");
  }
  return options;
}

const mime = new Map([
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".css", "text/css; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".png", "image/png"],
  [".svg", "image/svg+xml"],
  [".ico", "image/x-icon"],
  [".map", "application/json; charset=utf-8"],
]);

function resolveInside(root, relative) {
  const candidate = path.resolve(root, `.${relative}`);
  const prefix = root.endsWith(path.sep) ? root : `${root}${path.sep}`;
  if (candidate !== root && !candidate.startsWith(prefix)) {
    return null;
  }
  return candidate;
}

function fileResponse(response, file) {
  if (!existsSync(file) || !statSync(file).isFile()) {
    return false;
  }
  const type = mime.get(path.extname(file).toLowerCase()) ?? "application/octet-stream";
  response.writeHead(200, {
    "content-type": type,
    "cache-control": "no-cache",
  });
  createReadStream(file).pipe(response);
  return true;
}

function notFound(response) {
  response.writeHead(404, { "content-type": "text/plain; charset=utf-8" });
  response.end("not found");
}

const RELEASE_ID = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/;

const options = parseArgs(process.argv.slice(2));
if (!existsSync(distRoot)) {
  console.error("web/dist 不存在，请先运行 npm run build");
  process.exit(1);
}

const server = createServer((request, response) => {
  const url = new URL(request.url ?? "/", `http://${request.headers.host ?? "localhost"}`);
  let pathname;
  try {
    pathname = decodeURIComponent(url.pathname);
  } catch {
    response.writeHead(400, { "content-type": "text/plain; charset=utf-8" });
    response.end("bad request");
    return;
  }

  if (pathname === "/current.json") {
    if (!fileResponse(response, path.join(options.mapRoot, "current.json"))) {
      notFound(response);
    }
    return;
  }

  if (pathname.startsWith("/releases/")) {
    const rest = pathname.slice("/releases/".length);
    const [release, ...parts] = rest.split("/");
    if (!RELEASE_ID.test(release) || parts.length === 0) {
      notFound(response);
      return;
    }
    const relative = parts.join("/");
    const publishedRoot = path.join(options.mapRoot, "releases", release);
    const stagedRoot = path.join(options.mapRoot, "staging", release);
    const published = resolveInside(publishedRoot, `/${relative}`);
    if (published && fileResponse(response, published)) {
      return;
    }
    const staged = resolveInside(stagedRoot, `/${relative}`);
    if (staged && fileResponse(response, staged)) {
      return;
    }
    notFound(response);
    return;
  }

  const candidate = resolveInside(distRoot, pathname);
  if (candidate && fileResponse(response, candidate)) {
    return;
  }
  if (!fileResponse(response, path.join(distRoot, "index.html"))) {
    notFound(response);
  }
});

server.listen(options.port, options.host, () => {
  console.log(`preview: http://${options.host}:${options.port}/`);
  console.log(`map root: ${options.mapRoot}`);
});
