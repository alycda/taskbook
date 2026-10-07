use std::io::{self, Write};

use base64::Engine;
use colored::Colorize;

use crate::api_client::{ApiClient, LoginRequest, RegisterRequest};
use crate::config::{Config, DittoConnect, SyncBackend};
use crate::credentials::{Credentials, DittoCredentials};
use crate::error::{Result, TaskbookError};

fn prompt(message: &str) -> Result<String> {
    print!("{}", message);
    io::stdout()
        .flush()
        .map_err(|e| TaskbookError::General(format!("failed to flush stdout: {e}")))?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|e| TaskbookError::General(format!("failed to read input: {e}")))?;
    Ok(input.trim().to_string())
}

fn prompt_password(message: &str) -> Result<String> {
    rpassword::prompt_password(message)
        .map_err(|e| TaskbookError::General(format!("failed to read password: {e}")))
}

/// Register a new account on the server (interactive).
pub fn register(
    server_url: Option<&str>,
    username: Option<&str>,
    email: Option<&str>,
    password: Option<&str>,
) -> Result<()> {
    println!("{}", "Register new account".bold());
    println!();

    let server = match server_url {
        Some(s) => s.to_string(),
        None => prompt("Server URL: ")?,
    };

    let user = match username {
        Some(u) => u.to_string(),
        None => prompt("Username: ")?,
    };

    let mail = match email {
        Some(e) => e.to_string(),
        None => prompt("Email: ")?,
    };

    let pass = match password {
        Some(p) => p.to_string(),
        None => {
            let p1 = prompt_password("Password: ")?;
            let p2 = prompt_password("Confirm password: ")?;
            if p1 != p2 {
                return Err(TaskbookError::Auth("passwords do not match".to_string()));
            }
            p1
        }
    };

    let client = ApiClient::new(&server, None);

    let resp = client.register(&RegisterRequest {
        username: user,
        email: mail,
        password: pass,
    })?;

    // Generate encryption key locally
    let key = taskbook_common::encryption::generate_key();
    let key_b64 = base64::engine::general_purpose::STANDARD.encode(key);

    // Save credentials
    let creds = Credentials {
        server_url: server.clone(),
        token: resp.token,
        encryption_key: key_b64.clone(),
    };
    creds.save()?;

    // Enable sync in config
    let mut config = Config::load_or_default();
    config.enable_sync(&server)?;

    println!();
    println!("{}", "Registration successful!".green().bold());
    println!("{}", "Sync is now enabled.".green());
    println!();
    println!(
        "{}",
        "Your encryption key (save this — it cannot be recovered):".yellow()
    );
    println!();
    println!("  {}", key_b64.bright_white().bold());
    println!();

    Ok(())
}

/// Log in to an existing account (interactive).
pub fn login(
    server_url: Option<&str>,
    username: Option<&str>,
    password: Option<&str>,
    encryption_key: Option<&str>,
) -> Result<()> {
    println!("{}", "Login".bold());
    println!();

    let server = match server_url {
        Some(s) => s.to_string(),
        None => prompt("Server URL: ")?,
    };

    let user = match username {
        Some(u) => u.to_string(),
        None => prompt("Username: ")?,
    };

    let pass = match password {
        Some(p) => p.to_string(),
        None => prompt_password("Password: ")?,
    };

    let key = match encryption_key {
        Some(k) => k.to_string(),
        None => prompt("Encryption key: ")?,
    };

    let client = ApiClient::new(&server, None);

    let resp = client.login(&LoginRequest {
        username: user,
        password: pass,
    })?;

    let creds = Credentials {
        server_url: server.clone(),
        token: resp.token,
        encryption_key: key,
    };
    creds.save()?;

    // Enable sync in config
    let mut config = Config::load_or_default();
    config.enable_sync(&server)?;

    println!();
    println!("{}", "Login successful!".green().bold());
    println!("{}", "Sync is now enabled.".green());

    Ok(())
}

/// Log out and delete credentials.
pub fn logout() -> Result<()> {
    if let Some(creds) = Credentials::load()? {
        let client = ApiClient::new(&creds.server_url, Some(&creds.token));
        // Best-effort server logout
        let _ = client.logout();
    }

    Credentials::delete()?;

    // Disable sync in config
    let mut config = Config::load_or_default();
    config.disable_sync()?;

    println!("{}", "Logged out.".green());
    println!("{}", "Sync disabled, using local storage.".dimmed());

    Ok(())
}

/// Show current sync status.
pub fn status() -> Result<()> {
    let config = Config::load_or_default();

    if config.sync.enabled && config.sync.backend == SyncBackend::Ditto {
        return ditto_status(&config);
    }

    if config.sync.enabled {
        println!("Mode:   {}", "remote".green().bold());
        println!("Server: {}", config.sync.server_url);
    } else {
        println!("Mode:   {}", "local".yellow().bold());
    }

    match Credentials::load()? {
        Some(creds) => {
            println!("Credentials: {}", "saved".green());
            println!("Server URL:  {}", creds.server_url);

            if config.sync.enabled {
                let client = ApiClient::new(&creds.server_url, Some(&creds.token));
                match client.get_me() {
                    Ok(me) => {
                        println!("Session:     {}", "valid".green());
                        println!("Logged in as: {} ({})", me.username, me.email);
                    }
                    Err(e) => {
                        println!("Session:     {}", "invalid".red());
                        println!("Error:       {}", e);
                    }
                }
            }
        }
        None => {
            println!("Credentials: {}", "none".dimmed());
        }
    }

    Ok(())
}

