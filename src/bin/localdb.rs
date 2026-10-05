//! localdb - manage local Babelfish (T-SQL over TDS) instances from the command line,
//! in the spirit of SQL Server's `sqllocaldb`.
//!
//! It owns a small registry of named instances (`~/.pg0/localdb/<name>.json`: fixed ports,
//! credentials, sharing) and delegates the lifecycle to the `pg0-babelfish` binary, so the two
//! tools never disagree about where an instance lives. Instances appear in `pg0-babelfish`
//! as `bf-<name>`.

use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{self, Command};

use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const INSTANCE_PREFIX: &str = "bf-";
const DEFAULT_USER: &str = "postgres";
const DEFAULT_PASSWORD: &str = "postgres";

#[derive(Parser)]
#[command(name = "localdb", version, about = "Manage local Babelfish (T-SQL) instances, like sqllocaldb")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a named instance (ports are reserved now and stay stable)
    Create {
        name: String,
        /// Start the instance after creating it
        #[arg(short = 's', long)]
        start: bool,
        /// Listen on all interfaces instead of localhost only
        #[arg(long)]
        share: bool,
        /// Disable TLS (by default the TDS endpoint offers encryption with a self-signed certificate)
        #[arg(long)]
        no_tls: bool,
        /// Babelfish migration mode
        #[arg(long, default_value = "multi-db", value_parser = ["multi-db", "single-db"])]
        migration_mode: String,
        /// Login name (a PostgreSQL superuser; T-SQL clients use it as User Id)
        #[arg(short, long, default_value = DEFAULT_USER)]
        user: String,
        #[arg(short = 'P', long, default_value = DEFAULT_PASSWORD)]
        password: String,
        /// PostgreSQL port (default: first free port from 5432)
        #[arg(long)]
        port: Option<u16>,
        /// TDS / SQL Server port (default: first free port from 1433)
        #[arg(long)]
        tds_port: Option<u16>,
    },
    /// Start an instance (no-op if it is already running)
    Start { name: String },
    /// Stop an instance
    Stop {
        name: String,
        /// Seconds to wait for a clean shutdown before killing the server
        #[arg(short, long, default_value_t = 60)]
        timeout: u64,
    },
    /// Stop (if needed) and delete an instance together with its data
    Delete { name: String },
    /// List instance names, or show one instance in detail
    Info {
        name: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Print a connection string for scripts: `$(localdb connection dev)`
    Connection {
        name: String,
        #[arg(short, long, value_enum, default_value_t = Format::Ado)]
        format: Format,
    },
    /// Show the bundled Babelfish version
    Versions,
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    /// ADO.NET / Microsoft.Data.SqlClient
    Ado,
    Odbc,
    Jdbc,
    /// PostgreSQL URI (the same server, native protocol)
    Postgres,
}

#[derive(Serialize, Deserialize, Clone)]
struct Config {
    name: String,
    port: u16,
    tds_port: u16,
    migration_mode: String,
    username: String,
    password: String,
    share: bool,
    #[serde(default = "yes")]
    tls: bool,
}

fn yes() -> bool {
    true
}

fn fail(msg: impl AsRef<str>) -> ! {
    eprintln!("Error: {}", msg.as_ref());
    process::exit(1);
}

fn registry_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| fail("cannot determine the home directory"))
        .join(".pg0")
        .join("localdb")
}

fn config_path(name: &str) -> PathBuf {
    registry_dir().join(format!("{}.json", name))
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn load(name: &str) -> Config {
    let path = config_path(name);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|_| fail(format!("instance \"{}\" does not exist (create it with `localdb create {}`)", name, name)));
    serde_json::from_str(&text).unwrap_or_else(|e| fail(format!("{}: {}", path.display(), e)))
}

