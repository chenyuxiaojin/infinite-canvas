use serde_json::{json, Value};

pub fn catalog() -> Value {
    json!({
        "schema_version": "1.0",
        "transport": {
            "kind": "http_loopback",
            "listen_host": "127.0.0.1",
            "authentication": "desktop_install_credential",
            "public_network": false
        },
        "operation_protocol": {
            "actor": "agent",
            "required_fields": ["project_id", "request_id", "base_revision", "actor", "operations"],
            "idempotency": "request_id_and_payload",
            "concurrency": "canvas_operation_state_revision_compare_and_swap",
            "canonical_adapter": "CanonicalCanvasAdapter",
            "mutation_semantics": "applyCanvasOperationBatch",
            "storage": "same_canvas_projects_row"
        },
        "capabilities": [
            {
                "id": "capabilities.read",
                "method": "GET",
                "path": "/v1/capabilities",
                "risk": "read_only",
                "dry_run": false,
                "paid": false,
                "source": "agent_bridge"
            },
            {
                "id": "projects.list",
                "method": "GET",
                "path": "/v1/projects",
                "risk": "read_only",
                "dry_run": false,
                "paid": false,
                "source": "go_canvas_projects_same_database"
            },
            {
                "id": "projects.get",
                "method": "GET",
                "path": "/v1/projects/{project_id}",
                "risk": "read_only",
                "dry_run": false,
                "paid": false,
                "source": "go_canvas_projects_same_database"
            },
            {
                "id": "canvas.operations.dry_run",
                "method": "POST",
                "path": "/v1/canvas/operations/dry-run",
                "risk": "read_only",
                "dry_run": true,
                "paid": false,
                "source": "CanonicalCanvasAdapter"
            },
            {
                "id": "canvas.operations.apply",
                "method": "POST",
                "path": "/v1/canvas/operations/apply",
                "risk": "reversible_write",
                "dry_run": true,
                "paid": false,
                "source": "CanonicalCanvasAdapter",
                "operations": [
                    "create_text_node",
                    "move_node",
                    "set_node_text",
                    "set_project_title",
                    "add_connection",
                    "remove_connection"
                ]
            },
            {
                "id": "runtime.probe",
                "method": "GET",
                "path": "/v1/runtime",
                "risk": "read_only",
                "dry_run": false,
                "paid": false,
                "source": "DesktopRuntime"
            },
            {
                "id": "tasks.test_clip",
                "method": "POST",
                "path": "/v1/tasks/test-clips",
                "risk": "reversible_write",
                "dry_run": false,
                "paid": false,
                "mode": "deterministic_local_fixture",
                "source": "DesktopRuntime"
            },
            {
                "id": "tasks.status",
                "method": "GET",
                "path": "/v1/tasks/{task_id}",
                "risk": "read_only",
                "dry_run": false,
                "paid": false,
                "source": "DesktopRuntime"
            },
            {
                "id": "tasks.cancel",
                "method": "POST",
                "path": "/v1/tasks/{task_id}/cancel",
                "risk": "irreversible_local_side_effect",
                "dry_run": false,
                "paid": false,
                "source": "DesktopRuntime"
            },
            {
                "id": "credentials.revoke",
                "method": "POST",
                "path": "/v1/credentials/revoke",
                "risk": "security_state_change",
                "dry_run": false,
                "paid": false,
                "source": "agent_bridge"
            }
        ],
        "existing_interfaces": {
            "go_rest": [
                "GET /api/v1/canvas/projects",
                "POST /api/v1/canvas/projects",
                "POST /api/v1/canvas/projects/sync",
                "POST /api/v1/canvas/projects/delete",
                "POST /api/v1/canvas/image-tasks",
                "GET /api/v1/canvas/image-tasks/{id}",
                "POST /api/v1/canvas/audio-tasks",
                "GET /api/v1/canvas/audio-tasks/{id}",
                "GET /api/v1/video-tasks",
                "DELETE /api/v1/video-tasks/{id}"
            ],
            "tauri_ipc": [
                "probe_desktop_runtime",
                "generate_desktop_test_clip",
                "generate_canvas_test_clip",
                "desktop_task_status",
                "desktop_task_media",
                "cancel_desktop_task",
                "desktop_canvas_projects",
                "save_desktop_canvas_project",
                "delete_desktop_canvas_projects"
            ],
            "desktop_runtime": [
                "ffmpeg_probe",
                "external_connector_probe",
                "local_audio_service_probe",
                "deterministic_test_clip",
                "task_status",
                "task_cancel"
            ]
        },
        "explicitly_denied": [
            "arbitrary_shell",
            "arbitrary_executable",
            "arbitrary_path",
            "arbitrary_url",
            "paid_generation",
            "public_network_listener",
            "raw_sql"
        ]
    })
}
