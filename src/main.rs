//! mapstique — renders and serves a browsable map from a Vintage Story world
//! export written by the Mapstique server mod.
//!
//! The mod knows the game; this knows pixels. Nothing here reads a save file or
//! needs the game installed.

mod color;
mod columns;
mod config;
mod error;
mod live;
mod palette;
mod pyramid;
mod render;
mod server;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::columns::World;
use crate::config::Config;
use crate::error::Result;
use crate::palette::Palette;
use crate::render::Renderer;

#[derive(Debug, Parser)]
#[command(name = "mapstique", version, about = "Serve a Vintage Story world map")]
struct Args {
    /// Configuration file (default: ~/.config/mapstique/config.toml).
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// The Vintage Story data directory — the server's --dataPath. Exports are
    /// read from the `mapstique` folder inside it.
    #[arg(short = 'd', long, value_name = "DIR")]
    vs_data: Option<PathBuf>,

    /// Address to listen on when serving.
    #[arg(short, long, value_name = "ADDR")]
    bind: Option<String>,

    /// Where the server mod posts live data: a unix socket path, or `host:port`.
    #[arg(short = 'a', long, value_name = "SOCKET")]
    api_socket: Option<String>,

    /// How many threads render tiles. 0 decides from the machine.
    #[arg(short = 't', long, value_name = "N")]
    threads: Option<usize>,

    /// Write these settings to the configuration file, then carry on.
    #[arg(short = 'S', long)]
    save_config: bool,

    /// Print the resolved configuration as TOML and exit.
    #[arg(short, long)]
    print_config: bool,

    /// What to do. Serves the map when omitted.
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Render the whole exported world to a single PNG.
    Render {
        /// Where to write it.
        #[arg(short, long, value_name = "FILE", default_value = "map.png")]
        out: PathBuf,
    },
    /// Serve the map in a browser.
    Serve,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mapstique: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    let (config, config_path) = resolve(&args)?;

    if args.save_config {
        let existed = config_path.exists();
        config.write(&config_path)?;
        println!(
            "mapstique: {} settings in {}",
            if existed { "replaced" } else { "wrote" },
            config_path.display()
        );
    }

    if args.print_config {
        print!("{}", config.to_template());
        return Ok(());
    }

    // Written on a first run so the settings are there to edit. It holds the
    // defaults, not this run's flags: a one-off --vs-data must not quietly
    // become permanent. That is what --save-config is for.
    if args.config.is_none() && !config_path.exists() {
        match Config::default().write(&config_path) {
            Ok(()) => println!("mapstique: wrote default settings to {}", config_path.display()),
            Err(error) => eprintln!("mapstique: {error}"),
        }
    }

    let exports = config.exports();
    let palette = Palette::load(&exports)?;
    let world = World::load(&exports)?;
    let (min_x, min_z, max_x, max_z) = world.bounds();

    // Printed first and on every run: the quickest way to tell a deployed binary
    // from the one you meant to deploy.
    println!("mapstique {}", env!("CARGO_PKG_VERSION"));
    println!("mapstique: reading {}", exports.display());
    if world.is_empty() {
        println!(
            "mapstique: nothing exported yet — the map fills in as the server \
             exports, and this page is already serving"
        );
    } else {
        println!(
            "mapstique: {} chunks in {} regions, {}x{} blocks",
            world.chunks.len(),
            world.regions.len(),
            max_x - min_x,
            max_z - min_z
        );
    }
    println!(
        "mapstique: palette from {}, {} blocks, {} colour maps (game {})",
        palette.source,
        palette.named,
        palette.color_maps.len(),
        palette.game_version
    );

    let coverage = Renderer::new(&world, &palette).coverage();
    println!("mapstique: surface {}", coverage.summary());
    if coverage.is_poor() {
        eprintln!(
            "mapstique: most of the map has no colour — the palette is probably \
             the server's own. An admin joining the game supplies a better one; \
             see `/mapstique status` on the server."
        );
    }

    match args.command.unwrap_or(Command::Serve) {
        Command::Render { out } => {
            if world.is_empty() {
                return Err(error::Error::Empty(
                    "there is nothing to draw yet — the server has exported no regions".to_owned(),
                ));
            }
            let renderer = Renderer::new(&world, &palette);
            let width = (max_x - min_x).unsigned_abs();
            let image = renderer.render(min_x, min_z, width.max((max_z - min_z).unsigned_abs()));
            image
                .save(&out)
                .map_err(|error| error::Error::parse(&out, error.to_string()))?;
            println!("wrote {}", out.display());
            Ok(())
        }
        Command::Serve => {
            let api = server::ApiSocket::resolve(&config.api_socket, &exports);
            server::serve(&config.bind, &exports, palette, &api, config.threads)
        }
    }
}

/// Settings from the file, with any flags laid over the top.
fn resolve(args: &Args) -> Result<(Config, PathBuf)> {
    let path = args.config.clone().unwrap_or_else(config::default_path);
    let mut config = Config::load(&path)?;

    if let Some(vs_data) = &args.vs_data {
        config.vs_data = vs_data.clone();
    }
    if let Some(bind) = &args.bind {
        config.bind = bind.clone();
    }
    if let Some(api_socket) = &args.api_socket {
        config.api_socket = api_socket.clone();
    }
    if let Some(threads) = args.threads {
        config.threads = threads;
    }

    Ok((config, path))
}
