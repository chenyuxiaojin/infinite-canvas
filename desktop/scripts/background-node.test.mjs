import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";

test("desktop title changes never reach macOS Launch Services", { skip: process.platform !== "darwin" }, () => {
  const script = `
    const { execFileSync } = require('node:child_process');
    const nativeTitle = () => execFileSync('/bin/ps', ['-p', String(process.pid), '-o', 'comm='], {encoding:'utf8'}).trim();
    const before = nativeTitle();
    process.title = 'canvas-background-regression-test';
    console.log(JSON.stringify({before, after:nativeTitle(), title:process.title}));
  `;
  const result = JSON.parse(execFileSync(process.execPath, ["--require", fileURLToPath(new URL("./background-node.cjs", import.meta.url)), "-e", script], { encoding: "utf8" }));
  assert.equal(result.title, "canvas-background-regression-test");
  assert.equal(result.after, result.before);
});
