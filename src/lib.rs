/*!
Gridist - A tool for creating and managing grid-based image layouts on GitHub Gists

This library provides functionality to:
1. Process images and GIFs into grid layouts
2. Upload processed images to GitHub Gists
3. Manage uploaded gists through a TUI interface

# Main Components

- `config`: Configuration settings for image processing and layout
- `cropper`: Image and GIF processing functionality
- `github`: GitHub Gist API interaction and file management
- `tui`: Terminal user interface for gist management

# Error Handling

The library uses a custom error type `GridistError` for all operations,
with `GridistResult<T>` as a convenience type alias.
*/

use anyhow::Context;
use arboard::Clipboard;
use gif::{Decoder, Encoder, Frame, Repeat};
use git2::{Cred, RemoteCallbacks, Signature};
use image::{GenericImageView, RgbaImage};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use kdtree::distance::squared_euclidean;
use kdtree::KdTree;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use rayon::prelude::*;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use serde_json::json;
use std::borrow::Cow;
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use tempfile::TempDir;
use thiserror::Error;
use tracing::{debug, error, info};

/// Custom error types for Gridist operations
#[derive(Error, Debug)]
pub enum GridistError {
    #[error("Failed to process image: {0}")]
    ImageProcessingError(#[from] image::ImageError),

    #[error("Failed to process GIF: {0}")]
    GifError(#[from] gif::DecodingError),

    #[error("Failed to encode GIF: {0}")]
    GifEncodingError(#[from] gif::EncodingError),

    #[error("Failed to create file: {0}")]
    FileCreationError(#[from] std::io::Error),

    #[error("Failed to upload to GitHub: {0}")]
    GithubUploadError(String),

    #[error("Invalid file name: {0}")]
    InvalidFileName(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Result type alias for Gridist operations
pub type GridistResult<T> = std::result::Result<T, GridistError>;

/// Configuration settings for image processing and layout
pub mod config {
    /// Configuration for image dimensions and spacing
    #[derive(Debug, Clone)]
    pub struct ImageConfig {
        /// Width of the container that holds all cards
        pub container_width: u32,
        /// Width of each cut/card
        pub cut_width: u32,
        /// Height of each cut/card
        pub cut_height: u32,
        /// Top padding within each card
        pub card_padding_top: u32,
        /// Horizontal padding within each card
        pub card_padding_horizontal: u32,
        /// Bottom padding within each card
        pub card_padding_bottom: u32,
        /// Margin between cards
        pub card_margin_bottom: u32,
    }

    impl Default for ImageConfig {
        fn default() -> Self {
            Self {
                container_width: 928,
                cut_width: 422,
                cut_height: 100,
                card_padding_top: 37,
                card_padding_horizontal: 16,
                card_padding_bottom: 16,
                card_margin_bottom: 16,
            }
        }
    }

    impl ImageConfig {
        /// Calculates the total height of a card including content and padding
        pub fn card_height(&self) -> u32 {
            self.card_padding_top + self.cut_height + self.card_padding_bottom
        }

        /// Calculates the vertical offset between cards including margin
        pub fn y_offset(&self) -> u32 {
            self.card_height() + self.card_margin_bottom
        }

        /// Calculates the minimum height required for a grid layout
        pub fn minimum_height(&self) -> u32 {
            3 * self.card_height() + 2 * self.card_margin_bottom
        }
    }
}

/// Image and GIF processing functionality
pub mod cropper {
    use super::*;
    use image::imageops::FilterType;

    /// Handles the cropping and processing of images into grid layouts
    #[derive(Default)]
    pub struct ImageCropper {
        config: config::ImageConfig,
    }

    impl ImageCropper {
        /// Creates a new ImageCropper with the specified configuration
        pub fn new(config: config::ImageConfig) -> Self {
            info!("Creating new ImageCropper with config: {:?}", config);
            Self { config }
        }

        /// Calculates the x,y coordinates for a grid segment at the given index
        pub fn get_xy(&self, index: u32) -> (u32, u32) {
            let is_left = index % 2 == 0;
            let x = if is_left {
                self.config.card_padding_horizontal
            } else {
                self.config.container_width
                    - self.config.cut_width
                    - self.config.card_padding_horizontal
            };
            let index_from_top = index / 2;
            let y = self.config.card_padding_top + index_from_top * self.config.y_offset();
            debug!("Calculated position for index {}: ({}, {})", index, x, y);
            (x, y)
        }

        /// Calculates the dimensions to resize an image while maintaining aspect ratio
        pub fn calculate_resize_dimensions(&self, width: u32, height: u32) -> (u32, u32) {
            let aspect_ratio = width as f32 / height as f32;
            let target_aspect_ratio =
                self.config.container_width as f32 / self.config.minimum_height() as f32;

            let (resize_width, resize_height) = if aspect_ratio >= target_aspect_ratio {
                let scale = f32::max(
                    self.config.container_width as f32 / width as f32,
                    self.config.minimum_height() as f32 / height as f32,
                );
                (
                    (width as f32 * scale) as u32,
                    (height as f32 * scale) as u32,
                )
            } else {
                let scale = self.config.container_width as f32 / width as f32;
                (self.config.container_width, (height as f32 * scale) as u32)
            };

            debug!(
                "Calculated resize dimensions: {}x{} -> {}x{} (aspect ratio: {:.2})",
                width, height, resize_width, resize_height, aspect_ratio
            );
            (resize_width, resize_height)
        }

        /// Crops a static image into a grid layout
        /// Returns paths to the generated grid segments
        pub fn crop_image(&self, path: &Path) -> GridistResult<Vec<PathBuf>> {
            info!("Starting image cropping process for: {}", path.display());
            let image = image::open(path).context("Failed to open image")?;
            let (width, height) = image.dimensions();
            info!("Original image dimensions: {}x{}", width, height);

            let (resize_width, resize_height) = self.calculate_resize_dimensions(width, height);
            info!("Resizing image to {}x{}", resize_width, resize_height);

            let progress_bar = ProgressBar::new_spinner();
            progress_bar.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.green} {msg}")
                    .unwrap(),
            );
            progress_bar.set_message("Resizing image...");

            let resized = image.resize(resize_width, resize_height, FilterType::Lanczos3);
            progress_bar.finish_with_message("Resizing complete");

            let offset_x =
                ((resize_width as i32 - self.config.container_width as i32) / 2).max(0) as u32;
            let offset_y =
                ((resize_height as i32 - self.config.minimum_height() as i32) / 2).max(0) as u32;

            info!(
                "Cropping image into grid with offsets: x={}, y={}",
                offset_x, offset_y
            );

            let progress_bar = ProgressBar::new(6);
            progress_bar.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                    .unwrap()
                    .progress_chars("#>-"),
            );

            let output_files: Vec<_> = (0..6)
                .into_par_iter()
                .map(|i| -> GridistResult<PathBuf> {
                    let result = (|| -> GridistResult<PathBuf> {
                        debug!("Processing grid segment {}/6", i + 1);
                        let filename = format!(
                            "{}.{}.{}",
                            path.file_stem()
                                .ok_or_else(|| GridistError::InvalidFileName(
                                    "No file stem".to_string()
                                ))?
                                .to_str()
                                .ok_or_else(|| GridistError::InvalidFileName(
                                    "Invalid UTF-8 in file stem".to_string()
                                ))?,
                            i,
                            path.extension()
                                .ok_or_else(|| GridistError::InvalidFileName(
                                    "No file extension".to_string()
                                ))?
                                .to_str()
                                .ok_or_else(|| GridistError::InvalidFileName(
                                    "Invalid UTF-8 in extension".to_string()
                                ))?
                        );
                        let output_path = PathBuf::from(&filename);
                        debug!("Creating output file: {}", output_path.display());

                        let (base_x, base_y) = self.get_xy(i);
                        let x = base_x + offset_x;
                        let y = base_y + offset_y;

                        let cropped =
                            resized.crop_imm(x, y, self.config.cut_width, self.config.cut_height);
                        cropped.save(&output_path).with_context(|| {
                            format!("Failed to save cropped image {}", output_path.display())
                        })?;

                        debug!(
                            "Successfully saved grid segment {} to {}",
                            i + 1,
                            output_path.display()
                        );
                        Ok(output_path)
                    })();
                    if let Err(ref e) = result {
                        error!("Failed to process grid segment {}/6: {}", i + 1, e);
                    }
                    progress_bar.inc(1);
                    result
                })
                .collect::<Result<Vec<_>, _>>()?;

            progress_bar.finish_with_message("Grid creation complete");
            info!("Successfully created {} grid segments", output_files.len());
            Ok(output_files)
        }

        /// Crops an animated GIF into a grid layout, maintaining animation
        /// Returns paths to the generated grid segments
        pub fn crop_gif(&self, path: &Path) -> GridistResult<Vec<PathBuf>> {
            info!("Reading GIF file: {}", path.display());
            let multi_progress = MultiProgress::new();
            let spinner = multi_progress.add(ProgressBar::new_spinner());
            spinner.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.green} {msg}")
                    .unwrap(),
            );
            spinner.set_message("Reading GIF frames...");

            let file = File::open(path)
                .with_context(|| format!("Failed to open GIF file: {}", path.display()))?;
            let mut decoder = Decoder::new(file).with_context(|| "Failed to create GIF decoder")?;
            let mut frames = Vec::new();
            let global_palette = decoder.global_palette().map(|p| p.to_vec());

            while let Some(frame) = decoder
                .read_next_frame()
                .with_context(|| "Failed to read GIF frame")?
            {
                frames.push(frame.clone());
            }
            spinner.finish_with_message(format!("Read {} frames", frames.len()));

            let orig_width = decoder.width() as f32;
            let orig_height = decoder.height() as f32;

            let (target_width, target_height) =
                self.calculate_resize_dimensions(orig_width as u32, orig_height as u32);

            let offset_x =
                ((target_width as i32 - self.config.container_width as i32) / 2).max(0) as u32;
            let offset_y =
                ((target_height as i32 - self.config.minimum_height() as i32) / 2).max(0) as u32;

            let default_palette = self.create_default_palette();
            let encoder_palette = global_palette
                .as_ref()
                .or_else(|| frames.first().and_then(|f| f.palette.as_ref()))
                .map_or(&default_palette, |p| p);

            let palette_lookup = self.create_optimized_palette_lookup(encoder_palette);

            info!("Creating grid from GIF with {} frames", frames.len());
            let grid_progress = multi_progress.add(ProgressBar::new(6));
            grid_progress.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} Grid {msg}")
                    .unwrap()
                    .progress_chars("#>-"),
            );

            let frame_progress =
                Arc::new(multi_progress.add(ProgressBar::new(frames.len() as u64 * 6)));
            frame_progress.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} Frames")
                    .unwrap()
                    .progress_chars("#>-"),
            );

            let output_files: Vec<_> = (0..6)
                .into_par_iter()
                .map(|i| -> GridistResult<PathBuf> {
                    let result = (|| -> GridistResult<PathBuf> {
                        let (base_x, base_y) = self.get_xy(i);
                        let x = base_x + offset_x;
                        let y = base_y + offset_y;

                        let filename = format!(
                            "{}.{}.gif",
                            path.file_stem()
                                .ok_or_else(|| GridistError::InvalidFileName(
                                    "No file stem".to_string()
                                ))?
                                .to_str()
                                .ok_or_else(|| GridistError::InvalidFileName(
                                    "Invalid UTF-8 in file stem".to_string()
                                ))?,
                            i
                        );
                        let output_path = PathBuf::from(&filename);
                        let output = File::create(&output_path).with_context(|| {
                            format!("Failed to create output file: {}", output_path.display())
                        })?;

                        let mut encoder = Encoder::new(
                            output,
                            self.config.cut_width as u16,
                            self.config.cut_height as u16,
                            encoder_palette,
                        )
                        .with_context(|| "Failed to create GIF encoder")?;

                        encoder
                            .set_repeat(Repeat::Infinite)
                            .with_context(|| "Failed to set GIF repeat mode")?;

                        let frame_progress = Arc::clone(&frame_progress);
                        let processed_frames: Vec<_> = frames
                            .par_iter()
                            .map(|frame| -> GridistResult<Frame> {
                                let result = (|| -> GridistResult<Frame> {
                                    let mut resized_frame = Frame {
                                        delay: frame.delay,
                                        dispose: frame.dispose,
                                        transparent: frame.transparent,
                                        needs_user_input: frame.needs_user_input,
                                        top: 0,
                                        left: 0,
                                        width: self.config.cut_width as u16,
                                        height: self.config.cut_height as u16,
                                        ..Default::default()
                                    };

                                    let rgba_buffer =
                                        self.convert_to_rgba_optimized(frame, encoder_palette);
                                    let image = RgbaImage::from_raw(
                                        frame.width as u32,
                                        frame.height as u32,
                                        rgba_buffer,
                                    )
                                    .ok_or_else(|| {
                                        GridistError::ImageProcessingError(
                                            image::ImageError::Limits(
                                                image::error::LimitError::from_kind(
                                                    image::error::LimitErrorKind::DimensionError,
                                                ),
                                            ),
                                        )
                                    })?;

                                    let resized = image::imageops::resize(
                                        &image,
                                        target_width,
                                        target_height,
                                        FilterType::Lanczos3,
                                    );

                                    let cropped = image::imageops::crop_imm(
                                        &resized,
                                        x,
                                        y,
                                        self.config.cut_width,
                                        self.config.cut_height,
                                    )
                                    .to_image();

                                    let cropped_rgba = cropped.as_raw();
                                    let indexed_buffer = self.convert_to_indexed_optimized(
                                        cropped_rgba,
                                        &palette_lookup,
                                        frame.transparent.unwrap_or(0),
                                    );

                                    resized_frame.buffer = Cow::Owned(indexed_buffer);
                                    Ok(resized_frame)
                                })();
                                frame_progress.inc(1);
                                result
                            })
                            .collect::<Result<Vec<_>, _>>()?;

                        for frame in processed_frames {
                            encoder
                                .write_frame(&frame)
                                .map_err(GridistError::GifEncodingError)?;
                        }

                        Ok(output_path)
                    })();
                    grid_progress.inc(1);
                    result
                })
                .collect::<Result<Vec<_>, _>>()?;

            grid_progress.finish_with_message("complete");
            frame_progress.finish();
            Ok(output_files)
        }

        /// Creates a default color palette for GIF processing
        fn create_default_palette(&self) -> Vec<u8> {
            let mut palette = Vec::with_capacity(768);

            let base_colors = [
                (255, 0, 0),     // Red
                (0, 255, 0),     // Green
                (0, 0, 255),     // Blue
                (255, 255, 0),   // Yellow
                (255, 0, 255),   // Magenta
                (0, 255, 255),   // Cyan
                (255, 255, 255), // White
                (0, 0, 0),       // Black
            ];

            for &(r, g, b) in &base_colors {
                palette.push(r);
                palette.push(g);
                palette.push(b);
            }

            for i in 0..31 {
                palette.push((i * 8) as u8);
                palette.push((i * 8) as u8);
                palette.push((i * 8) as u8);
            }

            while palette.len() < 768 {
                palette.push(0);
                palette.push(0);
                palette.push(0);
            }

            palette
        }

        /// Creates an optimized lookup table for palette colors
        fn create_optimized_palette_lookup(&self, palette: &[u8]) -> Vec<(u8, [u8; 3])> {
            let mut lookup = Vec::with_capacity(palette.len() / 3);
            for (idx, colors) in palette.chunks(3).enumerate() {
                if colors.len() < 3 {
                    continue;
                }
                lookup.push((idx as u8, [colors[0], colors[1], colors[2]]));
            }
            lookup
        }

        /// Creates a KD-tree for efficient color matching
        fn create_palette_kdtree(&self, palette: &[u8]) -> KdTree<f32, u8, [f32; 3]> {
            let mut kdtree = KdTree::new(3);
            for (idx, colors) in palette.chunks(3).enumerate() {
                if colors.len() < 3 {
                    continue;
                }
                kdtree
                    .add(
                        [colors[0] as f32, colors[1] as f32, colors[2] as f32],
                        idx as u8,
                    )
                    .unwrap();
            }
            kdtree
        }

        /// Converts RGBA pixels to indexed colors using the palette
        fn convert_to_indexed_optimized(
            &self,
            rgba: &[u8],
            palette_lookup: &[(u8, [u8; 3])],
            transparent: u8,
        ) -> Vec<u8> {
            let kdtree = self.create_palette_kdtree(
                &palette_lookup
                    .iter()
                    .flat_map(|(_, colors)| colors.iter().copied())
                    .collect::<Vec<_>>(),
            );
            let result = Arc::new(Mutex::new(Vec::with_capacity(rgba.len() / 4)));

            let chunk_size = 8; // Process 8 pixels at a time
            rgba.par_chunks(4 * chunk_size).for_each(|chunk| {
                let mut local_result = Vec::with_capacity(chunk_size);

                for pixel in chunk.chunks(4) {
                    if pixel[3] < 128 {
                        local_result.push(transparent);
                        continue;
                    }

                    let nearest = kdtree
                        .nearest(
                            &[pixel[0] as f32, pixel[1] as f32, pixel[2] as f32],
                            1,
                            &squared_euclidean,
                        )
                        .unwrap();

                    local_result.push(*nearest[0].1);
                }

                let mut result = result.lock().unwrap();
                result.extend(local_result);
            });

            Arc::try_unwrap(result).unwrap().into_inner().unwrap()
        }

        /// Converts indexed colors to RGBA using the palette
        fn convert_to_rgba_optimized(&self, frame: &Frame, palette: &[u8]) -> Vec<u8> {
            let buffer_size = frame.buffer.len() * 4;
            let mut rgba = Vec::with_capacity(buffer_size);
            let transparent = frame.transparent;

            let chunk_size = 8; // Process 8 pixels at a time
            let chunks = frame.buffer.chunks_exact(chunk_size);
            let remainder = chunks.remainder();

            // Process main chunks
            for chunk in chunks {
                let mut local_rgba = Vec::with_capacity(chunk_size * 4);
                for &pixel in chunk {
                    let palette_idx = (pixel as usize) * 3;
                    let alpha = if transparent.map(|t| t == pixel).unwrap_or(false) {
                        0u8
                    } else {
                        255u8
                    };

                    if palette_idx + 2 < palette.len() {
                        local_rgba.extend_from_slice(&[
                            palette[palette_idx],
                            palette[palette_idx + 1],
                            palette[palette_idx + 2],
                            alpha,
                        ]);
                    } else {
                        local_rgba.extend_from_slice(&[0, 0, 0, alpha]);
                    }
                }
                rgba.extend(local_rgba);
            }

            // Process remaining pixels
            for &pixel in remainder {
                let palette_idx = (pixel as usize) * 3;
                let alpha = if transparent.map(|t| t == pixel).unwrap_or(false) {
                    0
                } else {
                    255
                };

                if palette_idx + 2 < palette.len() {
                    rgba.extend_from_slice(&[
                        palette[palette_idx],
                        palette[palette_idx + 1],
                        palette[palette_idx + 2],
                        alpha,
                    ]);
                } else {
                    rgba.extend_from_slice(&[0, 0, 0, alpha]);
                }
            }

            rgba
        }
    }
}

