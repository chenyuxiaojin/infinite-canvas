import {
  cpSync,
  existsSync,
  mkdirSync,
  rmSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const desktopDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repositoryDir = resolve(desktopDir, "..");
const webDir = join(repositoryDir, "web");
const tauriDir = join(desktopDir, "src-tauri");
const binariesDir = join(tauriDir, "binaries");
const webResourceDir = join(tauriDir, "resources", "web");
const targetTriple = "aarch64-apple-darwin";
const packagedNodeVersion = "v24.12.0";

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? repositoryDir,
    env: options.env ?? process.env,
    stdio: "inherit",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} exited with status ${result.status}`);
  }
}

if (process.platform !== "darwin" || process.arch !== "arm64") {
  throw new Error("P1 desktop packaging currently supports Apple Silicon macOS only");
}
if (process.release.name !== "node" || !existsSync(process.execPath)) {
  throw new Error("Run this preparation script with the Node.js runtime being packaged");
}
if (process.version !== packagedNodeVersion) {
  throw new Error(
    `Expected Node.js ${packagedNodeVersion}, received ${process.version}`,
  );
}

run("bun", ["run", "build"], { cwd: webDir });

const standaloneDir = join(webDir, ".next", "standalone");
const staticDir = join(webDir, ".next", "static");
const publicDir = join(webDir, "public");
for (const requiredPath of [standaloneDir, staticDir, publicDir]) {
  if (!existsSync(requiredPath)) {
    throw new Error(`Missing Next.js build input: ${requiredPath}`);
  }
}

rmSync(webResourceDir, { recursive: true, force: true });
mkdirSync(webResourceDir, { recursive: true });
cpSync(standaloneDir, webResourceDir, { recursive: true });
cpSync(publicDir, join(webResourceDir, "public"), { recursive: true });
mkdirSync(join(webResourceDir, ".next"), { recursive: true });
cpSync(staticDir, join(webResourceDir, ".next", "static"), { recursive: true });

mkdirSync(binariesDir, { recursive: true });
run(
  "go",
  [
    "build",
    "-trimpath",
    "-ldflags=-s -w",
    "-o",
    join(binariesDir, `infinite-canvas-api-${targetTriple}`),
    ".",
  ],
  {
    env: {
      ...process.env,
      CGO_ENABLED: "0",
      GOARCH: "arm64",
      GOOS: "darwin",
    },
  },
);
run("lipo", [
  "-thin",
  "arm64",
  process.execPath,
  "-output",
  join(binariesDir, `node-${targetTriple}`),
]);

console.log(`Prepared desktop resources for ${targetTriple}`);
