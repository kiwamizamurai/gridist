# 🎨 Gridist (grid + gist)

> Transform your GitHub profile into an art gallery with smart image grids! Perfect for developers who want their profile to stand out.

> [!TIP]
> [Please check this example](https://github.com/kiwamizamurai)

<table>
<tr></tr>
<td width="50%">
<a href="https://gist.github.com/kiwamizamurai/3a8e31b049b62eb2841a6524dd58f364">
<img width="100%" src="https://gist.githubusercontent.com/kiwamizamurai/3a8e31b049b62eb2841a6524dd58f364/raw/466cb5179a9402d2e485f78049a4b804c54691f3/kiwamizamurai.0.png">
</a>
</td>
<td width="50%">
<a href="https://gist.github.com/kiwamizamurai/da4bd2d26a32ff36999c098b0f5e9fb4">
<img width="100%" src="https://gist.githubusercontent.com/kiwamizamurai/da4bd2d26a32ff36999c098b0f5e9fb4/raw/54cc0d2291ae735e9e79be7fb60ca4247e98ab87/kiwamizamurai.1.png">
</a>
</td>
</tr>
<tr>
<td width="50%">
<a href="https://gist.github.com/kiwamizamurai/d2c04c16c1c45773ea1e239404209ccb">
<img width="100%" src="https://gist.githubusercontent.com/kiwamizamurai/d2c04c16c1c45773ea1e239404209ccb/raw/b63aada36020e7f9225878cd651c95bbece276b9/kiwamizamurai.2.png">
</a>
</td>
<td width="50%">
<a href="https://gist.github.com/kiwamizamurai/dbc8a180b4e914afb1d5239f4a19cb8e">
<img width="100%" src="https://gist.githubusercontent.com/kiwamizamurai/dbc8a180b4e914afb1d5239f4a19cb8e/raw/1698fef9de614cfac2709df0694b7a4bd3ed7eb2/kiwamizamurai.3.png">
</a>
</td>
</tr>
<tr>
<td width="50%">
<a href="https://gist.github.com/kiwamizamurai/df8441a5bcfb45170b3add2bc92e9efd">
<img width="100%" src="https://gist.githubusercontent.com/kiwamizamurai/df8441a5bcfb45170b3add2bc92e9efd/raw/9782ae13f2d01cfe5e112bac744170fdc7eb1db5/kiwamizamurai.4.png">
</a>
</td>
<td width="50%">
<a href="https://gist.github.com/kiwamizamurai/01f3dd8feb6d581a569934d6d664cb37">
<img width="100%" src="https://gist.githubusercontent.com/kiwamizamurai/01f3dd8feb6d581a569934d6d664cb37/raw/37ae2056ce3cfa4f5b6d000b17bc341a5f8e0669/kiwamizamurai.5.png">
</a>
</td>
</tr>
</table>

## 🌟 What is Gridist?

Gridist is a powerful tool that transforms your images into eye-catching grid layouts for your GitHub profile. Whether you want to showcase your artwork, display your project screenshots, or just make your profile more visually appealing, Gridist makes it simple and elegant.

## ✨ Features

- 🖼️ **Smart Image Splitting**: Automatically splits images into perfectly sized grid pieces for GitHub profile display
- 🎬 **GIF Support**: Works with both static PNG images and animated GIFs
- 🔄 **GitHub Integration**: Seamlessly uploads split images to GitHub Gists with just one command
- 🎯 **Profile Ready**: Creates grid layouts that are perfectly sized for GitHub profile pinned gists
- 🖥️ **Simple CLI**: User-friendly command line interface with intuitive commands

## 📦 Installation

Choose your preferred installation method:

```bash
# Using Homebrew
brew install kiwamizamurai/tap/gridist

# Using Cargo
cargo install gridist

# Using Binary (macOS/Linux/Windows)
# 1. Download the latest binary from:
#    https://github.com/kiwamizamurai/gridist/releases
# 2. Extract and add to your PATH
```

## 🚀 Usage

### CLI Commands

Gridist provides two main commands: `upload` for splitting and uploading images, and `manage` for managing your uploaded gists.

```bash
gridist upload /images/your-image.png -t $(gh auth token)

gridist manage -t $(gh auth token)
```

## 🎮 CLI Reference

Global options:
- `-d, --debug`: Enable debug logging

Upload command options:
- `FILE`: Path to the image file (PNG or GIF)
- `-t, --token`: GitHub personal access token (can also be set via `GITHUB_TOKEN` environment variable)

Manage command options:
- `-t, --token`: GitHub personal access token (can also be set via `GITHUB_TOKEN` environment variable)

### GitHub Actions Integration

<details>
<summary>🚧 Work in Progress</summary>

To use Gridist with GitHub Actions, follow these steps:

1. Create a `.github/workflows/` directory in your repository
2. Create a new workflow file (e.g., `profile-grid.yml`) with the following content:

```yaml
name: Update Profile Grid

on:
  # Run when images are updated
  push:
    paths:
      - 'images/**'  # When files in the images/ directory change
  # Manual trigger option
  workflow_dispatch:

jobs:
  update-grid:
    uses: kiwamizamurai/gridist/.github/workflows/gridist.yml@main
    with:
      image_path: 'images/profile.png'  # Path to the image you want to split
    secrets:
      GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

3. Create an `images/` directory in your repository and place your image there
4. The workflow will automatically run when you push image changes
5. You can also manually trigger the workflow from the GitHub Actions tab

</details>

## 🤝 Contributing

PRs welcome! Check out our [contribution guidelines](.github/pull_request_template.md).

## 🔗 References

- [GitHub Gist Image Upload Reference](https://stackoverflow.com/questions/16425770/gist-how-are-images-uploaded-to-a-gist/43150165#43150165)
- [GitHub Gist API Reference](https://docs.github.com/en/rest/gists?apiVersion=2022-11-28)
