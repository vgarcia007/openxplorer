// SPDX-License-Identifier: AGPL-3.0-only
//! Change icon… and Restore default icon on the General tab (PROP-016),
//! as Files' Properties and Dolphin's icon button offer them. The choice
//! is stored as `GVfs` metadata (`metadata::custom-icon`, the key Files
//! uses), so both file managers show it; the default Windows-style art
//! stays for every other item.

use gtk::prelude::*;
use gtk::{gio, glib};
use std::rc::Rc;

use super::general_panel::glyph_button;
use crate::dialog::DialogFrame;
use crate::folder_view::{CUSTOM_ICON, CUSTOM_ICON_NAME};
use crate::icons::{self, Art, ArtImage, Icon, ALL_ICONS};

/// Change icon…, and Restore default icon when `has_custom_icon`, for the
/// local item at `uri`.
pub(super) fn icon_buttons(row: &gtk::Box, uri: &str, has_custom_icon: bool) {
    let change = glyph_button(ox_core::i18n::gettext_static("Change icon…"), Icon::Image);
    let target = uri.to_owned();
    change.connect_clicked(move |button| choose_icon(button, &target));
    row.append(&change);
    let theme = glyph_button(ox_core::i18n::gettext_static("Choose from icons…"), Icon::Image);
    let target = uri.to_owned();
    theme.connect_clicked(move |button| choose_theme_icon(button, &target));
    row.append(&theme);
    if has_custom_icon {
        let restore = glyph_button(
            ox_core::i18n::gettext_static("Restore default icon"),
            Icon::ArrowReset,
        );
        let target = uri.to_owned();
        restore.connect_clicked(move |button| {
            let button = button.clone();
            let target = target.clone();
            glib::spawn_future_local(async move {
                let result = set_custom_icon(&target, None).await;
                report(&button, &target, result);
            });
        });
        row.append(&restore);
    }
}

/// Asks for a picture, then makes it the icon of the item at `uri`.
fn choose_icon(button: &gtk::Button, uri: &str) {
    let images = gtk::FileFilter::new();
    images.set_name(Some("Images"));
    images.add_mime_type("image/*");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&images);
    let dialog = gtk::FileDialog::builder()
        .title(ox_core::i18n::gettext("Choose an icon"))
        .modal(true)
        .filters(&filters)
        .build();
    let window = button.root().and_downcast::<gtk::Window>();
    let button = button.clone();
    let uri = uri.to_owned();
    glib::spawn_future_local(async move {
        let Ok(chosen) = dialog.open_future(window.as_ref()).await else {
            return;
        };
        let result = set_custom_icon(&uri, Some(chosen.uri().to_string())).await;
        report(&button, &uri, result);
    });
}

/// Stores `icon` as the custom icon of the item at `uri`, or removes it
/// for `None`. GIO's Rust binding cannot unset an attribute, so removing
/// runs `gio set -t unset`, as Files' "Reset" does through GIO.
///
/// # Errors
///
/// GIO's error, or the failure of `gio set`.
pub(crate) async fn set_custom_icon(uri: &str, icon: Option<String>) -> Result<(), glib::Error> {
    let Some(icon) = icon else {
        unset_attribute(uri, CUSTOM_ICON).await?;
        return unset_attribute(uri, CUSTOM_ICON_NAME).await;
    };
    unset_attribute(uri, CUSTOM_ICON_NAME).await?;
    set_attribute(uri, CUSTOM_ICON, &icon).await
}

async fn unset_attribute(uri: &str, key: &str) -> Result<(), glib::Error> {
    let unset = gio::Subprocess::newv(
        &[
            "gio".as_ref(),
            "set".as_ref(),
            "-t".as_ref(),
            "unset".as_ref(),
            uri.as_ref(),
            key.as_ref(),
        ],
        gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_SILENCE,
    )?;
    unset.wait_check_future().await
}

async fn set_attribute(uri: &str, key: &str, value: &str) -> Result<(), glib::Error> {
    let file = gio::File::for_uri(uri);
    let info = gio::FileInfo::new();
    info.set_attribute_string(key, value);
    file.set_attributes_future(&info, gio::FileQueryInfoFlags::NONE, glib::Priority::DEFAULT)
        .await
        .map(|_| ())
}

async fn set_theme_icon(uri: &str, name: &str) -> Result<(), glib::Error> {
    unset_attribute(uri, CUSTOM_ICON).await?;
    set_attribute(uri, CUSTOM_ICON_NAME, name).await
}

/// Opens the bundled icons first, with the desktop theme as another source.
fn choose_theme_icon(button: &gtk::Button, uri: &str) {
    let Some(parent) = button.root().and_downcast::<gtk::Window>() else {
        return;
    };
    let theme = gtk::IconTheme::for_display(&gtk::prelude::WidgetExt::display(&parent));
    let mut names: Vec<String> = theme
        .icon_names()
        .iter()
        .map(ToString::to_string)
        .filter(|name| !name.ends_with("-symbolic"))
        .collect();
    names.sort_unstable();
    names.dedup();
    let window = gtk::Window::builder()
        .title(ox_core::i18n::gettext("Choose an icon"))
        .transient_for(&parent)
        .modal(true)
        .default_width(600)
        .default_height(480)
        .build();
    let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
    column.set_margin_top(12);
    column.set_margin_bottom(12);
    column.set_margin_start(12);
    column.set_margin_end(12);
    let source = gtk::DropDown::from_strings(&[
        &ox_core::i18n::gettext("Places and folders"),
        &ox_core::i18n::gettext("All OpenXplorer icons"),
        &ox_core::i18n::gettext("System theme icons"),
    ]);
    source.set_halign(gtk::Align::Start);
    column.append(&source);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some(&ox_core::i18n::gettext("Search icons…")));
    column.append(&search);
    let scroller = gtk::ScrolledWindow::builder().vexpand(true).build();
    let gallery = gtk::FlowBox::builder()
        .max_children_per_line(7)
        .min_children_per_line(3)
        .selection_mode(gtk::SelectionMode::None)
        .row_spacing(8)
        .column_spacing(8)
        .build();
    scroller.set_child(Some(&gallery));
    column.append(&scroller);
    window.set_child(Some(&column));
    let picker = Rc::new(IconPicker {
        gallery,
        theme,
        names,
        source: source.clone(),
        button: button.clone(),
        uri: uri.to_owned(),
        window: window.downgrade(),
    });
    picker.render("");
    let on_search = Rc::clone(&picker);
    search.connect_search_changed(move |entry| on_search.render(entry.text().as_str()));
    let on_source = Rc::downgrade(&picker);
    let search_weak = search.downgrade();
    source.connect_selected_notify(move |_| {
        if let (Some(picker), Some(search)) = (on_source.upgrade(), search_weak.upgrade()) {
            picker.render(search.text().as_str());
        }
    });
    window.present();
}

