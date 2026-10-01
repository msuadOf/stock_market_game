use server::deployment::{deployment_router, resolve_web_root, DeploymentOptions, DEPLOYMENT_HELP};
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
    let router = deployment_router(options.services, web_root.as_deref())?;
    let listener = tokio::net::TcpListener::bind(&options.bind_addr).await.map_err(|error| {
        std::io::Error::new(error.kind(), format!("cannot bind {}: {error}; check --bind / STOCK_MARKET_GAME_SERVER_BIND_ADDR and port availability", options.bind_addr))
    })?;
    tracing::info!(addr = %listener.local_addr()?, services = ?options.services, "HTTP/WS service starting");
    axum::serve(listener, router.into_make_service()).await?;
    Ok(())
}
