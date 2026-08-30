"""Fixed DaVinci Resolve bridge. It intentionally exposes no write operation."""

import importlib.util
import json
import os


def emit(payload, exit_code=0):
    print(json.dumps(payload, separators=(",", ":")))
    raise SystemExit(exit_code)


module_path = os.environ.get("INFINITE_CANVAS_RESOLVE_MODULE")
if not module_path:
    emit({"ok": False, "code": "module_import_failed"}, 2)

try:
    spec = importlib.util.spec_from_file_location("DaVinciResolveScript", module_path)
    if spec is None or spec.loader is None:
        emit({"ok": False, "code": "module_import_failed"}, 2)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
except PermissionError:
    emit({"ok": False, "code": "permission_denied"}, 3)
except (ImportError, OSError):
    emit({"ok": False, "code": "module_import_failed"}, 2)

try:
    resolve = module.scriptapp("Resolve")
    if resolve is None:
        emit({"ok": False, "code": "resolve_unavailable"}, 3)

    version = resolve.GetVersionString()
    project_manager = resolve.GetProjectManager()
    project = project_manager.GetCurrentProject() if project_manager else None
    timeline = project.GetCurrentTimeline() if project else None
    emit(
        {
            "ok": True,
            "version": version,
            "project_loaded": project is not None,
            "timeline_loaded": timeline is not None,
        }
    )
except PermissionError:
    emit({"ok": False, "code": "permission_denied"}, 3)
except (AttributeError, TypeError):
    emit({"ok": False, "code": "scripting_library_unavailable"}, 2)
except Exception:
    emit({"ok": False, "code": "probe_failed"}, 4)
