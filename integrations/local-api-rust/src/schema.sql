-- Existing SQLite schema retained without dropping user data.
CREATE TABLE IF NOT EXISTS agent_operation_requests (
                request_id TEXT PRIMARY KEY NOT NULL,
                project_id TEXT NOT NULL,
                payload_hash TEXT NOT NULL,
                response_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
CREATE TABLE IF NOT EXISTS `ai_call_logs` (`id` text,`user_id` text,`endpoint` text,`method` text,`model` text,`channel_id` text,`channel_name` text,`status` integer,`duration_ms` integer,`credits` integer,`request_body` text,`response_body` text,`error` text,`created_at` text,PRIMARY KEY (`id`));
CREATE TABLE IF NOT EXISTS `assets` (`id` text,`title` text,`type` text,`cover_url` text,`tags` text,`category` text,`description` text,`content` text,`url` text,`created_at` text,`updated_at` text,PRIMARY KEY (`id`));
CREATE TABLE IF NOT EXISTS `canvas_audio_tasks` (`id` text,`user_id` text,`user_display_name` text,`source` text,`source_id` text,`node_id` text,`model` text,`channel_id` text,`user_channel_id` text,`channel_name` text,`status` text,`progress` integer,`prompt` text,`endpoint` text,`content_type` text,`request_body` text,`response_body` text,`error` text,`error_detail` text,`audio_url` text,`storage_key` text,`mime_type` text,`bytes` integer,`created_at` text,`updated_at` text,`started_at` text,`completed_at` text,PRIMARY KEY (`id`));
CREATE TABLE IF NOT EXISTS `canvas_image_tasks` (`id` text,`user_id` text,`user_display_name` text,`source` text,`source_id` text,`node_id` text,`model` text,`channel_id` text,`user_channel_id` text,`channel_name` text,`status` text,`progress` integer,`prompt` text,`generation_type` text,`endpoint` text,`content_type` text,`request_body` text,`response_body` text,`error` text,`error_detail` text,`image_url` text,`image_urls` text,`storage_key` text,`width` integer,`height` integer,`mime_type` text,`bytes` integer,`created_at` text,`updated_at` text,`started_at` text,`completed_at` text,PRIMARY KEY (`id`));
CREATE TABLE IF NOT EXISTS `canvas_projects` (`user_id` text,`id` text,`project_data` text,`created_at` text,`updated_at` text,`deleted_at` text NOT NULL DEFAULT "",PRIMARY KEY (`user_id`,`id`));
CREATE TABLE IF NOT EXISTS `creative_workflows` (`id` text,`owner_user_id` text,`scope` text,`name` text,`category` text,`description` text,`data` text,`created_at` text,`updated_at` text,`last_run_at` text,PRIMARY KEY (`id`));
CREATE TABLE IF NOT EXISTS `credit_logs` (`id` text,`user_id` text,`type` text,`amount` integer,`balance` integer,`related_id` text,`remark` text,`extra` text,`created_at` text,PRIMARY KEY (`id`));
CREATE TABLE IF NOT EXISTS `image_generation_logs` (`id` text,`user_id` text,`task_id` text,`image_id` text,`status` text,`payload_json` text,`created_at` text,`updated_at` text,`deleted_at` text,PRIMARY KEY (`id`));
CREATE TABLE IF NOT EXISTS `prompt_catalogs` (`id` text,`title` text,`cover_url` text,`tags` text,`category` text,`github_url` text,`preview` text,`content_hash` text,`created_at` text,`updated_at` text,PRIMARY KEY (`id`));
CREATE TABLE IF NOT EXISTS `prompt_categories` (`category` text,`name` text,`description` text,`github_url` text,`source_type` text,`path_or_url` text,`remote` numeric,`enabled` numeric DEFAULT true,`updated_at` text, `index_updated_at` text,PRIMARY KEY (`category`));
CREATE TABLE IF NOT EXISTS `prompt_favorites` (`id` text,`title` text,`cover_url` text,`prompt` text,`tags` text,`category` text,`preview` text,`created_at` text,`updated_at` text,`source_url` text,`saved_at` text,PRIMARY KEY (`id`));
CREATE TABLE IF NOT EXISTS `prompts` (`id` text,`title` text,`cover_url` text,`prompt` text,`tags` text,`category` text,`preview` text,`created_at` text,`updated_at` text,PRIMARY KEY (`id`));
CREATE TABLE IF NOT EXISTS `settings` (`key` text,`value` text,`created_at` text,`updated_at` text,PRIMARY KEY (`key`));
CREATE TABLE IF NOT EXISTS `storage_objects` (`id` text,`provider_id` text,`bucket` text,`object_key` text,`public_url` text,`mime_type` text,`bytes` integer,`width` integer,`height` integer,`sha256` text,`direct` numeric,`created_by` text,`created_at` text,`deleted_at` text,PRIMARY KEY (`id`));
CREATE TABLE IF NOT EXISTS `user_configs` (`user_id` text,`model_config` text,`storage_provider` text,`image_history` text,`asset_data` text,`created_at` text,`updated_at` text,PRIMARY KEY (`user_id`));
CREATE TABLE IF NOT EXISTS `users` (`id` text,`username` text,`password` text,`email` text,`display_name` text,`avatar_url` text,`role` text,`credits` integer,`aff_code` text,`aff_count` integer,`inviter_id` text,`github_id` text,`linux_do_id` text,`wechat_id` text,`status` text,`last_login_at` text,`extra` text,`created_at` text,`updated_at` text,PRIMARY KEY (`id`));
CREATE TABLE IF NOT EXISTS `video_generation_logs` (`id` text,`user_id` text,`task_id` text,`video_id` text,`status` text,`payload_json` text,`created_at` text,`updated_at` text,`deleted_at` text,PRIMARY KEY (`id`));
CREATE TABLE IF NOT EXISTS `video_tasks` (`id` text,`user_id` text,`user_display_name` text,`model` text,`channel_id` text,`user_channel_id` text,`channel_name` text,`source` text,`source_id` text,`upstream_task_id` text,`upstream_video_id` text,`status` text,`progress` integer,`seconds` text,`size` text,`video_url` text,`error` text,`error_detail` text,`request_body` text,`response_body` text,`last_response` text,`credits` integer,`created_at` text,`updated_at` text,`started_at` text,`completed_at` text,`last_polled_at` text,PRIMARY KEY (`id`));
CREATE INDEX IF NOT EXISTS idx_agent_operation_project
                ON agent_operation_requests(project_id, created_at);
CREATE INDEX IF NOT EXISTS `idx_ai_call_logs_channel_id` ON `ai_call_logs`(`channel_id`);
CREATE INDEX IF NOT EXISTS `idx_ai_call_logs_created_at` ON `ai_call_logs`(`created_at`);
CREATE INDEX IF NOT EXISTS `idx_ai_call_logs_endpoint` ON `ai_call_logs`(`endpoint`);
CREATE INDEX IF NOT EXISTS `idx_ai_call_logs_model` ON `ai_call_logs`(`model`);
CREATE INDEX IF NOT EXISTS `idx_ai_call_logs_status` ON `ai_call_logs`(`status`);
CREATE INDEX IF NOT EXISTS `idx_ai_call_logs_user_id` ON `ai_call_logs`(`user_id`);
CREATE INDEX IF NOT EXISTS `idx_canvas_audio_tasks_user_source_node` ON `canvas_audio_tasks`(`user_id`,`source`,`source_id`,`node_id`);
CREATE INDEX IF NOT EXISTS `idx_canvas_image_tasks_user_source_node` ON `canvas_image_tasks`(`user_id`,`source`,`source_id`,`node_id`);
CREATE INDEX IF NOT EXISTS `idx_canvas_projects_deleted_at` ON `canvas_projects`(`deleted_at`);
CREATE INDEX IF NOT EXISTS `idx_canvas_projects_user_deleted_updated` ON `canvas_projects`(`user_id`,`deleted_at`,`updated_at`);
CREATE INDEX IF NOT EXISTS `idx_creative_workflows_category` ON `creative_workflows`(`category`);
CREATE INDEX IF NOT EXISTS `idx_creative_workflows_name` ON `creative_workflows`(`name`);
CREATE INDEX IF NOT EXISTS `idx_creative_workflows_owner_user_id` ON `creative_workflows`(`owner_user_id`);
CREATE INDEX IF NOT EXISTS `idx_creative_workflows_scope` ON `creative_workflows`(`scope`);
CREATE INDEX IF NOT EXISTS `idx_credit_logs_user_id` ON `credit_logs`(`user_id`);
CREATE INDEX IF NOT EXISTS `idx_image_generation_logs_created_at` ON `image_generation_logs`(`created_at`);
CREATE INDEX IF NOT EXISTS `idx_image_generation_logs_deleted_at` ON `image_generation_logs`(`deleted_at`);
CREATE INDEX IF NOT EXISTS `idx_image_generation_logs_image_id` ON `image_generation_logs`(`image_id`);
CREATE INDEX IF NOT EXISTS `idx_image_generation_logs_status` ON `image_generation_logs`(`status`);
CREATE INDEX IF NOT EXISTS `idx_image_generation_logs_task_id` ON `image_generation_logs`(`task_id`);
CREATE INDEX IF NOT EXISTS `idx_image_generation_logs_updated_at` ON `image_generation_logs`(`updated_at`);
CREATE INDEX IF NOT EXISTS `idx_image_generation_logs_user_deleted_created` ON `image_generation_logs`(`user_id`,`deleted_at`,`created_at`);
CREATE INDEX IF NOT EXISTS `idx_image_generation_logs_user_id` ON `image_generation_logs`(`user_id`);
CREATE INDEX IF NOT EXISTS `idx_prompt_catalogs_category` ON `prompt_catalogs`(`category`);
CREATE INDEX IF NOT EXISTS `idx_prompt_favorites_category` ON `prompt_favorites`(`category`);
CREATE INDEX IF NOT EXISTS `idx_prompts_category` ON `prompts`(`category`);
CREATE INDEX IF NOT EXISTS `idx_storage_objects_created_by` ON `storage_objects`(`created_by`);
CREATE UNIQUE INDEX IF NOT EXISTS `idx_storage_objects_object_key` ON `storage_objects`(`object_key`);
CREATE INDEX IF NOT EXISTS `idx_storage_objects_provider_id` ON `storage_objects`(`provider_id`);
CREATE UNIQUE INDEX IF NOT EXISTS `idx_users_aff_code` ON `users`(`aff_code`);
CREATE INDEX IF NOT EXISTS `idx_users_linux_do_id` ON `users`(`linux_do_id`);
CREATE UNIQUE INDEX IF NOT EXISTS `idx_users_username` ON `users`(`username`);
CREATE INDEX IF NOT EXISTS `idx_video_generation_logs_created_at` ON `video_generation_logs`(`created_at`);
CREATE INDEX IF NOT EXISTS `idx_video_generation_logs_deleted_at` ON `video_generation_logs`(`deleted_at`);
CREATE INDEX IF NOT EXISTS `idx_video_generation_logs_status` ON `video_generation_logs`(`status`);
CREATE INDEX IF NOT EXISTS `idx_video_generation_logs_task_id` ON `video_generation_logs`(`task_id`);
CREATE INDEX IF NOT EXISTS `idx_video_generation_logs_updated_at` ON `video_generation_logs`(`updated_at`);
CREATE INDEX IF NOT EXISTS `idx_video_generation_logs_user_deleted_created` ON `video_generation_logs`(`user_id`,`deleted_at`,`created_at`);
CREATE INDEX IF NOT EXISTS `idx_video_generation_logs_user_id` ON `video_generation_logs`(`user_id`);
CREATE INDEX IF NOT EXISTS `idx_video_generation_logs_video_id` ON `video_generation_logs`(`video_id`);
CREATE INDEX IF NOT EXISTS `idx_video_tasks_channel_id` ON `video_tasks`(`channel_id`);
CREATE INDEX IF NOT EXISTS `idx_video_tasks_created_at` ON `video_tasks`(`created_at`);
CREATE INDEX IF NOT EXISTS `idx_video_tasks_last_polled_at` ON `video_tasks`(`last_polled_at`);
CREATE INDEX IF NOT EXISTS `idx_video_tasks_model` ON `video_tasks`(`model`);
CREATE INDEX IF NOT EXISTS `idx_video_tasks_source` ON `video_tasks`(`source`);
CREATE INDEX IF NOT EXISTS `idx_video_tasks_source_id` ON `video_tasks`(`source_id`);
CREATE INDEX IF NOT EXISTS `idx_video_tasks_status_created_at` ON `video_tasks`(`status`,`created_at`);
CREATE INDEX IF NOT EXISTS `idx_video_tasks_updated_at` ON `video_tasks`(`updated_at`);
CREATE INDEX IF NOT EXISTS `idx_video_tasks_upstream_task_id` ON `video_tasks`(`upstream_task_id`);
CREATE INDEX IF NOT EXISTS `idx_video_tasks_upstream_video_id` ON `video_tasks`(`upstream_video_id`);
CREATE INDEX IF NOT EXISTS `idx_video_tasks_user_channel_id` ON `video_tasks`(`user_channel_id`);
CREATE INDEX IF NOT EXISTS `idx_video_tasks_user_id` ON `video_tasks`(`user_id`);
