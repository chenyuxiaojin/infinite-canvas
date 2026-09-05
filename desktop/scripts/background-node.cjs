// Next changes process.title at startup. Node 24's macOS implementation checks
// into Launch Services using the enclosing App bundle, creating a second Dock
// application. Keep title changes in JS for this desktop sidecar only.
if (process.platform === "darwin") {
  let title = process.title;
  Object.defineProperty(process, "title", {
    configurable: true,
    enumerable: true,
    get: () => title,
    set: (value) => { title = String(value); },
  });
}