/// Information about a GitHub Gist
#[derive(Debug, Clone)]
pub struct GistInfo {
    /// Unique identifier of the gist
    pub id: String,
    /// Description of the gist
    pub description: String,
    /// Creation timestamp
    pub created_at: String,
}

/// GitHub API interaction and file management
pub mod github {
    use super::*;
    use std::path::{Path, PathBuf};

    /// Handles uploading and managing files on GitHub Gists
    pub struct GithubUploader {
        client: reqwest::Client,
        token: String,
        quiet_mode: bool,
    }

    impl GithubUploader {
        /// Creates a new GithubUploader with the specified token
        pub fn new(token: String) -> Self {
            Self {
                client: reqwest::Client::new(),
                token,
                quiet_mode: false,
            }
        }

        /// Sets whether to suppress log output
        pub fn set_quiet_mode(&mut self, quiet: bool) {
            self.quiet_mode = quiet;
        }

        /// Logs an info message if not in quiet mode
        fn log_info(&self, message: &str) {
            if !self.quiet_mode {
                info!("{}", message);
            }
        }

        /// Logs a debug message if not in quiet mode
        fn log_debug(&self, message: &str) {
            if !self.quiet_mode {
                debug!("{}", message);
            }
        }

        /// Uploads multiple files to GitHub Gists
        pub async fn upload_files(&self, files: Vec<PathBuf>) -> GridistResult<()> {
            self.log_info(&format!(
                "Starting upload of {} files to GitHub",
                files.len()
            ));
            let multi_progress = MultiProgress::new();
            let total_progress = multi_progress.add(ProgressBar::new(files.len() as u64));
            total_progress.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} files")
                    .unwrap()
                    .progress_chars("#>-"),
            );

