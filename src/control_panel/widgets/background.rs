/// Background wallpaper widget.
///
/// Allows selecting a wallpaper from a configured folder and applying it via `feh`.
/// Configuration is persisted to `~/.config/i3more/background.json`.

use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::prelude::*;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const THUMB_SIZE: i32 = 80;
const PREVIEW_WIDTH: i32 = 200;
const PREVIEW_HEIGHT: i32 = 120;
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

/// Feh background modes.
const MODES: &[&str] = &["fill", "scale", "center", "tile", "max"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundConfig {
    pub folder: String,
    pub current: String,
    pub mode: String,
}

impl Default for BackgroundConfig {
    fn default() -> Self {
        let folder = dirs::home_dir()
            .map(|h| h.join("Pictures/Wallpapers"))
            .unwrap_or_else(|| PathBuf::from("~/Pictures/Wallpapers"))
            .to_string_lossy()
            .to_string();
        Self {
            folder,
            current: String::new(),
            mode: "fill".to_string(),
        }
    }
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("i3more")
        .join("background.json")
}

fn load_config() -> BackgroundConfig {
    let path = config_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_config(config: &BackgroundConfig) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(config) {
        let _ = std::fs::write(&path, json);
    }
}

fn apply_background(image_path: &str, mode: &str) {
    let feh_mode = format!("--bg-{}", mode);
    let _ = std::process::Command::new("feh")
        .args([&feh_mode, image_path])
        .spawn();
}

fn scan_images(folder: &str) -> Vec<PathBuf> {
    let path = Path::new(folder);
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut images: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .map(|e| e.path())
        .collect();
    images.sort();
    images
}

fn load_thumbnail(path: &Path, width: i32, height: i32) -> Option<Pixbuf> {
    Pixbuf::from_file_at_scale(path, width, height, true).ok()
}

/// Set a pixbuf on a Picture widget via texture.
fn set_picture_pixbuf(picture: &gtk4::Picture, pb: &Pixbuf) {
    let texture = gtk4::gdk::Texture::for_pixbuf(pb);
    picture.set_paintable(Some(&texture));
}

/// Build the background widget. Returns the widget box.
pub fn build_widget() -> gtk4::Box {
    let config = Rc::new(RefCell::new(load_config()));

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    container.add_css_class("widget-background");
    container.set_margin_start(4);
    container.set_margin_end(4);
    container.set_margin_top(4);

    // Header
    let header = gtk4::Label::new(None);
    header.set_use_markup(true);
    header.set_markup(&format!(
        "{}  <span foreground=\"#ebdbb2\">Background</span>",
        crate::fa::fa_icon(crate::fa::IMAGE, "#a89984", 10)
    ));
    header.set_halign(gtk4::Align::Start);
    header.add_css_class("widget-section-title");
    container.append(&header);

    // Current wallpaper preview using Image (GTK4 0.10 compatible)
    let preview = gtk4::Picture::new();
    preview.set_size_request(PREVIEW_WIDTH, PREVIEW_HEIGHT);
    preview.add_css_class("widget-background-preview");
    preview.set_halign(gtk4::Align::Center);

    {
        let cfg = config.borrow();
        if !cfg.current.is_empty() {
            if let Some(pb) = load_thumbnail(Path::new(&cfg.current), PREVIEW_WIDTH, PREVIEW_HEIGHT)
            {
                set_picture_pixbuf(&preview, &pb);
            }
        }
    }
    container.append(&preview);

    // Controls row: folder button + mode dropdown
    let controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    controls.set_margin_start(4);
    controls.set_margin_end(4);
    controls.set_margin_top(4);

    let folder_btn = gtk4::Button::with_label("Folder\u{2026}");
    folder_btn.add_css_class("notification-panel-clear");
    controls.append(&folder_btn);

    // Mode dropdown
    let mode_label = gtk4::Label::new(Some("Mode:"));
    mode_label.add_css_class("widget-audio-device-label");
    controls.append(&mode_label);

    let mode_strings: Vec<&str> = MODES.to_vec();
    let mode_dropdown = gtk4::DropDown::from_strings(&mode_strings);
    mode_dropdown.add_css_class("widget-background-mode");
    {
        let cfg = config.borrow();
        let idx = MODES.iter().position(|m| *m == cfg.mode).unwrap_or(0);
        mode_dropdown.set_selected(idx as u32);
    }
    mode_dropdown.set_hexpand(true);
    controls.append(&mode_dropdown);

    container.append(&controls);

    // Image grid
    let flow_box = gtk4::FlowBox::new();
    flow_box.set_homogeneous(true);
    flow_box.set_max_children_per_line(4);
    flow_box.set_min_children_per_line(3);
    flow_box.set_column_spacing(4);
    flow_box.set_row_spacing(4);
    flow_box.set_selection_mode(gtk4::SelectionMode::Single);
    flow_box.add_css_class("widget-background-grid");

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scrolled.set_min_content_height(180);
    scrolled.set_max_content_height(180);
    scrolled.set_child(Some(&flow_box));
    scrolled.set_margin_top(4);
    scrolled.set_margin_start(4);
    scrolled.set_margin_end(4);
    container.append(&scrolled);

    // Populate grid
    populate_grid(&flow_box, &config.borrow());

    // Folder button: open FileChooserDialog to pick folder
    let config_for_folder = config.clone();
    let flow_box_for_folder = flow_box.clone();
    let preview_for_folder = preview.clone();
    folder_btn.connect_clicked(move |btn| {
        let win = btn
            .root()
            .and_then(|r| r.downcast::<gtk4::Window>().ok());

        let dialog = gtk4::FileChooserDialog::new(
            Some("Select Wallpaper Folder"),
            win.as_ref(),
            gtk4::FileChooserAction::SelectFolder,
            &[
                ("Cancel", gtk4::ResponseType::Cancel),
                ("Select", gtk4::ResponseType::Accept),
            ],
        );

        let cfg = config_for_folder.clone();
        let fb = flow_box_for_folder.clone();
        let pv = preview_for_folder.clone();
        dialog.connect_response(move |dlg, response| {
            if response == gtk4::ResponseType::Accept {
                if let Some(file) = dlg.file() {
                    if let Some(path) = file.path() {
                        let folder_str = path.to_string_lossy().to_string();
                        {
                            let mut c = cfg.borrow_mut();
                            c.folder = folder_str;
                            c.current.clear();
                            save_config(&c);
                        }
                        pv.set_paintable(Option::<&gtk4::gdk::Texture>::None);
                        populate_grid(&fb, &cfg.borrow());
                    }
                }
            }
            dlg.close();
        });

        dialog.present();
    });

    // Image selection handler
    let config_for_select = config.clone();
    let preview_for_select = preview.clone();
    let mode_dropdown_for_select = mode_dropdown.clone();
    flow_box.connect_child_activated(move |_, child| {
        if let Some(picture) = child.child().and_then(|w| w.downcast::<gtk4::Picture>().ok()) {
            if let Some(tooltip) = picture.tooltip_text() {
                let path_str = tooltip.to_string();
                let mode_idx = mode_dropdown_for_select.selected() as usize;
                let mode = MODES.get(mode_idx).unwrap_or(&"fill");

                // Update preview
                if let Some(pb) =
                    load_thumbnail(Path::new(&path_str), PREVIEW_WIDTH, PREVIEW_HEIGHT)
                {
                    set_picture_pixbuf(&preview_for_select, &pb);
                }

                // Apply and save
                apply_background(&path_str, mode);
                {
                    let mut cfg = config_for_select.borrow_mut();
                    cfg.current = path_str;
                    save_config(&cfg);
                }
            }
        }
    });

    // Mode change handler
    let config_for_mode = config;
    mode_dropdown.connect_selected_notify(move |dd| {
        let idx = dd.selected() as usize;
        let mode = MODES.get(idx).unwrap_or(&"fill");
        let mut cfg = config_for_mode.borrow_mut();
        cfg.mode = mode.to_string();
        if !cfg.current.is_empty() {
            apply_background(&cfg.current, mode);
        }
        save_config(&cfg);
    });

    container
}

fn populate_grid(flow_box: &gtk4::FlowBox, config: &BackgroundConfig) {
    // Remove existing children
    while let Some(child) = flow_box.first_child() {
        flow_box.remove(&child);
    }

    let images = scan_images(&config.folder);
    for img_path in &images {
        let picture = gtk4::Picture::new();
        picture.set_size_request(THUMB_SIZE, THUMB_SIZE);
        picture.add_css_class("widget-background-thumb");
        picture.set_tooltip_text(Some(&img_path.to_string_lossy()));

        if img_path.to_string_lossy() == config.current {
            picture.add_css_class("widget-background-thumb-active");
        }

        if let Some(pb) = load_thumbnail(img_path, THUMB_SIZE, THUMB_SIZE) {
            set_picture_pixbuf(&picture, &pb);
        }

        flow_box.insert(&picture, -1);
    }
}
