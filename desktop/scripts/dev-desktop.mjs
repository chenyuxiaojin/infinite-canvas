import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const desktopDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: desktopDir,
    env: process.env,
    stdio: "inherit",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

run(process.execPath, [join(desktopDir, "scripts", "prepare-desktop.mjs")]);
run(join(desktopDir, "node_modules", ".bin", "tauri"), [
  "dev",
  "--no-watch",
  ...process.argv.slice(2),
]);