            let mut uploaded_files = Vec::new();
            for file in files {
                let filename = file
                    .file_name()
                    .ok_or_else(|| {
                        GridistError::GithubUploadError("Invalid file name".to_string())
                    })?
                    .to_str()
                    .ok_or_else(|| {
                        GridistError::GithubUploadError("Invalid UTF-8 in file name".to_string())
                    })?;

                info!("Processing file for upload: {}", filename);
                let spinner = multi_progress.add(ProgressBar::new_spinner());
                spinner.set_style(
                    ProgressStyle::default_spinner()
                        .template("{spinner:.green} {msg}")
                        .unwrap(),
                );
                spinner.enable_steady_tick(std::time::Duration::from_millis(100));

                spinner.set_message(format!("Creating gist for {}", filename));
                let gist_data = json!({
                    "description": format!("Generated by gridist: {}", filename),
                    "public": true,
                    "files": {
                        filename: {
                            "content": "placeholder"
                        }
                    }
                });

                debug!("Creating initial gist for file: {}", filename);
                let gist_id = self.create_gist(&gist_data).await?;
                info!("Created gist with ID: {}", gist_id);

                spinner.set_message(format!("Uploading {} to gist", filename));
                debug!("Updating gist {} with file content", gist_id);
                self.update_gist_via_git(&gist_id, &file)?;

                spinner.finish_and_clear();
                uploaded_files.push(filename.to_string());
                total_progress.inc(1);
            }