fn all_configs() -> Vec<Config> {
    let mut out: Vec<Config> = fs::read_dir(registry_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_str(&fs::read_to_string(e.path()).ok()?).ok())
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The `pg0-babelfish` binary: $LOCALDB_PG0, then next to this executable, then PATH.
fn pg0_binary() -> PathBuf {
    if let Some(p) = std::env::var_os("LOCALDB_PG0") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(sibling) = exe.parent().map(|d| d.join("pg0-babelfish")) {
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    PathBuf::from("pg0-babelfish")
}

fn run_pg0(args: &[String]) -> process::Output {
    Command::new(pg0_binary())
        .args(args)
        .output()
        .unwrap_or_else(|e| {
            fail(format!(
                "cannot run {} ({}). Install pg0-babelfish next to localdb or set LOCALDB_PG0.",
                pg0_binary().display(),
                e
            ))
        })
}

fn pg0_ok(args: &[String], what: &str) {
    let out = run_pg0(args);
    if !out.status.success() {
        fail(format!(
            "{} failed:\n{}{}",
            what,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// Live state from `pg0-babelfish info`. Null if the instance was never started.
fn state(name: &str) -> Value {
    let out = run_pg0(&s(&["info", "--name", &format!("{}{}", INSTANCE_PREFIX, name), "--output", "json"]));
    serde_json::from_slice(&out.stdout).unwrap_or(Value::Null)
}

fn is_running(name: &str) -> bool {
    state(name)["running"].as_bool().unwrap_or(false)
}

fn port_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// First free port from `start` that no other registered instance has reserved.
fn allocate(start: u16, taken: &[u16]) -> u16 {
    (start..u16::MAX)
        .find(|p| !taken.contains(p) && port_free(*p))
        .unwrap_or_else(|| fail("no free port available"))
}

fn start_instance(cfg: &Config) {
    if is_running(&cfg.name) {
        println!("Instance \"{}\" is already running.", cfg.name);
        return;
    }
    println!("Starting instance \"{}\" (the first start unpacks Babelfish and can take a minute)...", cfg.name);
    let mut args = s(&["start", "--babelfish", "--name"]);
    args.push(format!("{}{}", INSTANCE_PREFIX, cfg.name));
    args.extend(s(&["-p", &cfg.port.to_string(), "--tds-port", &cfg.tds_port.to_string()]));
    args.extend(s(&["--babelfish-migration-mode", &cfg.migration_mode, "-u", &cfg.username, "-P", &cfg.password]));
    if cfg.share {
        args.extend(s(&["-c", "listen_addresses=*", "-c", "babelfishpg_tds.listen_addresses=*"]));
    }
    if !cfg.tls {
        args.extend(s(&["-c", "ssl=off"]));
    }
    pg0_ok(&args, "start");
    println!("Instance \"{}\" started. TDS port {}.", cfg.name, cfg.tds_port);
}

fn connection_string(cfg: &Config, f: Format) -> String {
    let host = "127.0.0.1";
    let (u, p) = (&cfg.username, &cfg.password);
    // The certificate is self-signed, so TLS clients must trust it explicitly.
    let (ado, odbc, jdbc) = if cfg.tls {
        ("Encrypt=True", "Encrypt=yes", "encrypt=true")
    } else {
        ("Encrypt=False", "Encrypt=no", "encrypt=false")
    };
    match f {
        Format::Ado => format!(
            "Server={host},{};Database=master;User Id={u};Password={p};{ado};TrustServerCertificate=True",
            cfg.tds_port
        ),
        Format::Odbc => format!(
            "Driver={{ODBC Driver 18 for SQL Server}};Server={host},{};Database=master;UID={u};PWD={p};{odbc};TrustServerCertificate=yes",
            cfg.tds_port
        ),
        Format::Jdbc => format!(
            "jdbc:sqlserver://{host}:{};databaseName=master;user={u};password={p};{jdbc};trustServerCertificate=true",
            cfg.tds_port
        ),
        Format::Postgres => format!("postgresql://{u}:{p}@{host}:{}/postgres", cfg.port),
    }
}

fn main() {
    match Cli::parse().command {
        Commands::Create { name, start, share, no_tls, migration_mode, user, password, port, tds_port } => {
            if !valid_name(&name) {
                fail("instance names may contain letters, digits, '-' and '_' (max 32 characters)");
            }
            if config_path(&name).exists() {
                fail(format!("instance \"{}\" already exists", name));
            }
            let others = all_configs();
            let taken: Vec<u16> = others.iter().flat_map(|c| [c.port, c.tds_port]).collect();
            let port = port.unwrap_or_else(|| allocate(5432, &taken));
            let tds_port = tds_port.unwrap_or_else(|| allocate(1433, &taken));
            if port == tds_port || taken.contains(&port) || taken.contains(&tds_port) {
                fail("the requested ports are already used by another instance");
            }
            let cfg = Config { name: name.clone(), port, tds_port, migration_mode, username: user, password, share, tls: !no_tls };
            fs::create_dir_all(registry_dir()).unwrap_or_else(|e| fail(e.to_string()));
            fs::write(config_path(&name), serde_json::to_string_pretty(&cfg).unwrap())
                .unwrap_or_else(|e| fail(e.to_string()));
            println!("Instance \"{}\" created (PostgreSQL port {}, TDS port {}).", name, port, tds_port);
            if start {
                start_instance(&cfg);
            }
        }
        Commands::Start { name } => start_instance(&load(&name)),
        Commands::Stop { name, timeout } => {
            load(&name);
            if !is_running(&name) {
                println!("Instance \"{}\" is not running.", name);
                return;
            }
            pg0_ok(&s(&["stop", "--name", &format!("{}{}", INSTANCE_PREFIX, name), "--timeout", &timeout.to_string()]), "stop");
            println!("Instance \"{}\" stopped.", name);
        }
        Commands::Delete { name } => {
            load(&name);
            let full = format!("{}{}", INSTANCE_PREFIX, name);
            if is_running(&name) {
                pg0_ok(&s(&["stop", "--name", &full]), "stop");
            }
            // `drop` fails for an instance that was created but never started; that is fine.
            let _ = run_pg0(&s(&["drop", "--name", &full, "--force"]));
            let _ = fs::remove_file(config_path(&name));
            println!("Instance \"{}\" deleted.", name);
        }
        Commands::Info { name: None, json } => {
            let names: Vec<String> = all_configs().into_iter().map(|c| c.name).collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&names).unwrap());
            } else if names.is_empty() {
                println!("No instances. Create one with `localdb create <name> -s`.");
            } else {
                names.iter().for_each(|n| println!("{}", n));
            }
        }
        Commands::Info { name: Some(name), json } => {
            let cfg = load(&name);
            let st = state(&name);
            let running = st["running"].as_bool().unwrap_or(false);
            let data_dir = st["data_dir"].as_str().unwrap_or("-").to_string();
            let version = st["version"].as_str().unwrap_or("-").to_string();
            if json {
                let v = serde_json::json!({
                    "name": cfg.name, "state": if running { "Running" } else { "Stopped" },
                    "postgres_version": version, "port": cfg.port, "tds_port": cfg.tds_port,
                    "migration_mode": cfg.migration_mode, "owner": cfg.username, "shared": cfg.share, "tls": cfg.tls,
                    "data_dir": data_dir, "pid": st["pid"],
                    "connection": connection_string(&cfg, Format::Ado),
                });
                println!("{}", serde_json::to_string_pretty(&v).unwrap());
            } else {
                println!("Name:               {}", cfg.name);
                println!("PostgreSQL version: {}", version);
                println!("State:              {}", if running { "Running" } else { "Stopped" });
                println!("Shared:             {}", if cfg.share { "all interfaces" } else { "localhost only" });
                println!("Encryption:         {}", if cfg.tls { "TLS, self-signed certificate" } else { "off" });
                if cfg.tls {
                    println!("Certificate:        {}/server.crt", data_dir);
                }
                println!("Migration mode:     {}", cfg.migration_mode);
                println!("Owner:              {}", cfg.username);
                println!("PostgreSQL port:    {}", cfg.port);
                println!("TDS port:           {}", cfg.tds_port);
                println!("Data directory:     {}", data_dir);
                println!("Connection:         {}", connection_string(&cfg, Format::Ado));
            }
        }
        Commands::Connection { name, format } => println!("{}", connection_string(&load(&name), format)),
        Commands::Versions => {
            println!("Babelfish {} (PostgreSQL {})", env!("BABELFISH_VERSION"), env!("BABELFISH_PG_VERSION"));
        }
    }
}
