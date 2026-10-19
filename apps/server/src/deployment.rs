use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Services {
    WebUi,
    Server,
    All,
}

#[derive(Debug)]
pub struct DeploymentOptions {
    pub services: Services,
    pub bind_addr: String,
    pub web_root: Option<PathBuf>,
    pub database: PathBuf,
    pub new_market: bool,
    pub archive_slot: Option<String>,
    pub help: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct DeploymentError(pub(crate) String);

impl DeploymentOptions {
    pub fn parse(
        arguments: impl IntoIterator<Item = OsString>,
        environment: Result<String, std::env::VarError>,
    ) -> Result<Self, DeploymentError> {
        let arguments: Vec<OsString> = arguments.into_iter().collect();
        let mut options = Self {
            services: if cfg!(feature = "web-ui") {
                Services::All
            } else {
                Services::Server
            },
            bind_addr: String::new(),
            web_root: None,
            database: PathBuf::from("data/stock-market-game.sqlite"),
            new_market: false,
            archive_slot: None,
            help: false,
        };
        if arguments.len() == 1 && (arguments[0] == "--help" || arguments[0] == "-h") {
            options.help = true;
            return Ok(options);
        }
        let mut arguments = arguments.into_iter();
        let mut services = None;
        let mut bind_addr = None;
        let mut database = None;
        while let Some(argument) = arguments.next() {
            let flag = argument.to_str().ok_or_else(|| {
                DeploymentError("CLI option must be valid Unicode; use --help".to_owned())
            })?;
            match flag {
                "--new-market" => {
                    if options.new_market || options.archive_slot.is_some() {
                        return Err(DeploymentError("--new-market 与 --archive-slot 只能明确选择一种启动方式，且不可重复".into()));
                    }
                    options.new_market = true;
                }
                "--services" | "--bind" | "--web-root" | "--database" | "--archive-slot" => {
                    if match flag {
                        "--services" => services.is_some(),
                        "--bind" => bind_addr.is_some(),
                        "--database" => database.is_some(),
                        "--archive-slot" => options.archive_slot.is_some() || options.new_market,
                        _ => options.web_root.is_some(),
                    } {
                        return Err(DeploymentError(format!(
                            "{flag} was specified more than once; use --help"
                        )));
                    }
                    let value = arguments.next().ok_or_else(|| {
                        DeploymentError(format!("{flag} requires a value; use --help"))
                    })?;
                    if value.to_str().is_some_and(|value| value.starts_with('-')) {
                        return Err(DeploymentError(format!(
                            "{flag} requires a value, not another option; use --help"
                        )));
                    }
                    if value.is_empty() {
                        return Err(DeploymentError(format!(
                            "{flag} must not be empty; use --help"
                        )));
                    }
                    if flag == "--archive-slot" {
                        let slot = value.into_string().map_err(|_| DeploymentError("--archive-slot 必须是 Unicode 槽身份".into()))?;
                        if slot.trim().is_empty() || slot.contains('\0') {
                            return Err(DeploymentError("--archive-slot 不可为空或含 NUL".into()));
                        }
                        options.archive_slot = Some(slot);
                        continue;
                    }
                    if flag == "--database" {
                        database = Some(PathBuf::from(value));
                        continue;
                    }
                    if flag == "--web-root" {
                        options.web_root = Some(PathBuf::from(value));
                        continue;
                    }
                    let value = value
                        .into_string()
                        .map_err(|_| DeploymentError(format!("{flag} must be valid Unicode")))?;
                    if flag == "--services" {
                        services = Some(match value.as_str() {
                            "webui" => Services::WebUi,
                            "server" => Services::Server,
                            "all" => Services::All,
                            _ => {
                                return Err(DeploymentError(
                                    "--services must be webui, server or all; use --help"
                                        .to_owned(),
                                ))
                            }
                        });
                    } else {
                        bind_addr = Some(value);
                    }
                }
                _ => {
                    return Err(DeploymentError(format!(
                        "unknown CLI option {flag:?}; use --help"
                    )))
                }
            }
        }
        if let Some(services) = services {
            options.services = services;
        }
        options.services.ensure_supported()?;
        if options.services == Services::WebUi && (options.new_market || options.archive_slot.is_some()) {
            return Err(DeploymentError("--new-market / --archive-slot 只适用于运行 Server 的服务模式".into()));
        }
        if let Some(database) = database {
            if database == Path::new(":memory:") {
                return Err(DeploymentError("--database 必须使用真实 SQLite 文件，不接受 :memory:".into()));
            }
            options.database = database;
        }
        options.bind_addr = match bind_addr {
            Some(address) => address,
            None => match environment {
                Ok(address) => address,
                Err(std::env::VarError::NotPresent) => "127.0.0.1:3000".to_owned(),
                Err(std::env::VarError::NotUnicode(_)) => {
                    return Err(DeploymentError(
                        "STOCK_MARKET_GAME_SERVER_BIND_ADDR must be valid Unicode".to_owned(),
                    ))
                }
            },
        };
        validate_bind_addr(&options.bind_addr)?;
        Ok(options)
    }
}

impl Services {
    pub fn includes_web_ui(self) -> bool {
        matches!(self, Self::WebUi | Self::All)
    }