fn ditto_status(config: &Config) -> Result<()> {
    let ditto = &config.ditto;
    println!("Mode:    {}", "ditto".green().bold());
    if !cfg!(feature = "ditto") {
        println!(
            "{}",
            "This build of tb has no Ditto support; rebuild with `--features ditto`."
                .red()
                .bold()
        );
    }
    println!(
        "App ID:  {}",
        if ditto.app_id.is_empty() {
            "(unset)".red().to_string()
        } else {
            ditto.app_id.clone()
        }
    );
    match ditto.connect {
        DittoConnect::Peers => println!("Connect: peers (LAN / P2P mesh)"),
        DittoConnect::Server => println!(
            "Connect: server {}",
            ditto.url.as_deref().unwrap_or("(url unset)")
        ),
    }
    println!("Encrypt: {}", if ditto.encrypt { "on" } else { "off" });
    println!("Data:    {}", ditto.persistence_path().display());

    match DittoCredentials::load()? {
        Some(creds) => {
            println!("Credentials: {}", "saved".green());
            let yes_no = |present: bool| {
                if present {
                    "set".green()
                } else {
                    "missing".red()
                }
            };
            println!(
                "  encryption key: {}",
                yes_no(creds.encryption_key.is_some())
            );
            match ditto.connect {
                DittoConnect::Server => {
                    println!("  auth token:     {}", yes_no(creds.token.is_some()));
                }
                DittoConnect::Peers => {
                    println!(
                        "  license token:  {}",
                        yes_no(creds.license_token.is_some())
                    );
                }
            }
            if let Some(path) = &creds.private_key_path {
                println!("  private key:    {path}");
            }
        }
        None => {
            println!("Credentials: {} (run `tb --ditto-init`)", "none".dimmed());
        }
    }

    Ok(())
}

/// Set up secrets for the Ditto backend: generate (or import) the item
/// encryption key and, in `server` mode, store the auth token.
pub fn ditto_init(key: Option<&str>) -> Result<()> {
    println!("{}", "Ditto backend setup".bold());
    println!();

    let config = Config::load_or_default();
    let ditto = &config.ditto;
    let mut creds = DittoCredentials::load()?.unwrap_or_default();
    let mut changed = false;

    if let Some(key) = key {
        creds.encryption_key = Some(key.to_string());
        creds.encryption_key_bytes()?; // validate
        changed = true;
        println!("Imported encryption key.");
    } else if ditto.encrypt && creds.encryption_key.is_none() {
        let key = taskbook_common::encryption::generate_key();
        let encoded = base64::engine::general_purpose::STANDARD.encode(key);
        creds.encryption_key = Some(encoded.clone());
        changed = true;
        println!("Generated a new encryption key. Other devices need the same key");
        println!("(`tb --ditto-init --key <key>`); it cannot be recovered if lost:");
        println!();
        println!("  {}", encoded.bold());
        println!();
    }

    match ditto.connect {
        DittoConnect::Server if creds.token.is_none() => {
            let token = prompt_password("Ditto auth token (not echoed): ")?;
            if token.is_empty() {
                return Err(TaskbookError::Auth("token cannot be empty".to_string()));
            }
            creds.token = Some(token);
            changed = true;
        }
        DittoConnect::Peers if creds.license_token.is_none() => {
            println!(
                "Peers mode needs an offline license token (free) from https://portal.ditto.live."
            );
            let token = prompt("Ditto offline license token: ")?;
            if token.is_empty() {
                return Err(TaskbookError::Auth(
                    "license token cannot be empty".to_string(),
                ));
            }
            creds.license_token = Some(token);
            changed = true;
        }
        _ => {}
    }

    let path = DittoCredentials::path()?;
    if changed {
        creds.save()?;
        println!("{}", format!("Saved {}", path.display()).green());
    } else {
        println!(
            "Nothing to do; credentials already present at {}",
            path.display()
        );
    }

    if ditto.app_id.is_empty() {
        println!(
            "{}",
            format!(
                "Next: set ditto.appId (and sync.enabled = true, sync.backend = \"ditto\") in {}",
                Config::config_file_path().display()
            )
            .yellow()
        );
    } else if !(config.sync.enabled && config.sync.backend == SyncBackend::Ditto) {
        println!(
            "{}",
            format!(
                "Next: set sync.enabled = true and sync.backend = \"ditto\" in {}",
                Config::config_file_path().display()
            )
            .yellow()
        );
    }

    Ok(())
}
