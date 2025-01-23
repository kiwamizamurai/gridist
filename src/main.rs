/*!
Gridist CLI - Command line interface for the Gridist image grid tool

This binary provides a command-line interface to:
- Upload images and convert them to grid layouts on GitHub Gists
- Manage existing gists through an interactive TUI

# Usage

```bash
# Upload an image
gridist upload image.png -t <github_token>

# Manage gists
gridist manage -t <github_token>
```

The GitHub token can also be provided via the GITHUB_TOKEN environment variable.
*/

use clap::{Parser, Subcommand};
use gridist::{
    config::ImageConfig, cropper::ImageCropper, github::GithubUploader, tui::GistManager,
};
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

/// Command line interface for Gridist
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    #[command(subcommand)]
    command: Commands,
}

/// Available commands in the CLI
#[derive(Subcommand)]
enum Commands {
    /// Upload an image to GitHub Gist
    Upload {
        /// Path to the image file
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// GitHub personal access token
        #[arg(short, long, env = "GITHUB_TOKEN")]
        token: String,
    },
    /// Manage uploaded gists
    Manage {
        /// GitHub personal access token
        #[arg(short, long, env = "GITHUB_TOKEN")]
        token: String,
    },
}

/// Entry point for the Gridist CLI application
///
/// Sets up logging based on the command and handles:
/// - Image/GIF processing and upload to GitHub Gists
/// - Interactive TUI for gist management
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize tracing subscriber based on command
    match cli.command {
        Commands::Upload { .. } => {
            // For Upload command, use normal logging
            FmtSubscriber::builder()
                .with_env_filter(
                    EnvFilter::from_default_env()
                        .add_directive(if cli.debug { Level::DEBUG } else { Level::INFO }.into()),
                )
                .with_file(true)
                .with_line_number(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_target(true)
                .pretty()
                .init();
        }
        Commands::Manage { .. } => {
            // For Manage command, only show errors
            FmtSubscriber::builder()
                .with_env_filter(EnvFilter::from_default_env().add_directive(Level::ERROR.into()))
                .with_file(true)
                .with_line_number(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_target(true)
                .pretty()
                .init();
        }
    }

    match cli.command {
        Commands::Upload { file, token } => {
            info!("Starting image upload process for file: {}", file.display());
            let config = ImageConfig::default();
            let cropper = ImageCropper::new(config);
            let uploader = GithubUploader::new(token);

            let cropped_files = if file.extension().map_or(false, |ext| ext == "gif") {
                info!("Processing GIF file");
                cropper.crop_gif(&file)?
            } else {
                info!("Processing static image file");
                cropper.crop_image(&file)?
            };

            info!(
                "Successfully cropped image into {} files",
                cropped_files.len()
            );
            uploader.upload_files(cropped_files).await?;
            info!("Upload process completed successfully");
        }
        Commands::Manage { token } => {
            let uploader = GithubUploader::new(token);
            let mut manager = GistManager::new(uploader);
            manager.run().await?;
        }
    }

    Ok(())
}
