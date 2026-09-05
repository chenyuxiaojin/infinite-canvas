mod apimart;
mod apimart_refs;
mod assets;
mod db;
mod direct;
mod kie;
mod kie_advanced;
mod media;
mod prompt_sources;
mod prompts;
mod query;
mod routes;
mod scheduler;
mod settings;

#[tokio::main]
async fn main() {
    if let Err(error) = start().await {
        eprintln!("Rust local API startup failed: {error}");
        std::process::exit(1);
    }
}

async fn start() -> db::ApiResult<()> {
    let database = std::env::var("DATABASE_DSN")
        .map(std::path::PathBuf::from)
        .map_err(|_| "DATABASE_DSN is required")?;
    if !database.is_absolute() {
        return Err("DATABASE_DSN must be an absolute local path".into());
    }
    let host = std::env::var("BIND_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    if host != "127.0.0.1" {
        return Err("The personal desktop API only binds to 127.0.0.1".into());
    }
    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "3101".into())
        .parse::<u16>()
        .map_err(|_| "Invalid PORT")?;
    db::initialize(&database)?;
    prompts::initialize(&mut db::connect(&database)?)?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|e| e.to_string())?;
    let state = routes::AppState::new(database);
    let scheduler = if std::env::var("CANVAS_DISABLE_SCHEDULERS").as_deref() != Ok("1") {
        Some(tokio::spawn(scheduler::run(state.clone())))
    } else {
        None
    };
    eprintln!(
        "infinite-canvas-api runtime=rust version={} listening=127.0.0.1:{port}",
        env!("CARGO_PKG_VERSION")
    );
    let result = axum::serve(listener, routes::router(state))
        .with_graceful_shutdown(shutdown())
        .await
        .map_err(|e| e.to_string());
    if let Some(scheduler) = scheduler {
        scheduler.abort();
    }
    result
}
async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {_ = tokio::signal::ctrl_c()=>{},_ = signal.recv()=>{}};
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