            total_progress.finish();
            info!(
                "Successfully uploaded files:\n{}",
                uploaded_files.join("\n")
            );
            Ok(())
        }

        /// Creates HTTP headers for GitHub API requests
        fn create_headers(&self) -> GridistResult<HeaderMap> {
            debug!("Creating GitHub API headers");
            let mut headers = HeaderMap::new();
            headers.insert(
                ACCEPT,
                HeaderValue::from_static("application/vnd.github+json"),
            );
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", self.token))
                    .map_err(|e| GridistError::GithubUploadError(e.to_string()))?,
            );
            headers.insert(
                reqwest::header::USER_AGENT,
                HeaderValue::from_static("gridist"),
            );
            headers.insert(
                "X-GitHub-Api-Version",
                HeaderValue::from_static("2022-11-28"),
            );
            Ok(headers)
        }

        /// Creates a new GitHub Gist
        async fn create_gist(&self, data: &serde_json::Value) -> GridistResult<String> {
            debug!("Sending create gist request");
            let response = self
                .client
                .post("https://api.github.com/gists")
                .json(data)
                .headers(self.create_headers()?)
                .send()
                .await
                .map_err(|e| {
                    GridistError::GithubUploadError(format!("Failed to create gist: {}", e))
                })?;

            let status = response.status();
            let response_body = response.text().await.map_err(|e| {
                GridistError::GithubUploadError(format!("Failed to read response body: {}", e))
            })?;

            if !status.is_success() {
                error!("Failed to create gist: {} - {}", status, response_body);
                return Err(GridistError::GithubUploadError(format!(
                    "Failed to create gist: {} - Response: {}",
                    status, response_body
                )));
            }

            let gist = serde_json::from_str::<serde_json::Value>(&response_body).map_err(|e| {
                GridistError::GithubUploadError(format!("Failed to parse gist response: {}", e))
            })?;

            let gist_id = gist["id"]
                .as_str()
                .ok_or_else(|| GridistError::GithubUploadError("Invalid gist ID".to_string()))
                .map(String::from)?;

            debug!("Successfully created gist with ID: {}", gist_id);
            Ok(gist_id)
        }

        /// Updates a Gist's content using Git operations
        fn update_gist_via_git(&self, gist_id: &str, file: &Path) -> GridistResult<()> {
            info!("Updating gist {} with file content via git", gist_id);
            // Create a temporary directory for the git operations
            let temp_dir = TempDir::new().map_err(|e| {
                GridistError::GithubUploadError(format!("Failed to create temp dir: {}", e))
            })?;

            debug!("Cloning gist repository");
            // Clone the gist repository
            let mut callbacks = RemoteCallbacks::new();
            callbacks.credentials(|_url, _username_from_url, _allowed_types| {
                Cred::userpass_plaintext("git", &self.token)
            });

            let mut fetch_options = git2::FetchOptions::new();
            fetch_options.remote_callbacks(callbacks);

            let mut builder = git2::build::RepoBuilder::new();
            builder.fetch_options(fetch_options);

            let repo = builder
                .clone(
                    &format!("https://gist.github.com/{}.git", gist_id),
                    temp_dir.path(),
                )
                .map_err(|e| {
                    GridistError::GithubUploadError(format!("Failed to clone gist: {}", e))
                })?;

            debug!("Copying file to repository");
            // Copy the file to the repository
            let target_path =
                temp_dir.path().join(file.file_name().ok_or_else(|| {
                    GridistError::GithubUploadError("Invalid file name".to_string())
                })?);
            fs::copy(file, target_path).map_err(|e| {
                GridistError::GithubUploadError(format!("Failed to copy file: {}", e))
            })?;

            debug!("Adding file to git index");
            // Add the file to git
            let mut index = repo.index().map_err(|e| {
                GridistError::GithubUploadError(format!("Failed to get index: {}", e))
            })?;
            index
                .add_path(Path::new(file.file_name().ok_or_else(|| {
                    GridistError::GithubUploadError("Invalid file name".to_string())
                })?))
                .map_err(|e| {
                    GridistError::GithubUploadError(format!("Failed to add file to index: {}", e))
                })?;
            index.write().map_err(|e| {
                GridistError::GithubUploadError(format!("Failed to write index: {}", e))
            })?;

            debug!("Creating commit");
            // Create the commit
            let signature = Signature::now("gridist", "gridist@example.com").map_err(|e| {
                GridistError::GithubUploadError(format!("Failed to create signature: {}", e))
            })?;
            let tree_id = index.write_tree().map_err(|e| {
                GridistError::GithubUploadError(format!("Failed to write tree: {}", e))
            })?;
            let tree = repo.find_tree(tree_id).map_err(|e| {
                GridistError::GithubUploadError(format!("Failed to find tree: {}", e))
            })?;

            let parent = repo
                .head()
                .ok()
                .and_then(|head| head.target())
                .and_then(|oid| repo.find_commit(oid).ok());

            let parents_refs: Vec<&git2::Commit> = match &parent {
                Some(commit) => vec![commit],
                None => vec![],
            };

            repo.commit(
                Some("HEAD"),
                &signature,
                &signature,
                "Update from gridist",
                &tree,
                parents_refs.as_slice(),
            )
            .map_err(|e| {
                GridistError::GithubUploadError(format!("Failed to create commit: {}", e))
            })?;

            debug!("Pushing changes to remote");
            // Push the changes
            let mut remote = repo.find_remote("origin").map_err(|e| {
                GridistError::GithubUploadError(format!("Failed to find remote: {}", e))
            })?;

            let mut push_callbacks = RemoteCallbacks::new();
            push_callbacks.credentials(|_url, _username_from_url, _allowed_types| {
                Cred::userpass_plaintext("git", &self.token)
            });

            let mut push_options = git2::PushOptions::new();
            push_options.remote_callbacks(push_callbacks);

            remote
                .push(&["refs/heads/main"], Some(&mut push_options))
                .map_err(|e| {
                    GridistError::GithubUploadError(format!("Failed to push changes: {}", e))
                })?;

            info!("Successfully updated gist {} with file content", gist_id);
            Ok(())
        }

        /// Deletes a GitHub Gist by ID
        pub async fn delete_gist(&self, gist_id: &str) -> GridistResult<()> {
            self.log_info(&format!("Deleting gist: {}", gist_id));
            let response = self
                .client
                .delete(format!("https://api.github.com/gists/{}", gist_id))
                .headers(self.create_headers()?)
                .send()
                .await
                .map_err(|e| {
                    GridistError::GithubUploadError(format!("Failed to delete gist: {}", e))
                })?;

            if !response.status().is_success() {
                error!("Failed to delete gist {}: {}", gist_id, response.status());
                return Err(GridistError::GithubUploadError(format!(
                    "Failed to delete gist: {}",
                    response.status()
                )));
            }

            info!("Successfully deleted gist: {}", gist_id);
            Ok(())
        }

        /// Lists all GitHub Gists for the authenticated user
        pub async fn list_gists(&self) -> GridistResult<Vec<GistInfo>> {
            self.log_debug("Fetching list of gists");
            let response = self
                .client
                .get("https://api.github.com/gists")
                .headers(self.create_headers()?)
                .send()
                .await
                .map_err(|e| {
                    GridistError::GithubUploadError(format!("Failed to list gists: {}", e))
                })?;

            let gists: Vec<serde_json::Value> = response.json().await.map_err(|e| {
                GridistError::GithubUploadError(format!("Failed to parse gists response: {}", e))
            })?;

            let gist_infos: Vec<GistInfo> = gists
                .into_iter()
                .filter_map(|gist: serde_json::Value| {
                    let id: &str = gist["id"].as_str()?;
                    let description: &str =
                        gist["description"].as_str().unwrap_or("No description");
                    let created_at: &str = gist["created_at"].as_str()?;
                    Some(GistInfo {
                        id: id.to_string(),
                        description: description.to_string(),
                        created_at: created_at.to_string(),
                    })
                })
                .collect();

            info!("Retrieved {} gists", gist_infos.len());
            Ok(gist_infos)
        }
    }
}

