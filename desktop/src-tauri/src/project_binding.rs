use std::path::{Path, PathBuf};

use local_agent_adapter::{load_project_binding, setup_project_binding};
use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, FilePath};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasProjectWorkspace {
    pub project_directory: String,
    pub configured: bool,
    pub source: String,
    pub configuration_error: Option<String>,
    pub agent_command: Option<String>,
}

fn default_workflow_root() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "无法找到用户目录".to_owned())?;
    Ok(home.join("项目").join("视频制作台").join("AI编导"))
}

fn bundled_agent_cli() -> Result<PathBuf, String> {
    let executable =
        std::env::current_exe().map_err(|error| format!("无法定位应用程序：{error}"))?;
    let directory = executable
        .parent()
        .ok_or_else(|| "无法定位应用程序目录".to_owned())?;
    let cli = directory.join("infinite-canvas");
    if cli.is_file() {
        Ok(cli)
    } else {
        Err(format!("画布 AI 命令不存在：{}", cli.display()))
    }
}

fn direct_film_directories(root: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| !name.starts_with('.'))
        })
        .collect()
}

fn normalized(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn case_number(value: &str) -> Option<char> {
    let lowered = value.to_lowercase();
    for marker in ["案例", "case"] {
        if let Some(index) = lowered.find(marker) {
            if let Some(number) = lowered[index + marker.len()..]
                .chars()
                .find(|character| character.is_ascii_digit())
            {
                return Some(number);
            }
        }
    }
    None
}

fn directory_score(path: &Path, project_id: &str, project_title: &str) -> usize {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return 0;
    };
    let directory = normalized(name);
    let title = normalized(project_title);
    let id = normalized(project_id);
    let mut score = 0;
    if directory.len() >= 4 && (title.contains(&directory) || directory.contains(&title)) {
        score += 100;
    }
    if let Some(number) = case_number(name) {
        if case_number(project_title) == Some(number) || case_number(project_id) == Some(number) {
            score += 60;
        }
    }
    for keyword in ["美甲", "国运", "克兰奇", "飞机稿"] {
        if directory.contains(keyword) && (title.contains(keyword) || id.contains(keyword)) {
            score += 40;
        }
    }
    score
}

fn find_workspace(root: &Path, project_id: &str, project_title: &str) -> Option<(PathBuf, String)> {
    let directories = direct_film_directories(root);
    if let Some(path) = directories.iter().find(|path| {
        load_project_binding(path).is_ok_and(|binding| binding.project_id == project_id)
    }) {
        return Some((path.clone(), "saved_binding".to_owned()));
    }
    directories
        .into_iter()
        .map(|path| {
            let score = directory_score(&path, project_id, project_title);
            (path, score)
        })
        .filter(|(_, score)| *score > 0)
        .max_by_key(|(_, score)| *score)
        .map(|(path, _)| (path, "matched_title".to_owned()))
}

fn configure_workspace(
    path: &Path,
    project_id: &str,
    project_title: &str,
    source: &str,
) -> CanvasProjectWorkspace {
    let cli = bundled_agent_cli();
    let agent_command = cli
        .as_ref()
        .ok()
        .map(|path| path.to_string_lossy().into_owned());
    let result = cli.and_then(|cli| {
        setup_project_binding(path, project_id, project_title, &cli)
            .map_err(|error| error.to_string())
    });
    CanvasProjectWorkspace {
        project_directory: path.to_string_lossy().into_owned(),
        configured: result.is_ok(),
        source: source.to_owned(),
        configuration_error: result.err(),
        agent_command,
    }
}

#[tauri::command]
pub fn resolve_canvas_project_workspace(
    project_id: String,
    project_title: String,
) -> Result<CanvasProjectWorkspace, String> {
    let root = default_workflow_root()?;
    if let Some((path, source)) = find_workspace(&root, &project_id, &project_title) {
        return Ok(configure_workspace(
            &path,
            &project_id,
            &project_title,
            &source,
        ));
    }
    Ok(CanvasProjectWorkspace {
        project_directory: root.to_string_lossy().into_owned(),
        configured: false,
        source: "workflow_root".to_owned(),
        configuration_error: Some("还没有为这个画布选择片子目录".to_owned()),
        agent_command: bundled_agent_cli()
            .ok()
            .map(|path| path.to_string_lossy().into_owned()),
    })
}

#[tauri::command]
pub fn select_film_directory(app: AppHandle) -> Result<Option<String>, String> {
    let root = default_workflow_root()?;
    let mut dialog = app.dialog().file().set_title("选择这部片子的目录");
    if root.is_dir() {
        dialog = dialog.set_directory(root);
    }
    let Some(selected) = dialog.blocking_pick_folder() else {
        return Ok(None);
    };
    match selected {
        FilePath::Path(path) => Ok(Some(path.to_string_lossy().into_owned())),
        FilePath::Url(_) => Err("片子目录必须是本机文件夹".to_owned()),
    }
}

#[tauri::command]
pub fn bind_canvas_project_directory(
    project_id: String,
    project_title: String,
    project_directory: String,
) -> Result<CanvasProjectWorkspace, String> {
    let path = PathBuf::from(project_directory);
    if !path.is_dir() {
        return Err("选择的片子目录不存在".to_owned());
    }
    let workspace = configure_workspace(&path, &project_id, &project_title, "selected_folder");
    if let Some(error) = &workspace.configuration_error {
        return Err(error.clone());
    }
    Ok(workspace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_case_directories_match_canvas_titles() {
        let root = tempfile::tempdir().unwrap();
        let case2 = root.path().join("案例2-美甲师日常");
        let case3 = root.path().join("案例3-国运末世");
        std::fs::create_dir(&case2).unwrap();
        std::fs::create_dir(&case3).unwrap();
        let (matched, source) =
            find_workspace(root.path(), "case2-mjs-ep01", "案例2-美甲师日常 EP01").unwrap();
        assert_eq!(matched, case2);
        assert_eq!(source, "matched_title");
    }
}