struct IconPicker {
    gallery: gtk::FlowBox,
    theme: gtk::IconTheme,
    names: Vec<String>,
    source: gtk::DropDown,
    button: gtk::Button,
    uri: String,
    window: glib::WeakRef<gtk::Window>,
}

/// Useful folder and place symbols, shown before the full bundled set.
const PLACE_ICONS: &[Icon] = &[
    Icon::FileFolder,
    Icon::Folder,
    Icon::Desktop,
    Icon::ArrowDownload,
    Icon::Document,
    Icon::Image,
    Icon::MusicNote,
    Icon::Video,
    Icon::Home,
    Icon::CloudColor20,
    Icon::Cloud,
    Icon::CloudSync,
    Icon::CloudArrowDown,
    Icon::FolderBriefcase,
    Icon::FolderDocument,
    Icon::FolderGlobe,
    Icon::FolderMail,
    Icon::FolderPeople,
    Icon::FolderPerson,
    Icon::FolderMultiple,
    Icon::FolderSync,
    Icon::FolderLink,
    Icon::FolderOpen,
    Icon::FolderSearch,
    Icon::BookColor20,
    Icon::BriefcaseColor20,
    Icon::CalendarColor20,
    Icon::CameraColor20,
    Icon::DatabaseColor20,
    Icon::GlobeColor20,
    Icon::HeartColor20,
    Icon::MailColor20,
    Icon::StarColor20,
    Icon::Games,
    Icon::Wallet,
    Icon::Archive,
    Icon::Building,
    Icon::Food,
    Icon::Key,
    Icon::People,
    Icon::Tag,
];

impl IconPicker {
    fn render(&self, query: &str) {
        while let Some(child) = self.gallery.first_child() {
            self.gallery.remove(&child);
        }
        let query = query.to_lowercase();
        if self.source.selected() < 2 {
            let icons: &[Icon] = if self.source.selected() == 0 {
                PLACE_ICONS
            } else {
                &ALL_ICONS
            };
            for &icon in icons {
                if icon.name().contains(&query) {
                    self.add_tile(icon.name(), icons::bundled_art(icon.name()));
                }
            }
            return;
        }
        let preferred = [
            "folder",
            "folder-home",
            "folder-documents",
            "folder-download",
            "folder-pictures",
            "folder-music",
            "folder-videos",
            "folder-publicshare",
            "folder-templates",
            "folder-favorites",
            "folder-network",
            "folder-remote",
        ];
        let candidates: Vec<&str> = if query.is_empty() {
            preferred
                .iter()
                .copied()
                .filter(|name| self.theme.has_icon(name))
                .collect()
        } else {
            self.names
                .iter()
                .filter(|name| name.to_lowercase().contains(&query))
                .take(140)
                .map(String::as_str)
                .collect()
        };
        for name in candidates {
            self.add_tile(name, None);
        }
    }

    fn add_tile(&self, name: &str, art: Option<Art>) {
        let tile = gtk::Button::new();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
        if let Some(art) = art {
            content.append(&ArtImage::new(art, 48));
        } else {
            let image = gtk::Image::from_icon_name(name);
            image.set_pixel_size(48);
            content.append(&image);
        }
        let label_text = name
            .strip_prefix("ox-")
            .unwrap_or(name)
            .trim_end_matches("-symbolic")
            .trim_end_matches("-color")
            .replace('-', " ");
        let label = gtk::Label::new(Some(&label_text));
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(14);
        content.append(&label);
        tile.set_child(Some(&content));
        tile.set_tooltip_text(Some(name));
        let name = name.to_owned();
        let uri = self.uri.clone();
        let button = self.button.clone();
        let window = self.window.clone();
        tile.connect_clicked(move |_| {
            if let Some(window) = window.upgrade() {
                window.close();
            }
            let uri = uri.clone();
            let name = name.clone();
            let button = button.clone();
            glib::spawn_future_local(async move {
                let result = set_theme_icon(&uri, &name).await;
                report(&button, &uri, result);
            });
        });
        self.gallery.insert(&tile, -1);
    }
}

/// Has the window redraw the item, or shows why the change failed.
fn report(button: &gtk::Button, uri: &str, result: Result<(), glib::Error>) {
    let window = button.root().and_downcast::<crate::window::BrowserWindow>();
    match result {
        Ok(()) => {
            if let Some(window) = window {
                window.refresh_item_icon(uri);
            }
        }
        Err(error) => {
            let frame = button
                .ancestor(DialogFrame::static_type())
                .and_downcast::<DialogFrame>();
            if let Some(frame) = frame {
                frame.show_error(&error.to_string());
            }
        }
    }
}