/// Terminal user interface for gist management
pub mod tui {
    use super::*;
    use crate::github::GithubUploader;
    use chrono::DateTime;
    use crossterm::{
        event::{self, Event, KeyCode},
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
        ExecutableCommand,
    };
    use ratatui::{
        backend::CrosstermBackend,
        layout::{Constraint, Direction, Layout},
        style::{Color, Modifier, Style},
        widgets::{Block, Borders, List, ListItem, ListState},
        Terminal,
    };
    use std::io;

    /// Manages the interactive TUI for gist operations
    pub struct GistManager {
        gists: Vec<GistInfo>,
        state: ListState,
        uploader: GithubUploader,
    }

    impl GistManager {
        /// Creates a new GistManager with the specified uploader
        pub fn new(mut uploader: GithubUploader) -> Self {
            uploader.set_quiet_mode(true);
            Self {
                gists: Vec::new(),
                state: ListState::default(),
                uploader,
            }
        }

        /// Generates a GitHub Gist URL from a gist ID
        fn get_gist_url(&self, gist_id: &str) -> String {
            format!("https://gist.github.com/{}", gist_id)
        }

        /// Copies text to the system clipboard
        fn copy_to_clipboard(&self, text: &str) -> GridistResult<()> {
            let mut clipboard = Clipboard::new().map_err(|e| {
                GridistError::Other(anyhow::anyhow!("Failed to initialize clipboard: {}", e))
            })?;
            clipboard.set_text(text).map_err(|e| {
                GridistError::Other(anyhow::anyhow!("Failed to set clipboard contents: {}", e))
            })?;
            Ok(())
        }

