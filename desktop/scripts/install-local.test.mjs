import { readFileSync } from "node:fs";
import vm from "node:vm";
import assert from "node:assert/strict";
import test from "node:test";

const source = readFileSync(new URL("./install-local.mjs", import.meta.url), "utf8");
const helper = source.slice(source.indexOf("function unregisterApp("), source.indexOf("\nfunction validateBundle"));
const path = "/Users/example/.Trash/小陈的画布.app";
const missing = { stderr: `failed to scan ${path}: -10814\n from spotlight` };
function invoke(error, registry = "") {
  const context = vm.createContext({
    lsregister: "lsregister",
    execFileSync(command, args, options) {
      assert.equal(command, "lsregister");
      if (args[0] === "-dump") {
        assert.equal(options.maxBuffer, 64 * 1024 * 1024);
        return registry;
      }
      assert.equal(args[0], "-u");
      assert.equal(args[1], path);
      if (error) throw error;
    },
    console: { log() {} },
  });
  vm.runInContext(helper, context);
  context.unregisterApp(path);
}

test("successful unregister needs no fallback", () => assert.doesNotThrow(() => invoke(null)));
test("missing registration tolerates a registry larger than the default buffer", () => {
  assert.doesNotThrow(() => invoke(missing, "x".repeat(2 * 1024 * 1024)));
});
test("an exact remaining registration still rejects installation", () => {
  assert.throws(() => invoke(missing, `path:   ${path} (0x255c)`));
});
test("unrelated unregister errors are not swallowed", () => {
  assert.throws(() => invoke({ stderr: "permission denied" }));
});
test("similarly named paths do not count as the exact app", () => {
  assert.doesNotThrow(() => invoke(missing, `path:   ${path}-other (0x255c)`));
});