    fn ensure_supported(self) -> Result<(), DeploymentError> {
        if self.includes_web_ui() && !cfg!(feature = "web-ui") {
            return Err(DeploymentError("--services webui/all requires a binary compiled with Cargo feature web-ui; use --services server or rebuild with --features web-ui".to_owned()));
        }
        Ok(())
    }
}

fn validate_bind_addr(address: &str) -> Result<(), DeploymentError> {
    if address.parse::<std::net::SocketAddr>().is_ok() {
        return Ok(());
    }
    if let Some((host, port)) = address.rsplit_once(':') {
        if !host.is_empty()
            && host.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
            })
            && !port.is_empty()
            && port.chars().all(|character| character.is_ascii_digit())
            && port.parse::<u16>().is_ok()
        {
            return Ok(());
        }
    }
    Err(DeploymentError("--bind / STOCK_MARKET_GAME_SERVER_BIND_ADDR must be host:port (IPv6: [address]:port); default is 127.0.0.1:3000".to_owned()))
}

pub fn resolve_web_root(
    explicit: Option<&Path>,
    executable: &Path,
) -> Result<PathBuf, DeploymentError> {
    match explicit {
        Some(path) => Ok(path.to_path_buf()),
        None => executable
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(|parent| parent.join("webui"))
            .ok_or_else(|| {
                DeploymentError(
                    "cannot locate executable parent for default web-root; specify --web-root PATH"
                        .to_owned(),
                )
            }),
    }
}

pub fn deployment_router(
    services: Services,
    web_root: Option<&Path>,
) -> Result<axum::Router, DeploymentError> {
    deployment_router_with_manager(services, web_root, Some(crate::SessionManager::default()))
}

pub fn deployment_router_with_manager(
    services: Services,
    web_root: Option<&Path>,
    manager: Option<crate::SessionManager>,
) -> Result<axum::Router, DeploymentError> {
    services.ensure_supported()?;
    if services == Services::Server {
        let manager = manager.ok_or_else(|| DeploymentError("Server 必须显式初始化 SQLite 与内存市场管理器".into()))?;
        return Ok(crate::app_router_with_manager(manager));
    }
    #[cfg(feature = "web-ui")]
    {
        let web_root = web_root.ok_or_else(|| DeploymentError("web-root is required for static services; resolve the executable-relative default before constructing the router".to_owned()))?;
        let static_router = crate::web_ui::static_router(web_root)?;
        Ok(match services {
            Services::WebUi => static_router,
            Services::All => crate::app_router_with_manager(manager.ok_or_else(|| DeploymentError("Server 必须显式初始化 SQLite 与内存市场管理器".into()))?).merge(static_router),
            Services::Server => unreachable!(),
        })
    }
    #[cfg(not(feature = "web-ui"))]
    {
        let _ = web_root;
        unreachable!("static service modes were rejected before router construction")
    }
}

pub const DEPLOYMENT_HELP: &str = "server [--services webui|server|all] [--bind host:port] [--web-root PATH] [--database PATH] [--new-market | --archive-slot ID]\n\nDefault services: server without Cargo feature web-ui; all with web-ui.\n--bind overrides STOCK_MARKET_GAME_SERVER_BIND_ADDR; default 127.0.0.1:3000.\n--web-root defaults to <executable directory>/webui and is checked only for webui/all.\n--database：Server 日终档与独立账号/授权 SQLite 文件，默认相对当前目录 data/stock-market-game.sqlite；不接受 :memory:，无需安装 SQLite 服务。\n--new-market：明确跳过启动存档，等待创建新市场；不删除既有档案、账号或授权。\n--archive-slot ID：明确选择并恢复该日终档，成功后记录启动槽；与 --new-market 互斥。\n未显式指定启动方式时只读已选档一次；已选槽删除或坏档明确报错，不自动选其他档。\nNative HTTP/WS; HTTPS/WSS requires an external TLS proxy. No public-deployment security certification is implied.";
