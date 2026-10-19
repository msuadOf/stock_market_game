use server::deployment::{deployment_router_with_manager, resolve_web_root, DeploymentOptions, Services, DEPLOYMENT_HELP};
use server::init_tracing;

#[tokio::main]
async fn main() {
    init_tracing();
    if let Err(error) = run().await {
        eprintln!("server startup failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let options = DeploymentOptions::parse(
        std::env::args_os().skip(1),
        std::env::var("STOCK_MARKET_GAME_SERVER_BIND_ADDR"),
    )?;
    if options.help {
        println!("{DEPLOYMENT_HELP}");
        return Ok(());
    }
    let web_root = if options.services.includes_web_ui() {
        Some(resolve_web_root(
            options.web_root.as_deref(),
            &std::env::current_exe()?,
        )?)
    } else {
        None
    };
    let manager = if options.services == Services::WebUi {
        None
    } else {
        let database = native_store::NativeDatabase::open(&options.database)?;
        let manager = server::SessionManager::with_database(database.clone());
        if options.new_market {
            tracing::info!("明确跳过启动存档，等待创建新市场；既有日终档、账号与控制授权保留");
        } else if let Some(slot_id) = &options.archive_slot {
            if manager.resume_session(slot_id)?.is_none() {
                return Err(format!("明确选择的日终存档槽 {slot_id} 不存在；使用其他 --archive-slot 或 --new-market").into());
            }
            database.select(slot_id).map_err(|error| format!("市场已读档，但启动槽选择保存失败；旧选择保留，下次启动可再次指定 --archive-slot {slot_id}：{error}"))?;
        } else {
            manager.resume_selected_session()?;
        }
        Some(manager)
    };
    let router = deployment_router_with_manager(options.services, web_root.as_deref(), manager)?;
    let listener = tokio::net::TcpListener::bind(&options.bind_addr).await.map_err(|error| {
        std::io::Error::new(error.kind(), format!("cannot bind {}: {error}; check --bind / STOCK_MARKET_GAME_SERVER_BIND_ADDR and port availability", options.bind_addr))
    })?;
    tracing::info!(addr = %listener.local_addr()?, services = ?options.services, "HTTP/WS service starting");
    axum::serve(listener, router.into_make_service()).await?;
    Ok(())
}
