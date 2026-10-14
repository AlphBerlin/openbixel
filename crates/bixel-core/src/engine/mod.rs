//! Engine export: one target-neutral scene graph, thin per-target writers.
//!
//! The seam this module closes is the last hand-off in the pipeline. Before
//! this, a Bixel map reached a running game only by way of Tiled and a manual
//! import; the artist left the studio and became an integrator. Here the map is
//! lowered once into [`scene::EngineScene`] — an engine-agnostic description of
//! tilesets, tiles, layers and cells — and each engine gets a writer that does
//! nothing but spelling.
//!
//! Adding a target is therefore additive, never a refactor:
//!
//! 1. add an [`EngineTarget`] variant and its id/aliases,
//! 2. write `engine/<target>.rs` exposing `write(scene, opts) -> WriterOutput`,
//! 3. add target-specific [`preflight`] findings,
//! 4. match it in [`export`].
//!
//! Nothing engine-specific is allowed to leak into the scene graph, and no
//! writer is allowed to read [`crate::map::TileMap`] directly — that keeps the
//! lowering tested once instead of once per target.

pub mod flame;
pub mod godot;
pub mod png;
pub mod preflight;
pub mod scene;
pub mod tiled;

use crate::map_validate::Finding;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub use scene::EngineScene;

/// Which engine dialect to emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineTarget {
    /// Godot 4.x: `.tres` TileSet + `.tscn` TileMapLayer nodes + `@tool` driver.
    Godot,
    /// Flame (Flutter): Tiled-format JSON plus a pre-packed sprite sheet.
    Flame,
    /// Tiled 1.10 `.tmj` bundle — the portable path, zipped with its images.
    Tiled,
}

impl EngineTarget {
    pub const ALL: [EngineTarget; 3] = [Self::Godot, Self::Flame, Self::Tiled];

    /// Stable id used on the FFI boundary and in the manifest.
    pub fn id(self) -> &'static str {
        match self {
            EngineTarget::Godot => "godot",
            EngineTarget::Flame => "flame",
            EngineTarget::Tiled => "tiled",
        }
    }

    /// Accept the id plus the aliases a user or skill is likely to type.
    pub fn from_id(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "godot" | "godot4" | "godot-4" | "godot_4" => Some(EngineTarget::Godot),
            "flame" | "flutter" | "flame_tiled" => Some(EngineTarget::Flame),
            "tiled" | "tmj" | "json" => Some(EngineTarget::Tiled),
            _ => None,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            EngineTarget::Godot => "Godot 4",
            EngineTarget::Flame => "Flame (Flutter)",
            EngineTarget::Tiled => "Tiled 1.10",
        }
    }

    /// Folder the writer puts its files under inside the export root.
    pub fn folder(self) -> &'static str {
        match self {
            EngineTarget::Godot => "godot",
            EngineTarget::Flame => "flame",
            EngineTarget::Tiled => "tiled",
        }
    }

    /// Path (relative to the export root) the user is meant to open first.
    pub fn entry_hint(self) -> &'static str {
        match self {
            EngineTarget::Godot => "godot/README.md",
            EngineTarget::Flame => "flame/README.md",
            EngineTarget::Tiled => "tiled/<map>.tmj",
        }
    }
}

/// An RGBA8 image a writer wants to hand to the engine.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl EngineImage {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, String> {
        let expected = width as usize * height as usize * 4;
        if rgba.len() != expected {
            return Err(format!(
                "image buffer is {} bytes, expected {expected} for {width}x{height} RGBA",
                rgba.len()
            ));
        }
        Ok(EngineImage {
            width,
            height,
            rgba,
        })
    }

    /// Encode as PNG, optionally at an integer nearest-neighbour scale.
    pub fn to_png(&self, scale: u32) -> Result<Vec<u8>, String> {
        png::encode_rgba_scaled(self.width, self.height, &self.rgba, scale)
    }
}

/// One file in an export bundle. Either text or an image, never both.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportFile {
    pub path: String,
    pub text: Option<String>,
    pub image: Option<EngineImage>,
    /// Short human note shown in the export summary and manifest.
    pub note: String,
}

impl ExportFile {
    pub fn text(path: impl Into<String>, body: impl Into<String>, note: impl Into<String>) -> Self {
        ExportFile {
            path: path.into(),
            text: Some(body.into()),
            image: None,
            note: note.into(),
        }
    }

    pub fn json(path: impl Into<String>, value: &serde_json::Value, note: impl Into<String>) -> Self {
        let body = serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".into());
        ExportFile::text(path, body, note)
    }

    pub fn png(
        path: impl Into<String>,
        image: EngineImage,
        note: impl Into<String>,
    ) -> Self {
        ExportFile {
            path: path.into(),
            text: None,
            image: Some(image),
            note: note.into(),
        }
    }

