# External connectors handoff

## Scope

`integrations/external-connectors-rust/` is a standalone Rust crate for read-only local discovery of Eagle and DaVinci Resolve. It is intentionally not wired into the Tauri shell, Web UI, or Go service in this branch.

The crate never modifies an Eagle library or a Resolve project. It has no arbitrary shell, command, script, URL, or HTTP method entry point.

## Public interface

The stable entry points are exported from `src/lib.rs`:

- `probe_all() -> Vec<ProviderReport>` probes both providers with production runtimes.
- `EagleProvider::new(runtime).probe()` probes Eagle.
- `DaVinciProvider::new(runtime).probe()` probes DaVinci Resolve.
- `EagleRuntime` and `DaVinciRuntime` allow deterministic mocks without touching live applications.

`ProviderReport` has one shared status contract:

| Status | Meaning |
| --- | --- |
| `available` | The provider's fixed read-only health/context probe succeeded. |
| `not_installed` | No supported application bundle was found. |
| `not_running` | The application is installed but its process is not running. |
| `permission_missing` | macOS or the application's scripting/API permission blocked the probe. |
| `incompatible` | An installed component, endpoint, or response shape is unsupported. |
| `error` | A non-permission, non-compatibility runtime failure occurred. |

Every report retains a human-readable `diagnostic`. `facts` contains only booleans, API/version identifiers, and environment classification. It deliberately excludes Eagle library names/paths, Resolve project/timeline names, media metadata, and credentials.

## External-call allowlist

Production code permits only these calls:

1. `/usr/bin/pgrep -x Eagle`
2. `/usr/bin/pgrep -x Resolve`
3. `GET http://127.0.0.1:41595/api/v2/library/info`, with an 800 ms timeout and a 64 KiB response limit
4. `/usr/bin/python3 <crate>/src/resolve_probe.py` with a cleared environment and only fixed module/library environment keys populated from standard Resolve installation candidates

The Resolve bridge invokes only these documented read methods:

- `scriptapp("Resolve")`
- `Resolve.GetVersionString()`
- `Resolve.GetProjectManager()`
- `ProjectManager.GetCurrentProject()`
- `Project.GetCurrentTimeline()`

It never asks for names and never calls `SaveProject`, render methods, media-pool mutation methods, settings setters, or any other write method. The local installed Resolve scripting README confirms that Resolve must be running for external scripts, documents the macOS module/library environment paths, and lists all five methods above as reads.

## Test and live evidence

Mock/fixture tests are kept separate from the live probe:

```text
cargo test --all-targets
14 passed; 0 failed
```

The fixtures cover both providers across not installed, not running, permission missing, missing components, malformed responses, and available responses. Available-response fixtures contain fake private library/project/timeline names; assertions prove none of those values enter `ProviderReport`.

Static checks:

```text
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

Live probe command:

```text
cargo run --bin external-connectors-probe -- all
```

Minimal live result from this handoff run:

```json
[
  {
    "provider": "eagle",
    "status": "available",
    "facts": {
      "installed": true,
      "running": true,
      "api_reachable": true,
      "api_version": "v2",
      "library_loaded": true
    }
  },
  {
    "provider": "davinci_resolve",
    "status": "not_running",
    "facts": {
      "installed": true,
      "running": false,
      "scripting_module_found": true,
      "scripting_library_found": true,
      "environment": {
        "resolve_script_api": "missing",
        "resolve_script_lib": "missing",
        "pythonpath": "missing"
      }
    }
  }
]
```

No Eagle item, tag, folder, or file endpoint was requested; library-info details beyond the boolean context signal were not retained or recorded. No Resolve scripting connection was attempted after the provider determined that Resolve was not running.

## Facts, inference, unknowns

Facts:

- Eagle was installed and running; the fixed V2 library-info GET returned a supported healthy response.
- DaVinci Resolve was installed but not running.
- The standard Resolve Python module and native scripting library were present.
- `RESOLVE_SCRIPT_API`, `RESOLVE_SCRIPT_LIB`, and `PYTHONPATH` were each absent in the probe process; the bridge supplies only its own approved module/library values when a connection is attempted.
- The installed local Resolve scripting documentation was last updated 24 Jul 2026 and matched the paths and read methods used here.

Inference:

- When Resolve is started, the same code should either return `available` with version/context booleans or identify scripting access as `permission_missing`/`incompatible`. This path was covered by fixtures but not claimed as live success.

Unknown:

- The live Resolve version, whether a project is loaded, and whether a timeline is loaded remain unknown because Resolve was not running.
- The user's current Resolve external-scripting preference remains unknown; this layer does not change preferences.

## Assembly notes

1. Add this crate to the future macOS Rust workspace or use it as a path dependency.
2. Call probes off the UI thread because they perform bounded local process/HTTP checks.
3. Treat `ProviderReport.status` as the machine contract and display `diagnostic` unchanged for operator troubleshooting.
4. Do not infer `available` from installation alone. Only the probe returns `available`.
5. Preserve the production runtime allowlists. UI/API inputs may select only `all`, `eagle`, or `davinci`; they must never become URLs, shell arguments, Python source, or Resolve method names.
6. Run `cargo test --all-targets` for fixtures. Run the CLI separately only when a live, read-only operator check is intended.

This branch does not modify `docs/development/macos-director-acceptance-matrix.md`; the sole assembly task can update broader acceptance tracking after merging.
