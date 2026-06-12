use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "gauge", about = "Gauge telemetry dashboard, MCP server, and admin CLI")]
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
    let cli = Cli::parse();
    let result: Result<(), Box<dyn std::error::Error>> = match cli.cmd {
        Cmd::Keys { cmd: KeysCmd::Generate { user_id } } => {
            gauge::keys::generate(&user_id).map(|wire| {
                println!("Public key (register this in the server's users.toml):\n");
                println!("[[users]]");
                println!("user_id = \"{user_id}\"");
                println!("role = \"viewer\"");
                println!("public_key = \"{wire}\"");
            }).map_err(Into::into)
        }
        Cmd::Login => todo_stub("login", ""),
        Cmd::Query { request } => todo_stub("query", &request),
        Cmd::Tui => todo_stub("tui", ""),
        Cmd::Mcp { cmd: McpCmd::Serve } => todo_stub("mcp serve", ""),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn todo_stub(name: &str, _arg: &str) -> Result<(), Box<dyn std::error::Error>> {
    Err(format!("`gauge {name}` is not implemented yet (see implementation plan)").into())
}
