import { lstatSync, mkdirSync, readFileSync, readdirSync, readlinkSync, renameSync, symlinkSync, unlinkSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";

const desktopDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const { productName } = JSON.parse(readFileSync(join(desktopDir, "src-tauri/tauri.conf.json"), "utf8"));
if (typeof productName !== "string" || !productName.trim() || /[\\/]/.test(productName)) throw new Error("无效的应用名称");
const builtApp = join(desktopDir, "src-tauri/target/release/bundle/macos", `${productName}.app`);
const installedApp = join(homedir(), "Applications", `${productName}.app`);
const legacyApp = join(homedir(), "Applications/无限画布.app");
const bundleId = "com.chenyuxiaojin.infinitecanvas";
const lsregister = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
const stamp = `${new Date().toISOString().replace(/[:.]/g, "-")}-${process.pid}`;
const archiveDir = resolve(desktopDir, "../../infinite-canvas-backups/local-installs");
const archive = join(archiveDir, `${productName}-${stamp}.zip`);
const retiredApp = join(homedir(), ".Trash", `${productName}-替换前-${stamp}.app`);
const cliLink = join(homedir(), ".local/bin/infinite-canvas");
const newCli = join(installedApp, "Contents/MacOS/infinite-canvas");
const oldCli = join(legacyApp, "Contents/MacOS/infinite-canvas");
const run = (command, args) => execFileSync(command, args, { stdio: "inherit" });
const output = (command, args) => execFileSync(command, args, { encoding: "utf8" }).trim();

function unregisterApp(path) {
  try {
    execFileSync(lsregister, ["-u", path], { encoding: "utf8", stdio: "pipe" });
  } catch (error) {
    const detail = `${error.stdout ?? ""}\n${error.stderr ?? ""}`;
    if (!/failed to scan [^\n]*: -10814\b/.test(detail)) throw error;
    // A freshly moved Trash path may never have been registered. Only tolerate
    // that specific failure after proving the exact path has no registration.
    const registry = execFileSync(lsregister, ["-dump"], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
    const registered = registry.split("\n").some((line) => {
      const match = line.match(/^path:\s+(.+?)(?: \(0x[0-9a-f]+\))?\s*$/i);
      return match?.[1] === path;
    });
    if (registered) throw error;
    console.log(`此路径没有应用登记，无需注销：${path}`);
  }
}

function validateBundle(path) {
  if (!lstatSync(path).isDirectory() || lstatSync(path).isSymbolicLink()) {
    throw new Error(`拒绝替换非普通 App 目录：${path}`);
  }
  if (output("/usr/bin/plutil", ["-extract", "CFBundleIdentifier", "raw", join(path, "Contents/Info.plist")]) !== bundleId) {
    throw new Error(`App 身份不符：${path}`);
  }
}

function nativeLibraries(path) {
  return readdirSync(path, { withFileTypes: true }).flatMap((entry) => {
    const child = join(path, entry.name);
    if (entry.isDirectory()) return nativeLibraries(child);
    return entry.isFile() && /\.(node|dylib)$/.test(entry.name) ? [child] : [];
  });
}

function ensureAppStopped() {
  const commands = output("/bin/ps", ["-axo", "comm="]).split("\n").map((s) => s.trim());
  if (commands.some((command) => [installedApp, legacyApp, builtApp].some((app) => command.startsWith(`${app}/Contents/MacOS/`)))) {
    throw new Error(`请先正常退出画布 App，再安装${productName}；不会强行结束画布或覆盖运行中的 App。`);
  }
}

function replaceCliLink(target) {
  const temporary = `${cliLink}.${stamp}`;
  mkdirSync(dirname(cliLink), { recursive: true });
  symlinkSync(target, temporary);
  try {
    renameSync(temporary, cliLink);
  } finally {
    if (lstatSync(temporary, { throwIfNoEntry: false })?.isSymbolicLink()) unlinkSync(temporary);
  }
}

if (process.platform !== "darwin" || process.arch !== "arm64") {
  throw new Error("此安装流程仅用于本机 Apple Silicon macOS。");
}
validateBundle(builtApp);
const previousApps = [...new Set([installedApp, legacyApp])].filter((path) => lstatSync(path, { throwIfNoEntry: false }));
if (previousApps.length > 1) throw new Error("新旧名称的 App 同时存在，请先确认要替换的正式版本，安装器不会猜测。");
const previousApp = previousApps[0];
if (previousApp) validateBundle(previousApp);
const cliStat = lstatSync(cliLink, { throwIfNoEntry: false });
const previousCliTarget = cliStat?.isSymbolicLink() ? readlinkSync(cliLink) : null;
// Never overwrite a user's independent CLI or an unrelated symbolic link.
const updateCliLink = !cliStat || previousCliTarget === oldCli || previousCliTarget === newCli;
ensureAppStopped();

// Local ad-hoc signatures, not a Developer ID / notarized distribution build.
for (const path of [
  ...nativeLibraries(join(builtApp, "Contents/Resources")),
  ...["infinite-canvas", "infinite-canvas-api", "node"].map((name) => join(builtApp, "Contents/MacOS", name)),
  builtApp,
]) run("/usr/bin/codesign", ["--force", "--sign", "-", path]);
run("/usr/bin/codesign", ["--verify", "--deep", "--strict", builtApp]);

if (previousApp) {
  mkdirSync(archiveDir, { recursive: true });
  run("/usr/bin/ditto", ["-c", "-k", "--sequesterRsrc", "--keepParent", previousApp, archive]);
  run("/usr/bin/unzip", ["-tq", archive]);
  // A ZIP backup cannot be mistaken for a second installed application.
  console.log(`旧版已压缩并验证：${archive}`);
}
ensureAppStopped();
mkdirSync(dirname(installedApp), { recursive: true });
mkdirSync(dirname(retiredApp), { recursive: true });
let retired = false;
let installed = false;
let relinked = false;
try {
  if (previousApp) {
    unregisterApp(previousApp);
    renameSync(previousApp, retiredApp);
    retired = true;
  }
  unregisterApp(builtApp);
  // Move the verified build instead of leaving another runnable .app behind.
  renameSync(builtApp, installedApp);
  installed = true;
  run("/usr/bin/codesign", ["--verify", "--deep", "--strict", installedApp]);
  run(lsregister, ["-f", installedApp]);
  if (retired) unregisterApp(retiredApp);
  if (updateCliLink) {
    replaceCliLink(newCli);
    relinked = true;
  }
} catch (error) {
  if (relinked) {
    if (previousCliTarget) replaceCliLink(previousCliTarget);
    else unlinkSync(cliLink);
  }
  if (installed) renameSync(installedApp, builtApp);
  if (retired) {
    renameSync(retiredApp, previousApp);
    run(lsregister, ["-f", previousApp]);
  }
  throw error;
}
console.log(`已安装唯一正式入口：${installedApp}`);
if (retired) console.log(`旧安装包位于废纸篓，可恢复：${retiredApp}`);
console.log(updateCliLink ? `画布命令已指向：${newCli}` : "保留现有独立 CLI，未修改其文件或链接。");
console.log(`应用身份与数据目录未改动。请启动${productName}验证；打开片子的终端时会更新该目录的画布 MCP 路径。`);