    pub fn kind(&self) -> &'static str {
        if self.image.is_some() {
            "png"
        } else {
            "text"
        }
    }

    /// Uncompressed payload size in bytes.
    pub fn byte_len(&self) -> usize {
        match (&self.text, &self.image) {
            (Some(t), _) => t.len(),
            (_, Some(img)) => img.rgba.len(),
            _ => 0,
        }
    }

    /// Bytes as they would land on disk (images get encoded).
    pub fn encode(&self, scale: u32) -> Result<Vec<u8>, String> {
        match (&self.text, &self.image) {
            (Some(text), _) => Ok(text.as_bytes().to_vec()),
            (_, Some(image)) => image.to_png(scale),
            _ => Err(format!("export file {:?} has no content", self.path)),
        }
    }

    fn assert_portable(&self) -> Result<(), String> {
        if self.path.is_empty() {
            return Err("export file with an empty path".into());
        }
        if self.path.starts_with('/') || self.path.contains('\\') {
            return Err(format!(
                "export path {:?} must be relative and use forward slashes",
                self.path
            ));
        }
        if self.path.split('/').any(|seg| seg == ".." || seg.is_empty()) {
            return Err(format!("export path {:?} escapes the export root", self.path));
        }
        Ok(())
    }
}

/// Knobs a user can turn; all have usable defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportOptions {
    /// Integer nearest-neighbour upscale applied to emitted PNGs.
    pub scale: u32,
    /// Emit object layers (spawns, doors, triggers) rather than skipping them.
    pub emit_objects: bool,
    /// Emit per-tile collision into the target's physics representation.
    pub emit_collision: bool,
    /// Emit the Godot `@tool` driver script / equivalent glue.
    pub emit_driver: bool,
    /// Emit the `bixel-export.json` manifest in the bundle.
    pub emit_manifest: bool,
    /// Base name used for the entry document and image files.
    pub base_name: String,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            scale: 1,
            emit_objects: true,
            emit_collision: true,
            emit_driver: true,
            emit_manifest: true,
            base_name: "map".into(),
        }
    }
}

impl ExportOptions {
    pub fn named(base_name: impl Into<String>) -> Self {
        ExportOptions {
            base_name: sanitize_stem(&base_name.into()),
            ..Default::default()
        }
    }

    pub fn with_scale(mut self, scale: u32) -> Self {
        self.scale = scale.max(1);
        self
    }
}

/// A writer's product: where to look first, plus the files.
#[derive(Debug, Clone)]
pub struct WriterOutput {
    pub entry: String,
    pub files: Vec<ExportFile>,
}

/// The whole result of an export, ready to write to disk or serialize.
#[derive(Debug, Clone)]
pub struct EngineBundle {
    pub target: EngineTarget,
    /// Relative path of the file to open first.
    pub entry: String,
    pub files: Vec<ExportFile>,
    pub findings: Vec<Finding>,
    /// One-line summary suitable for a toast in the app.
    pub summary: String,
}

impl EngineBundle {
    /// Total encoded size in bytes.
    pub fn encoded_bytes(&self) -> Result<usize, String> {
        self.files
            .iter()
            .map(|f| f.encode(1).map(|b| b.len()))
            .try_fold(0usize, |acc, r| r.map(|n| acc + n))
    }

    pub fn image_files(&self) -> impl Iterator<Item = &ExportFile> {
        self.files.iter().filter(|f| f.image.is_some())
    }

    /// Count of blocking findings; the bundle is still emitted (a warning never
    /// stops an artist from getting their files) but the UI should surface it.
    pub fn error_count(&self) -> usize {
        self.findings.iter().filter(|f| f.level == "error").count()
    }

    pub fn warn_count(&self) -> usize {
        self.findings.iter().filter(|f| f.level == "warning").count()
    }

    /// The `bixel-export.json` index: what this bundle is, where it goes, and
    /// what to do next. This is the artifact an AI agent reads to help the user
    /// wire the export into their project.
    pub fn manifest(&self, scene: &EngineScene, opts: &ExportOptions) -> serde_json::Value {
        let files: Vec<serde_json::Value> = self
            .files
            .iter()
            .map(|f| {
                serde_json::json!({
                    "path": f.path,
                    "kind": f.kind(),
                    "bytes": f.encode(opts.scale).map(|b| b.len()).unwrap_or(0),
                    "note": f.note,
                })
            })
            .collect();
        serde_json::json!({
            "format": "bixel-export/1",
            "target": self.target.id(),
            "targetName": self.target.display_name(),
            "entry": self.entry,
            "scale": opts.scale,
            "map": {
                "name": scene.name,
                "tileWidth": scene.tile_width,
                "tileHeight": scene.tile_height,
                "orientation": scene.orientation,
                "infinite": scene.infinite,
                "pixelWidth": scene.pixel_width(),
                "pixelHeight": scene.pixel_height(),
            },
            "counts": {
                "tilesets": scene.tilesets.len(),
                "layers": scene.layers.len(),
                "cells": scene.cell_count(),
                "objects": scene.object_count(),
                "collisionTiles": scene.collision_tile_count(),
            },
            "files": files,
            "findings": self.findings,
            "summary": self.summary,
        })
    }
}

