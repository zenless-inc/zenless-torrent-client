//! Collapsible file tree with tri-state checkboxes (Add torrent dialog).

use crate::model::FileEntry;
use crate::shared::{kit, theme::Palette};
use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;

pub struct Node {
    pub name: String,
    pub size: u64,
    pub children: Vec<Node>,
    /// File index for leaves.
    pub file: Option<usize>,
    /// All file indices below this node.
    pub files: Vec<usize>,
}

impl Node {
    fn dir(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            size: 0,
            children: Vec::new(),
            file: None,
            files: Vec::new(),
        }
    }

    fn finalize(&mut self) {
        if let Some(i) = self.file {
            self.files = vec![i];
            return;
        }
        for c in &mut self.children {
            c.finalize();
        }
        // folders first, keep torrent order otherwise
        self.children.sort_by_key(|c| c.file.is_some());
        self.size = self.children.iter().map(|c| c.size).sum();
        self.files = self.children.iter().flat_map(|c| c.files.iter().copied()).collect();
    }
}

/// Builds the tree (padding files are skipped).
pub fn build(files: &[FileEntry]) -> Node {
    let mut root = Node::dir("");
    for (i, f) in files.iter().enumerate() {
        if f.padding || f.path.is_empty() {
            continue;
        }
        let (dirs, leaf) = f.path.split_at(f.path.len() - 1);
        let mut cur = &mut root;
        for d in dirs {
            let idx = match cur.children.iter().position(|c| c.file.is_none() && c.name == *d) {
                Some(idx) => idx,
                None => {
                    cur.children.push(Node::dir(d));
                    cur.children.len() - 1
                }
            };
            cur = &mut cur.children[idx];
        }
        cur.children.push(Node {
            name: leaf[0].clone(),
            size: f.len,
            children: Vec::new(),
            file: Some(i),
            files: Vec::new(),
        });
    }
    root.finalize();
    root
}

pub fn file_icon(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "wmv" | "ts" => icon::FILE_VIDEO,
        "mp3" | "flac" | "ogg" | "wav" | "m4a" | "opus" | "aac" => icon::FILE_AUDIO,
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" => icon::FILE_IMAGE,
        "zip" | "7z" | "rar" | "gz" | "xz" | "zst" | "tar" | "bz2" => icon::FILE_ZIP,
        "iso" | "img" | "dmg" => icon::DISC,
        "txt" | "nfo" | "md" | "srt" | "ass" | "sub" | "pdf" | "epub" => icon::FILE_TEXT,
        "exe" | "msi" | "appimage" | "deb" | "rpm" => icon::APP_WINDOW,
        _ => icon::FILE,
    }
}

/// Shows the children of `node`. Returns true when the selection changed.
pub fn show(ui: &mut egui::Ui, p: &Palette, node: &Node, selected: &mut [bool], id: egui::Id, depth: usize) -> bool {
    let mut changed = false;
    for child in &node.children {
        if let Some(i) = child.file {
            ui.horizontal(|ui| {
                ui.add_space(ui.spacing().indent);
                if let Some(v) = selected.get_mut(i) {
                    changed |= ui.add(egui::Checkbox::without_text(v)).changed();
                }
                ui.label(RichText::new(file_icon(&child.name)).color(p.text_dim));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(kit::human_bytes(child.size)).small().color(p.text_dim));
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(&child.name).truncate());
                    });
                });
            });
        } else {
            let cid = id.with(&child.name);
            let state = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                cid,
                depth == 0,
            );
            state
                .show_header(ui, |ui| {
                    let all = child.files.iter().all(|&i| selected.get(i).copied().unwrap_or(false));
                    let any = child.files.iter().any(|&i| selected.get(i).copied().unwrap_or(false));
                    let mut v = all;
                    if ui
                        .add(egui::Checkbox::without_text(&mut v).indeterminate(any && !all))
                        .changed()
                    {
                        for &i in &child.files {
                            if let Some(s) = selected.get_mut(i) {
                                *s = v;
                            }
                        }
                        changed = true;
                    }
                    ui.label(RichText::new(icon::FOLDER_SIMPLE).color(p.accent2));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(kit::human_bytes(child.size)).small().color(p.text_dim));
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(egui::Label::new(RichText::new(&child.name).strong()).truncate());
                        });
                    });
                })
                .body(|ui| {
                    changed |= show(ui, p, child, selected, cid, depth + 1);
                });
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(path: &str, len: u64) -> FileEntry {
        FileEntry {
            path: path.split('/').map(str::to_owned).collect(),
            len,
            padding: false,
        }
    }

    #[test]
    fn builds_nested_tree() {
        let mut pad = f(".pad/0", 5);
        pad.padding = true;
        let files = vec![f("readme.txt", 1), f("a/b/one.mkv", 10), pad, f("a/two.mkv", 20)];
        let root = build(&files);
        assert_eq!(root.size, 31);
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.children[0].name, "a"); // folders first
        assert_eq!(root.children[0].size, 30);
        let mut all = root.files.clone();
        all.sort();
        assert_eq!(all, vec![0, 1, 3]);
        assert_eq!(file_icon("x.MKV"), icon::FILE_VIDEO);
    }
}