        /// Runs the interactive TUI for gist management
        pub async fn run(&mut self) -> GridistResult<()> {
            // Setup terminal
            enable_raw_mode()?;
            let mut stdout = io::stdout();
            stdout.execute(EnterAlternateScreen)?;
            let backend = CrosstermBackend::new(stdout);
            let mut terminal = Terminal::new(backend)?;

            // Load initial gists
            self.refresh_gists().await?;

            loop {
                terminal.draw(|f| {
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .margin(1)
                        .constraints([
                            Constraint::Length(1),
                            Constraint::Min(0),
                            Constraint::Length(1),
                        ])
                        .split(f.size());

                    // Title
                    let title = "Gridist Gist Manager".to_string();
                    let title_widget = ratatui::widgets::Paragraph::new(title)
                        .style(Style::default().fg(Color::Cyan));
                    f.render_widget(title_widget, chunks[0]);

                    // Gist list
                    let items: Vec<ListItem> = self.gists
                        .iter()
                        .map(|gist| {
                            let created_at = DateTime::parse_from_rfc3339(&gist.created_at)
                                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                                .unwrap_or_else(|_| gist.created_at.clone());

                            ListItem::new(format!(
                                "{} - {} ({})",
                                gist.id,
                                gist.description,
                                created_at
                            ))
                        })
                        .collect();

                    let list = List::new(items)
                        .block(Block::default().borders(Borders::ALL).title("Gists"))
                        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

                    f.render_stateful_widget(list, chunks[1], &mut self.state);

                    // Help text
                    let help_text = "↑↓: Navigate | c: Copy URL | o: Open in Browser | d: Delete | r: Refresh | q: Quit";
                    let help_widget = ratatui::widgets::Paragraph::new(help_text)
                        .style(Style::default().fg(Color::Gray));
                    f.render_widget(help_widget, chunks[2]);
                })?;

                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Up => self.previous(),
                        KeyCode::Down => self.next(),
                        KeyCode::Char('c') => {
                            if let Some(gist) = self.selected_gist() {
                                let url = self.get_gist_url(&gist.id);
                                if let Err(e) = self.copy_to_clipboard(&url) {
                                    error!("Failed to copy to clipboard: {}", e);
                                }
                            }
                        }
                        KeyCode::Char('o') => {
                            if let Some(gist) = self.selected_gist() {
                                let url = self.get_gist_url(&gist.id);
                                if let Err(e) = open::that(&url) {
                                    error!("Failed to open URL in browser: {}", e);
                                }
                            }
                        }
                        KeyCode::Char('d') => {
                            if let Some(gist) = self.selected_gist() {
                                let _ = self.uploader.delete_gist(&gist.id).await;
                                let _ = self.refresh_gists().await;
                            }
                        }
                        KeyCode::Char('r') => {
                            let _ = self.refresh_gists().await;
                        }
                        _ => {}
                    }
                }
            }