/// Lower a map and write it out for one engine.
///
/// Order is deliberate: preflight runs on the *scene*, so every target sees the
/// same warnings about the same data before any writer has a chance to paper
/// over a gap.
pub fn export(
    target: EngineTarget,
    map: &crate::map::TileMap,
    opts: &ExportOptions,
) -> Result<EngineBundle, String> {
    let scene = scene::build(map, opts);
    let mut findings = preflight::check(&scene, target);

    let mut out = match target {
        EngineTarget::Godot => godot::write(&scene, opts)?,
        EngineTarget::Flame => flame::write(&scene, opts)?,
        EngineTarget::Tiled => tiled::write(&scene, opts)?,
    };

    let mut seen: BTreeSet<String> = BTreeSet::new();
    for file in &out.files {
        file.assert_portable()?;
        if !seen.insert(file.path.clone()) {
            return Err(format!(
                "{} writer produced duplicate path {:?}",
                target.id(),
                file.path
            ));
        }
    }
    if out.files.is_empty() {
        return Err(format!("{} writer produced no files", target.id()));
    }
    if !seen.contains(&out.entry) {
        findings.push(Finding::new(
            "error",
            "entry-missing",
            format!(
                "{} claimed entry {:?} but did not emit it",
                target.display_name(),
                out.entry
            ),
            "",
            "",
        ));
    }

    let cells = scene.cell_count();
    let summary = format!(
        "{} export: {} files, {cells} cells, {} tilesets{}",
        target.display_name(),
        out.files.len(),
        scene.tilesets.len(),
        if findings.is_empty() {
            String::new()
        } else {
            format!(", {} finding(s)", findings.len())
        }
    );

    let mut bundle = EngineBundle {
        target,
        entry: out.entry.clone(),
        files: Vec::new(),
        findings,
        summary,
    };

    if opts.emit_manifest {
        let manifest_path = format!("{}/bixel-export.json", target.folder());
        // The manifest is generated last but declared first so it is the file a
        // human (or agent) sees when they open the folder.
        let mut files = Vec::with_capacity(out.files.len() + 1);
        let placeholder = ExportFile::text(manifest_path.clone(), "{}", "export index");
        files.push(placeholder);
        files.append(&mut out.files);
        bundle.files = files;
        let manifest = bundle.manifest(&scene, opts);
        bundle.files[0] = ExportFile::json(manifest_path, &manifest, "export index");
    } else {
        bundle.files = out.files;
    }

    let _ = &mut out.entry;
    Ok(bundle)
}

/// Standard base64 (RFC 4648, padded). Used by the single-file bundle path,
/// which has to be self-contained to be pasted into a chat or an issue.
pub fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Make a filesystem- and engine-safe stem out of a map name.
pub fn sanitize_stem(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if matches!(ch, ' ' | '-' | '_' | '.') && !out.ends_with('_') {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "map".into()
    } else {
        trimmed
    }
}

/// Identifier legal in Godot, Flame/Dart and Unity (C#) at once: a leading
/// digit is the only thing all three reject, and each dislikes a different set
/// of symbols, so we keep it to `[A-Za-z0-9_]` and force a letter start.
pub fn sanitize_identifier(name: &str, fallback: &str) -> String {
    let stem = sanitize_stem(name);
    let mut out: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if out.chars().next().is_none_or(|c| c.is_ascii_digit()) {
        out.insert_str(0, fallback);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_ids_round_trip_with_aliases() {
        for target in EngineTarget::ALL {
            assert_eq!(EngineTarget::from_id(target.id()), Some(target));
        }
        assert_eq!(EngineTarget::from_id("Flutter"), Some(EngineTarget::Flame));
        assert_eq!(EngineTarget::from_id(" GODOT4 "), Some(EngineTarget::Godot));
        assert_eq!(EngineTarget::from_id("unreal"), None);
    }

    #[test]
    fn paths_outside_the_root_are_rejected() {
        assert!(ExportFile::text("godot/map.tscn", "", "").assert_portable().is_ok());
        assert!(ExportFile::text("../escape.tscn", "", "").assert_portable().is_err());
        assert!(ExportFile::text("/abs.tscn", "", "").assert_portable().is_err());
        assert!(ExportFile::text("godot\\win.tscn", "", "").assert_portable().is_err());
        assert!(ExportFile::text("godot//double.tscn", "", "").assert_portable().is_err());
    }

    #[test]
    fn base64_matches_rfc_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn stems_and_identifiers_are_engine_safe() {
        assert_eq!(sanitize_stem("Level 1 — Kaveri"), "level_1_kaveri");
        assert_eq!(sanitize_stem("!!!"), "map");
        assert_eq!(sanitize_identifier("1st Floor", "layer"), "layer1st_floor");
        assert_eq!(sanitize_identifier("Boss: Fight!", "layer"), "boss_fight");
    }
}
