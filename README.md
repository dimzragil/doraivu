<div align="center">

# Doraivu
**Pure Rust TUI for Google Drive**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Build Status](https://img.shields.io/github/actions/workflow/status/dimzragil/doraivu/rust.yml?branch=main)](https://github.com/dimzragil/doraivu/actions)
[![Crates.io](https://img.shields.io/crates/v/doraivu.svg)](https://crates.io/crates/doraivu)

</div>

**Doraivu** is a fast, native, keyboard-centric terminal user interface (TUI) client for Google Drive. Built completely in Rust, it requires absolutely zero C-dependencies (powered by `rustls`), making it incredibly lightweight and portable across systems. Say goodbye to bloated web clients and hello to terminal velocity.

---

## ✨ Killer Features

- 🦀 **Pure Rust & Blazingly Fast**: Runs natively on your machine with minimal resource footprint. Wayland and X11 terminal ready out of the box.
- 🎬 **Direct Media Streaming**: Press `v` or `m` to stream video and audio directly to `mpv` without needing to download the entire file first.
- 🖼️ **Inline Image Preview**: Press `p` to view images directly inside the terminal. Leverages `ratatui-image` for Kitty/Sixel graphics protocol support.
- 📁 **Smart Path Uploads**: `mkdir -p` style recursive folder creation. Upload nested directories seamlessly.
- ⚡ **Queue Manager**: IDM-style download and upload tracker! Features ASCII sparkline (Braille) speed animations, real-time bandwidth charts, and full pause/resume support per-item.
- 🗑️ **Trash Management**: Integrated recycle bin. Restore or permanently obliterate files without leaving your terminal.

## 🚀 Installation

### 1-Liner Script (Linux/macOS)

The quickest way to install the pre-compiled binary:

```bash
curl -sSfL https://raw.githubusercontent.com/dimzragil/doraivu/main/install.sh | bash
```

### Build from Source

If you have Rust and Cargo installed, you can easily build it from source:

```bash
git clone https://github.com/dimzragil/doraivu.git
cd doraivu
cargo install --path .
```

*Note: You will need to provide your own Google OAuth Client ID/Secret via environment variables or the setup prompt on first run.*

## ⌨️ Keybindings & Usage

Doraivu is designed with Vim-style navigation in mind. No mouse needed!

| Keybind | Action | Description |
|---------|--------|-------------|
| `j` / `↓` | Move Down | Navigate down the file list |
| `k` / `↑` | Move Up | Navigate up the file list |
| `l` / `Enter`| Enter Folder | Open the selected directory |
| `h` / `Backspace`| Go Back | Return to the parent directory |
| `/` | Search | Real-time file and folder search |
| `a` | Select/Deselect | Multi-select files for batch operations |
| `A` | Clear Selection | Clear all selected files |
| `d` | Download | Queue selected file(s) for download |
| `D` | Download Tracker | Open the Download Queue Manager |
| `u` | Upload | Open the Upload menu (Target & Local Path) |
| `U` | Upload Tracker | Open the Upload Queue Manager |
| `v` / `m` | Stream Media | Launch `mpv` to stream the selected media file |
| `p` | Toggle Preview | Show/hide inline image preview (requires Sixel/Kitty) |
| `e` | Edit & Sync | Download file to `$EDITOR`, edit, and auto-upload on save |
| `x` / `Del` | Move to Trash | Send the selected file(s) to the Trash |
| `T` | Open Trash | View trashed items (Restore or Delete Permanently) |
| `r` | Refresh | Refresh current directory and quota |
| `q` | Quit | Exit Doraivu safely |

---

<div align="center">
  <i>Made with 🦀 for Terminal lovers.</i>
</div>
