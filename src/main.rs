//! Renders and serves a browsable map from a Vintage Story world export written
//! by the Witchlight server mod.
//!
//! Nothing in this binary reads a save file or requires the game to be
//! installed. The mod exports the world data; this service draws it.

mod config;
mod mapdata;
mod page;
mod protocol;
mod render;
mod server;
mod state;
mod util;
mod web;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::config::Config;
use crate::protocol::api;
use crate::render::palette::Palette;
use crate::render::tiles::Renderer;
use crate::state::State;
use crate::util::error::{self, Result};

#[derive(Debug, Parser)]
#[command(name = "witchlight", version, about = "Serve a Vintage Story world map")]
struct Args {
    /// Configuration file (default: ~/.config/witchlight/config.toml).
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// The Vintage Story data directory, which is the server's --dataPath.
    /// Exports are read from the `witchlight` folder inside it.
    #[arg(short = 'd', long, value_name = "DIR")]
    vs_data: Option<PathBuf>,

    /// The exported map directory to serve. The server mod passes this to name
    /// the world. Set it by hand only when the settings keep a directory per
    /// world and more than one world has been exported.
    #[arg(short = 'e', long, value_name = "DIR")]
    exports: Option<PathBuf>,

    /// Address to listen on when serving.
    #[arg(short, long, value_name = "ADDR")]
    bind: Option<String>,

    /// The address the server mod posts live data to. An empty value means
    /// loopback on a free port.
    #[arg(short = 'a', long, value_name = "ADDR")]
    api_bind: Option<String>,

    /// How many threads render tiles. 0 picks from the CPU count.
    #[arg(short = 't', long, value_name = "N")]
    threads: Option<usize>,

    /// Puts each world's map in a directory of its own. The mod passes this
    /// when it writes the settings, because only the mod can tell singleplayer
    /// from a dedicated server.
    #[arg(long, value_name = "BOOL")]
    per_world: Option<bool>,

    /// Write these settings to the configuration file, then carry on.
    #[arg(short = 'S', long)]
    save_config: bool,

    /// Print the resolved configuration as TOML and exit.
    #[arg(short, long)]
    print_config: bool,

    /// The subcommand to run. Serves the map when omitted.
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Render the whole exported world to a single PNG.
    Render {
        /// The path to write the PNG to.
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

    // Write a settings file on a first run so there is one to edit. It holds
    // the defaults, not this run's flags, so that a one-off --vs-data does not
    // become permanent. --save-config does that instead.
    if args.config.is_none() && !config_path.exists() {
        match Config::default().write(&config_path) {
            Ok(()) => say!("wrote default settings to {}", config_path.display()),
            Err(error) => warn!("{error}"),
        }
    }

    let exports = config.exports(args.exports.as_deref())?;
    let palette = Palette::load(&exports)?;
    // Open the map once here. The banner and the command that follows share
    // this handle rather than each opening the database.
    let state = std::sync::Arc::new(State::load(
        &exports,
        palette,
        config.tile_cache_mb.max(1) * 1024 * 1024,
        config.rules(),
    )?);

    // Print the version first on every run so a deployed binary can be
    // identified from its log.
    println!("witchlight {}", env!("CARGO_PKG_VERSION"));
    say!("reading {}", exports.display());
    banner(&state);

    match args.command.unwrap_or(Command::Serve) {
        Command::Render { out } => {
            let (Ok(world), Ok(palette)) = (state.world.read(), state.palette.read()) else {
                return Err(error::Error::Empty("the map could not be read".to_owned()));
            };
            if world.is_empty() {
                return Err(error::Error::Empty(
                    "there is nothing to draw yet — the server has exported no regions".to_owned(),
                ));
            }
            let (min_x, min_z, max_x, max_z) = world.bounds();
            let renderer = Renderer::new(&world, &palette, state.sea_level());
            let width = (max_x - min_x).unsigned_abs();
            let image = renderer.render(min_x, min_z, width.max((max_z - min_z).unsigned_abs()));
            image.save(&out).map_err(|error| error::Error::parse(&out, error.to_string()))?;
            println!("wrote {}", out.display());
            Ok(())
        }
        Command::Serve => server::serve(
            &config.bind,
            state,
            api::Api::resolve(&config.api_bind, &config.api_token),
            config.threads,
            config.backfill_radius_chunks,
        ),
    }
}

/// Logs what was loaded, so a reader can tell an empty map from a broken one
/// and a working palette from one that paints nothing.
fn banner(state: &State) {
    let (Ok(world), Ok(palette)) = (state.world.read(), state.palette.read()) else {
        return;
    };
    let (min_x, min_z, max_x, max_z) = world.bounds();

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
        // Logged at info rather than warn. The colours come from a client's
        // assets and the mod requests them on its own, so this reports progress
        // rather than a fault to act on.
        say!(
            "{} of them draw something this palette has no colour for, \
             and map as bare ground until the server has been given one",
            palette.uncoloured
        );
    }

    let coverage = Renderer::new(&world, &palette, state.sea_level()).coverage();
    say!("surface {}", coverage.summary());
    if coverage.is_poor() {
        warn!(
            "most of the map has no colour — the palette is probably \
             the server's own. An admin joining the game supplies a better one; \
             see `/witchlight status` on the server."
        );
    }
}

/// Loads the settings file and applies any command-line flags over it.
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
