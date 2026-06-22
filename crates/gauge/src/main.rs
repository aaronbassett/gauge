use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "gauge",
    about = "Gauge telemetry dashboard, MCP server, and admin CLI"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Manage Ed25519 keys for API authentication
    Keys {
        #[command(subcommand)]
        cmd: KeysCmd,
    },
    /// Authenticate against the gauge server and cache a token
    Login,
    /// Run a one-shot query (JSON QueryRequest as the argument)
    Query { request: String },
    /// Launch the dashboard TUI
    Tui,
    /// MCP server commands
    Mcp {
        #[command(subcommand)]
        cmd: McpCmd,
    },
    /// Show client/server status and a data overview
    Status {
        /// Emit machine-readable JSON instead of the human panel
        #[arg(long)]
        json: bool,
    },
    /// Print the gauge version and exit
    Version,
}

#[derive(Subcommand)]
enum KeysCmd {
    Generate {
        #[arg(long)]
        user_id: String,
    },
}

#[derive(Subcommand)]
enum McpCmd {
    /// Serve MCP over stdio
    Serve,
}

#[tokio::main]
async fn main() {
    // `--version` / `-V` print only the bare version, before clap dispatch.
    let raw: Vec<String> = std::env::args().collect();
    if raw.iter().skip(1).any(|a| a == "--version" || a == "-V") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let cli = Cli::parse();
    let result: Result<(), Box<dyn std::error::Error>> = match cli.cmd {
        Cmd::Keys {
            cmd: KeysCmd::Generate { user_id },
        } => gauge::keys::generate(&user_id)
            .map(|wire| {
                println!("Public key (register this in the server's users.toml):\n");
                println!("[[users]]");
                println!("user_id = \"{user_id}\"");
                println!("role = \"viewer\"");
                println!("public_key = \"{wire}\"");
            })
            .map_err(Into::into),
        Cmd::Login => {
            async {
                let cfg = gauge::config::ClientConfig::load()
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
                let api = gauge::api::ApiClient::from_config(&cfg);
                let cache = api
                    .login()
                    .await
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
                println!(
                    "logged in as {} (token expires at unix {})",
                    cache.user_id, cache.expires_at
                );
                Ok(())
            }
            .await
        }
        Cmd::Query { request } => {
            async {
                let cfg = gauge::config::ClientConfig::load()
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
                let api = gauge::api::ApiClient::from_config(&cfg);
                let out = gauge::query_cmd::run(&api, &request)
                    .await
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
                println!("{out}");
                Ok(())
            }
            .await
        }
        Cmd::Tui => {
            async {
                let cfg = gauge::config::ClientConfig::load()
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
                let api = gauge::api::ApiClient::from_config(&cfg);
                gauge::tui::run::run(api).await
            }
            .await
        }
        Cmd::Mcp { cmd: McpCmd::Serve } => {
            async {
                let cfg = gauge::config::ClientConfig::load()
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
                let api = std::sync::Arc::new(gauge::api::ApiClient::from_config(&cfg));
                gauge::mcp::server::serve(api).await
            }
            .await
        }
        Cmd::Status { json } => {
            let report = gauge::status::assemble_report(gauge::config::ClientConfig::load()).await;
            gauge::status::emit(&report, json);
            let code = report.overall.exit_code();
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        Cmd::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