            // Restore terminal
            disable_raw_mode()?;
            terminal.backend_mut().execute(LeaveAlternateScreen)?;
            terminal.show_cursor()?;

            Ok(())
        }

        /// Refreshes the list of gists from GitHub
        async fn refresh_gists(&mut self) -> GridistResult<()> {
            let previous_selected = self.state.selected();
            self.gists = self.uploader.list_gists().await?;

            // Update selection after refresh
            if self.gists.is_empty() {
                self.state.select(None);
            } else if previous_selected.is_none() {
                self.state.select(Some(0));
            } else {
                let new_index = previous_selected
                    .unwrap()
                    .min(self.gists.len().saturating_sub(1));
                self.state.select(Some(new_index));
            }
            Ok(())
        }

        /// Moves the selection to the next gist in the list
        fn next(&mut self) {
            let i = match self.state.selected() {
                Some(i) => {
                    if i >= self.gists.len().saturating_sub(1) {
                        0
                    } else {
                        i + 1
                    }
                }
                None => 0,
            };
            self.state.select(Some(i));
        }

        /// Moves the selection to the previous gist in the list
        fn previous(&mut self) {
            let i = match self.state.selected() {
                Some(i) => {
                    if i == 0 {
                        self.gists.len().saturating_sub(1)
                    } else {
                        i - 1
                    }
                }
                None => 0,
            };
            self.state.select(Some(i));
        }

        /// Returns the currently selected gist, if any
        fn selected_gist(&self) -> Option<&GistInfo> {
            self.state.selected().and_then(|i| self.gists.get(i))
        }
    }
}
