use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use kiln::BuildOptions;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "kiln", version, about = "A custom static site generator")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the site.
    Build {
        /// Project root directory (defaults to current directory).
        #[arg(long, default_value = ".")]
        root: PathBuf,

        /// Minify HTML, CSS, and JS in the output directory.
        #[arg(long)]
        minify: bool,

        /// Override `base_url` from `config.toml`. Useful for staging or
        /// per-deploy preview environments that share a single `config.toml`
        /// with prod. Falls back to the `KILN_BASE_URL` environment variable.
        #[arg(long, env = "KILN_BASE_URL")]
        base_url: Option<String>,
    },
    /// Convert Hugo content to kiln format.
    Convert {
        /// Path to Hugo site root.
        #[arg(long)]
        source: PathBuf,
        /// Path to kiln site root.
        #[arg(long)]
        dest: PathBuf,
    },
    /// Scaffold a new theme.
    InitTheme {
        /// Theme name (used as directory name under themes/).
        name: String,

        /// Project root directory (defaults to current directory).
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Start a dev server with live reload.
    Serve {
        /// Project root directory (defaults to current directory).
        #[arg(long, default_value = ".")]
        root: PathBuf,

        /// Port to serve on.
        #[arg(long, default_value_t = kiln::DEFAULT_PORT)]
        port: u16,

        /// Open the site in the default browser after starting.
        #[arg(long)]
        open: bool,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::Build {
            root,
            minify,
            base_url,
        } => {
            let root = root.canonicalize()?;
            // An empty `KILN_BASE_URL` env var (e.g., from a workflow input
            // left unset) should fall through to config.toml, not blank out
            // every URL.
            let base_url = base_url.filter(|s| !s.is_empty());
            kiln::build(
                &root,
                BuildOptions {
                    minify,
                    base_url_override: base_url.as_deref(),
                    ..Default::default()
                },
            )?;
        }
        Command::Convert { source, dest } => {
            let source = source.canonicalize()?;
            let dest = dest.canonicalize().unwrap_or(dest);
            kiln::convert(&source, &dest)?;
        }
        Command::InitTheme { name, root } => {
            let root = root.canonicalize()?;
            kiln::init_theme(&root, &name)?;
        }
        Command::Serve { root, port, open } => {
            let root = root.canonicalize()?;
            kiln::serve(&root, port, open)?;
        }
    }

    Ok(())
}
