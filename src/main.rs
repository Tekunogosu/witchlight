//! witchlight — renders and serves a browsable map from a Vintage Story world
//! export written by the Witchlight server mod.
//!
//! The mod knows the game; this knows pixels. Nothing here reads a save file or
//! needs the game installed.

mod api;
mod apiport;
mod auth;
mod cache;
mod chrome;
mod color;
mod columns;
mod config;
mod error;
mod facts;
mod feeds;
mod files;
mod http;
mod live;
mod log;
mod net;
mod palette;
mod pending;
mod preferences;
mod pull;
mod pyramid;
mod random;
mod render;
mod routes;
mod server;
mod snapshot;
mod state;
mod stored;
mod urls;
mod viewer;
mod watch;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::columns::World;
use crate::config::Config;
use crate::error::Result;
use crate::palette::Palette;
use crate::render::Renderer;

#[derive(Debug, Parser)]
#[command(name = "witchlight", version, about = "Serve a Vintage Story world map")]
struct Args {
    /// Configuration file (default: ~/.config/witchlight/config.toml).
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// The Vintage Story data directory — the server's --dataPath. Exports are
    /// read from the `witchlight` folder inside it.
    #[arg(short = 'd', long, value_name = "DIR")]
    vs_data: Option<PathBuf>,

    /// The map to serve, named outright. This is how the server mod says which
    /// world's map this is; by hand it is only needed where the settings keep a
    /// directory per world and more than one has been exported.
    #[arg(short = 'e', long, value_name = "DIR")]
    exports: Option<PathBuf>,

    /// Address to listen on when serving.
    #[arg(short, long, value_name = "ADDR")]
    bind: Option<String>,

    /// Where the server mod posts live data. Empty means loopback on a free port.
    #[arg(short = 'a', long, value_name = "ADDR")]
    api_bind: Option<String>,

    /// How many threads render tiles. 0 decides from the machine.
    #[arg(short = 't', long, value_name = "N")]
    threads: Option<usize>,

    /// Whether each world's map goes in a directory of its own. The mod passes
    /// this when it writes the settings, because it is the half that can tell
    /// singleplayer from a dedicated server.
    #[arg(long, value_name = "BOOL")]
    per_world: Option<bool>,

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
            warn!("{error}");
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
        say!(
            "{} settings in {}",
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
            Ok(()) => say!("wrote default settings to {}", config_path.display()),
            Err(error) => warn!("{error}"),
        }
    }

    let exports = config.exports(args.exports.as_deref())?;
    let palette = Palette::load(&exports)?;
    let world = World::load(&exports)?;
    let (min_x, min_z, max_x, max_z) = world.bounds();

    // Printed first and on every run: the quickest way to tell a deployed binary
    // from the one you meant to deploy.
    println!("witchlight {}", env!("CARGO_PKG_VERSION"));
    say!("reading {}", exports.display());
    if world.is_empty() {
        say!(
            "nothing exported yet — the map fills in as the server \
             exports, and this page is already serving"
        );
    } else {
        say!(
            "{} chunks in {} regions, {}x{} blocks",
            world.chunks.len(),
            world.region_count(),
            max_x - min_x,
            max_z - min_z
        );
    }
    say!(
        "palette from {}, {} blocks, {} colour maps (game {})",
        palette.source,
        palette.named,
        palette.color_maps.len(),
        palette.game_version
    );
    if palette.uncoloured > 0 {
        // Said, not warned about. Nothing here can fix it — the colours come off
        // a client's assets — and the mod goes and asks for them on its own, so
        // this is the line that lets somebody watching the log see it happen
        // rather than a fault to act on.
        say!(
            "{} of them draw something this palette has no colour for, \
             and map as bare ground until the server has been given one",
            palette.uncoloured
        );
    }

    let coverage = Renderer::new(&world, &palette, facts::read(&exports).sea_level).coverage();
    say!("surface {}", coverage.summary());
    if coverage.is_poor() {
        warn!(
            "most of the map has no colour — the palette is probably \
             the server's own. An admin joining the game supplies a better one; \
             see `/witchlight status` on the server."
        );
    }

    match args.command.unwrap_or(Command::Serve) {
        Command::Render { out } => {
            if world.is_empty() {
                return Err(error::Error::Empty(
                    "there is nothing to draw yet — the server has exported no regions".to_owned(),
                ));
            }
            let renderer = Renderer::new(&world, &palette, facts::read(&exports).sea_level);
            let width = (max_x - min_x).unsigned_abs();
            let image = renderer.render(min_x, min_z, width.max((max_z - min_z).unsigned_abs()));
            image.save(&out).map_err(|error| error::Error::parse(&out, error.to_string()))?;
            println!("wrote {}", out.display());
            Ok(())
        }
        Command::Serve => server::serve(
            &config.bind,
            &exports,
            palette,
            api::Api::resolve(&config.api_bind, &config.api_token),
            config.threads,
            config.tile_cache_mb,
            config.rules(),
            config.autosave_interval(),
            config.backfill_radius_chunks,
        ),
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
    if let Some(api_bind) = &args.api_bind {
        config.api_bind = api_bind.clone();
    }
    if let Some(threads) = args.threads {
        config.threads = threads;
    }
    if let Some(per_world) = args.per_world {
        config.per_world = per_world;
    }

    Ok((config, path))
}
