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
                "id": "projects.create",
                "method": "POST",
                "path": "/v1/projects",
                "risk": "reversible_write",
                "dry_run": false,
                "paid": false,
                "source": "CanonicalCanvasAdapter",
                "idempotency": "request_id_and_payload"
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
                    "create_image_node",
                    "create_video_node",
                    "create_config_node",
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
                "id": "media.inbox",
                "method": "GET",
                "path": "/v1/media/inbox",
                "risk": "read_only",
                "dry_run": false,
                "paid": false,
                "source": "DesktopRuntime",
                "arbitrary_paths": false
            },
            {
                "id": "media.video_ingest",
                "method": "POST",
                "path": "/v1/media/video-ingests",
                "risk": "reversible_write",
                "dry_run": false,
                "paid": false,
                "source": "DesktopRuntime+CanonicalCanvasAdapter",
                "accepted_mime_types": ["video/mp4"],
                "path_scope": "fixed_app_support_inbox_basename_only",
                "integrity": "required_lowercase_sha256",
                "canvas_node_type": "video"
            },
            {
                "id": "media.image_ingest",
                "method": "POST",
                "path": "/v1/media/image-ingests",
                "risk": "reversible_write",
                "dry_run": false,
                "paid": false,
                "source": "DesktopRuntime+CanonicalCanvasAdapter",
                "accepted_mime_types": ["image/png", "image/jpeg", "image/webp"],
                "path_scope": "fixed_app_support_inbox_basename_only",
                "integrity": "required_lowercase_sha256",
                "canvas_node_type": "image"
            },
            {
                "id": "generation.video_request",
                "method": "POST",
                "path": "/v1/generation/video-requests",
                "risk": "paid_write_pending_human_approval",
                "dry_run": false,
                "paid": true,
                "approval_required": true,
                "source": "DesktopRuntime+CanonicalCanvasAdapter",
                "resolutions": ["768P", "2K"],
                "duration_seconds_range": [4, 15],
                "keyframe_scope": "existing_image_node_with_local_media",
                "note": "只创建 pending_approval 任务与占位节点；人工在画布上批准前不调用任何付费 API"
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
                "desktop_task_media_reference",
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
                "allowlisted_mp4_ingest",
                "allowlisted_image_ingest",
                "task_status",
                "task_cancel"
            ]
        },
        "explicitly_denied": [
            "arbitrary_shell",
            "arbitrary_executable",
            "arbitrary_path",
            "arbitrary_url",
            "unapproved_paid_generation",
            "public_network_listener",
            "raw_sql"
        ]
    })
}
