use server::deployment::{deployment_router, resolve_web_root, DeploymentOptions, Services};
use std::ffi::OsString;
use std::path::Path;

fn parse(
    arguments: &[&str],
    environment: Result<String, std::env::VarError>,
) -> Result<DeploymentOptions, server::deployment::DeploymentError> {
    DeploymentOptions::parse(arguments.iter().map(OsString::from), environment)
}

#[test]
fn default_mode_matches_compiled_capability() {
    let options = parse(&[], Err(std::env::VarError::NotPresent)).unwrap();
    assert_eq!(
        options.services,
        if cfg!(feature = "web-ui") {
            Services::All
        } else {
            Services::Server
        }
    );
    assert_eq!(options.bind_addr, "127.0.0.1:3000");
    assert!(options.web_root.is_none());
}

#[test]
fn cli_bind_overrides_environment_even_when_environment_is_invalid() {
    for environment in [
        Ok("192.0.2.10:4000".to_owned()),
        Ok("".to_owned()),
        Err(std::env::VarError::NotUnicode(OsString::from("invalid"))),
    ] {
        let options = parse(
            &["--bind", "localhost:3210", "--services", "server"],
            environment,
        )
        .unwrap();
        assert_eq!(options.bind_addr, "localhost:3210");
        assert_eq!(options.services, Services::Server);
    }
}

#[test]
fn environment_is_preserved_without_cli_bind() {
    assert_eq!(
        parse(&[], Ok("192.0.2.10:4000".to_owned()))
            .unwrap()
            .bind_addr,
        "192.0.2.10:4000"
    );
    for address in [
        "",
        "   ",
        "\t\n",
        "localhost",
        "localhost:65536",
        ":3000",
        "http://localhost:3000",
    ] {
        assert!(parse(&[], Ok(address.to_owned())).is_err(), "{address:?}");
    }
    assert!(parse(
        &[],
        Err(std::env::VarError::NotUnicode(OsString::from("invalid")))
    )
    .is_err());
}

#[test]
fn invalid_or_duplicate_arguments_are_rejected() {
    for arguments in [
        vec!["--services", "desktop"],
        vec!["--services"],
        vec!["--bind"],
        vec!["--web-root"],
        vec!["--web-root", ""],
        vec!["--typo"],
        vec!["--bind", "127.0.0.1:1", "--bind", "127.0.0.1:2"],
        vec!["--services", "server", "--services", "server"],
        vec!["--web-root", "one", "--web-root", "two"],
    ] {
        assert!(
            parse(&arguments, Err(std::env::VarError::NotPresent)).is_err(),
            "{arguments:?}"
        );
    }
}

#[test]
fn option_values_cannot_swallow_following_options() {
    for arguments in [
        vec!["--services", "server", "--web-root", "--typo"],
        vec!["--web-root", "--bind", "127.0.0.1:3000"],
    ] {
        let error = parse(&arguments, Err(std::env::VarError::NotPresent))
            .expect_err("--web-root must not consume another option as a path");
        assert!(error.to_string().contains("--web-root"));
    }
}

#[test]
fn server_mode_never_checks_web_resources() {
    let options = parse(
        &["--services", "server", "--web-root", "/missing/webui"],
        Err(std::env::VarError::NotPresent),
    )
    .unwrap();
    assert_eq!(
        options.web_root.as_deref(),
        Some(Path::new("/missing/webui"))
    );
    assert!(deployment_router(options.services, options.web_root.as_deref()).is_ok());
}

#[test]
fn default_web_root_is_executable_relative_not_cwd_relative() {
    assert_eq!(
        resolve_web_root(None, Path::new("/opt/game/server")).unwrap(),
        Path::new("/opt/game/webui")
    );
    assert_eq!(
        resolve_web_root(Some(Path::new("custom")), Path::new("/opt/game/server")).unwrap(),
        Path::new("custom")
    );
}

#[cfg(not(feature = "web-ui"))]
#[test]
fn server_only_build_rejects_static_modes_in_parser_and_router() {
    for mode in ["webui", "all"] {
        assert!(
            parse(&["--services", mode], Err(std::env::VarError::NotPresent))
                .unwrap_err()
                .to_string()
                .contains("web-ui")
        );
    }
    for mode in [Services::WebUi, Services::All] {
        assert!(deployment_router(mode, Some(Path::new("/missing/webui"))).is_err());
    }
}

#[cfg(feature = "web-ui")]
#[test]
fn feature_build_accepts_each_service_mode() {
    for (argument, mode) in [
        ("webui", Services::WebUi),
        ("server", Services::Server),
        ("all", Services::All),
    ] {
        assert_eq!(
            parse(
                &["--services", argument],
                Err(std::env::VarError::NotPresent)
            )
            .unwrap()
            .services,
            mode
        );
    }
}
