use std::{collections::{BTreeMap, BTreeSet}, io::Write, path::{Path, PathBuf}, sync::Mutex};

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

pub(crate) fn default_workflow_root() -> Result<PathBuf, String> {
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

type WorkspaceRegistry = BTreeMap<String, String>;
static REGISTRY_LOCK: Mutex<()> = Mutex::new(());
fn registry_path() -> Result<PathBuf,String> {
    let home=std::env::var_os("HOME").ok_or("无法找到用户目录")?;
    Ok(PathBuf::from(home).join("Library/Application Support/com.chenyuxiaojin.infinitecanvas/project-workspaces.json"))
}
fn read_registry() -> Result<WorkspaceRegistry,String> {
    match std::fs::read(registry_path()?) {
        Ok(bytes)=>serde_json::from_slice(&bytes).map_err(|_|"片子目录登记损坏，原文件已保留，请修复后继续".into()),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(BTreeMap::new()),
        Err(e)=>Err(e.to_string())
    }
}
fn write_registry(registry:&WorkspaceRegistry)->Result<(),String> {
    let path=registry_path()?;let parent=path.parent().unwrap();
    std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;
    let mut nonce=[0u8;8];getrandom::fill(&mut nonce).map_err(|e|e.to_string())?;
    let temporary=parent.join(format!(".project-workspaces-{}.tmp",u64::from_ne_bytes(nonce)));
    let result=(||{
        let mut options=std::fs::OpenOptions::new();options.write(true).create_new(true);
        #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
        let mut file=options.open(&temporary).map_err(|e|e.to_string())?;
        file.write_all(&serde_json::to_vec_pretty(registry).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        file.sync_all().map_err(|e|e.to_string())?;
        std::fs::rename(&temporary,&path).map_err(|e|e.to_string())
    })();
    let _=std::fs::remove_file(temporary);result
}
fn binding_paths(root:&Path, registry:&WorkspaceRegistry, project_id:&str)->Result<Vec<PathBuf>,String> {
    let mut directories=direct_film_directories(root).into_iter().collect::<BTreeSet<_>>();
    directories.extend(registry.values().map(PathBuf::from));
    let mut matches=BTreeSet::new();
    for path in directories {
        let registered=registry.get(project_id).is_some_and(|value|Path::new(value)==path);
        match load_project_binding(&path) {
            Ok(binding) if binding.project_id==project_id=>{
                let canonical=path.canonicalize().map_err(|_|format!("片子目录已失效：{}",path.display()))?;
                let recorded=PathBuf::from(binding.project_directory).canonicalize().ok();
                if recorded.as_ref()!=Some(&canonical) {return Err(format!("片子目录位置已变化，请在终端重新选择：{}",path.display()));}
                matches.insert(canonical);
            },
            Ok(_) if registered=>return Err(format!("登记目录已绑定另一张画布：{}",path.display())),
            Err(_) if registered=>return Err(format!("目录或绑定文件已失效，请在终端重新选择：{}",path.display())),
            _=>{}
        }
    }
    Ok(matches.into_iter().collect())
}
fn unique_binding(paths:Vec<PathBuf>)->Result<Option<PathBuf>,String> {
    match paths.len(){0=>Ok(None),1=>Ok(paths.into_iter().next()),_=>Err(format!("多个目录绑定当前画布，需核对这些目录后保留一个绑定：{}",paths.iter().map(|p|p.display().to_string()).collect::<Vec<_>>().join("；")))}
}
pub(crate) fn bound_canvas_workspace(project_id:&str)->Result<PathBuf,String> {
    unique_binding(binding_paths(&default_workflow_root()?,&read_registry()?,project_id)?)?.ok_or_else(||"当前画布未绑定片子目录，请先在终端选择目录".into())
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct CanvasBindingInfo { project_id:String, state:String, directories:Vec<String>, message:String }
#[tauri::command]
pub fn inspect_canvas_project_bindings(project_ids:Vec<String>)->Result<Vec<CanvasBindingInfo>,String> {
    let root=default_workflow_root()?;let registry=read_registry()?;
    Ok(project_ids.into_iter().map(|project_id|{
        let (state,paths,message)=match binding_paths(&root,&registry,&project_id){
            Ok(paths) if paths.len()==1=>("bound",paths,"已绑定片子目录".into()),
            Ok(paths) if paths.len()>1=>("duplicate",paths,"多个目录绑定同一画布，请核对目录绑定".into()),
            Ok(paths)=>("unbound",paths,"普通画布 · 未绑定片子目录".into()),
            Err(error)=>("invalid",vec![],error)
        };
        CanvasBindingInfo{project_id,state:state.into(),directories:paths.iter().map(|p|p.display().to_string()).collect(),message}
    }).collect())
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
    let _guard=REGISTRY_LOCK.lock().map_err(|_|"目录登记正忙，请重试")?;
    let root = default_workflow_root()?;
    if let Some(path)=unique_binding(binding_paths(&root,&read_registry()?,&project_id)?)? {
        return Ok(configure_workspace(&path,&project_id,&project_title,"saved_binding"));
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
    let _guard=REGISTRY_LOCK.lock().map_err(|_|"目录登记正忙，请重试")?;
    let path=PathBuf::from(project_directory).canonicalize().map_err(|_|"选择的片子目录不存在")?;
    if !path.is_dir(){return Err("选择的片子目录不存在".into());}
    let mut registry=read_registry()?;
    let binding_file=path.join(".infinite-canvas/project.json");
    if binding_file.exists(){
        let binding=load_project_binding(&path).map_err(|_|"该目录绑定文件损坏，原文件已保留，不能覆盖")?;
        if binding.project_id!=project_id {return Err(format!("这个目录已有另一张画布（{}），请打开原画布或选择空目录",binding.project_id));}
    }
    let mut candidates=direct_film_directories(&default_workflow_root()?).into_iter().collect::<BTreeSet<_>>();
    candidates.extend(registry.values().map(PathBuf::from));
    if candidates.iter().filter(|p|p.canonicalize().ok().as_ref()!=Some(&path)).any(|p|load_project_binding(p).is_ok_and(|b|b.project_id==project_id)){return Err("当前画布已有其他目录绑定，请使用原目录，或先核对并解除旧绑定".into());}
    let workspace=configure_workspace(&path,&project_id,&project_title,"selected_folder");
    if let Some(error)=&workspace.configuration_error{return Err(error.clone());}
    registry.insert(project_id,path.to_string_lossy().into_owned());
    write_registry(&registry)?;
    Ok(workspace)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bind(path:&Path,id:&str){
        std::fs::create_dir_all(path.join(".infinite-canvas")).unwrap();
        let binding=local_agent_adapter::ProjectBinding::new(id,"same title",path).unwrap();
        std::fs::write(path.join(".infinite-canvas/project.json"),serde_json::to_vec(&binding).unwrap()).unwrap();
    }
    #[test]
    fn titles_never_bind_and_duplicate_ids_are_rejected(){
        let root=tempfile::tempdir().unwrap();let first=root.path().join("案例4");let second=root.path().join("案例4-copy");
        std::fs::create_dir(&first).unwrap();
        assert!(binding_paths(root.path(),&BTreeMap::new(),"case4").unwrap().is_empty());
        bind(&first,"case4");bind(&second,"other-id");
        assert_eq!(unique_binding(binding_paths(root.path(),&BTreeMap::new(),"case4").unwrap()).unwrap(),Some(first.canonicalize().unwrap()));
        bind(&second,"case4");assert!(unique_binding(binding_paths(root.path(),&BTreeMap::new(),"case4").unwrap()).is_err());
    }
    #[test]
    fn explicit_outside_directory_survives_restart_and_invalid_binding_is_visible(){
        let root=tempfile::tempdir().unwrap();let outside=tempfile::tempdir().unwrap();bind(outside.path(),"film");
        let registry:WorkspaceRegistry=BTreeMap::from([("film".into(),outside.path().display().to_string())]);
        let bytes=serde_json::to_vec(&registry).unwrap();let registry=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(binding_paths(root.path(),&registry,"film").unwrap(),vec![outside.path().canonicalize().unwrap()]);
        std::fs::write(outside.path().join(".infinite-canvas/project.json"),b"broken").unwrap();
        assert!(binding_paths(root.path(),&registry,"film").is_err());
        assert_eq!(std::fs::read(outside.path().join(".infinite-canvas/project.json")).unwrap(),b"broken");
    }
}
