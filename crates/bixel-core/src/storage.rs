//! Native host storage. The host supplies its sandbox's Documents root; no
//! process-global current project or platform-specific home-directory policy.
use std::{fs, path::Path, sync::atomic::{AtomicU64, Ordering}};
use serde_json::{json, Value};
use crate::paths::safe_resolve;

static WRITE_ID: AtomicU64 = AtomicU64::new(0);

// Inventory and binary reads accept only literal relative paths, and never
// follow links (including links that happen to point back inside the root).
fn file_path(root: &Path, relative: &str) -> Result<std::path::PathBuf, String> {
    use std::path::Component;
    let relative = relative.trim();
    let path = Path::new(relative);
    if path.is_absolute() || relative.starts_with('\\') || path.components().any(|part| matches!(part, Component::ParentDir)) {
        return Err("Expected a relative path without traversal".into());
    }
    let resolved = safe_resolve(relative, root).map_err(|e| e.to_string())?;
    let mut current = root.to_path_buf();
    for part in path.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err("Symbolic links are not allowed".into()),
            Ok(_) => {},
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(resolved)
}

fn files(root: &Path, relative: &str, recursive: bool) -> Result<Value, String> {
    let directory = file_path(root, relative)?;
    let mut pending = std::collections::BTreeSet::from([directory]);
    let mut files = Vec::new();
    while let Some(directory) = pending.pop_first() {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.to_string()),
        };
        let mut entries = entries.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() { continue; }
            let entry_path = entry.path();
            let relative = entry_path.strip_prefix(root).map_err(|e| e.to_string())?
                .to_str().ok_or("File path is not UTF-8")?;
            let path = file_path(root, relative)?;
            if kind.is_dir() && recursive {
                pending.insert(path);
            } else if kind.is_file() {
                let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
                if !metadata.is_file() { continue; }
                files.push(json!({"path": relative, "name": entry.file_name().to_string_lossy(), "bytes": metadata.len()}));
                if files.len() == 2000 { break; }
            }
        }
        if files.len() == 2000 { break; }
    }
    files.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    Ok(files.into())
}

fn ensure_root(base: &Path) -> Result<std::path::PathBuf, String> {
    if !base.is_absolute() { return Err("Storage root must be absolute".into()); }
    let ancestor = base.ancestors().find(|p| p.exists()).ok_or("Storage has no existing ancestor")?;
    let relative = base.strip_prefix(ancestor).map_err(|e| e.to_string())?;
    let root = safe_resolve(&relative.to_string_lossy(), ancestor).map_err(|e| e.to_string())?;
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    Ok(root)
}

pub fn write(base: &Path, relative: &str, bytes: &[u8]) -> Result<(), String> {
    let root = ensure_root(base)?;
    let base = root.as_path();
    let path = safe_resolve(relative, base).map_err(|e| e.to_string())?;
    let parent = path.parent().ok_or("missing parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = format!("{}.{}-{}.tmp", relative, std::process::id(), WRITE_ID.fetch_add(1, Ordering::Relaxed));
    let tmp = safe_resolve(&temporary, base).map_err(|e| e.to_string())?;
    let result = (|| {
        use std::io::Write;
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, &path)
    })();
    if result.is_err() { let _ = fs::remove_file(tmp); }
    result.map_err(|e: std::io::Error| e.to_string())
}

/// Read a regular file under the storage root. `Ok(None)` means not found.
/// Used by the bulk FFI path so the host never round-trips bytes as JSON.
pub fn read_bytes(base: &Path, relative: &str) -> Result<Option<Vec<u8>>, String> {
    use std::io::Read;
    const MAX_BYTES: u64 = 32_000_000;
    let root = ensure_root(base)?;
    let path = file_path(&root, relative)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => {},
        Ok(_) => return Err("Expected a regular file".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    }
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("Expected a regular file".into());
    }
    if metadata.len() > MAX_BYTES {
        return Err("Choose an asset no larger than 32 MB.".into());
    }
    // Bound the actual read too: a file may grow after metadata is read.
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Choose an asset no larger than 32 MB.".into());
    }
    Ok(Some(bytes))
}

pub fn request(base: &Path, value: &Value) -> Result<Value, String> {
    if !base.is_absolute() { return Err("Storage root must be absolute".into()); }
    let root = ensure_root(base)?;
    let field = |key| value.get(key).and_then(Value::as_str).ok_or_else(|| format!("Missing {key}"));
    match field("op")? {
        "create" => {
            let id = field("id")?;
            let name = field("name")?.trim();
            if id.is_empty() || id.len() > 120 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') || name.is_empty() || name.len() > 240 {
                return Err("Invalid project name or identifier".into());
            }
            let folder = safe_resolve(id, &root).map_err(|e| e.to_string())?;
            fs::create_dir(&folder).map_err(|e| e.to_string())?;
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs_f64();
            let metadata = json!({"id":id, "name":name, "schema":1, "created":stamp});
            write(&root, &format!("{id}/project.json"), metadata.to_string().as_bytes())?;
            let cache = safe_resolve(&format!("{id}/.studio/cache/ai"), &root).map_err(|e| e.to_string())?;
            fs::create_dir_all(cache).map_err(|e| e.to_string())?;
            Ok(metadata)
        }
        "list" => {
            let mut projects = Vec::new();
            for entry in fs::read_dir(&root).map_err(|e| e.to_string())?.flatten() {
                if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
                let id = entry.file_name().to_string_lossy().into_owned();
                let path = safe_resolve(&format!("{id}/project.json"), &root).map_err(|e| e.to_string())?;
                if let Ok(text) = fs::read_to_string(path) {
                    if let Ok(mut data) = serde_json::from_str::<Value>(&text) {
                        if data["name"].is_string() && data["schema"] == 1 {
                            data["id"] = id.into();
                            projects.push(data);
                        }
                    }
                }
            }
            projects.sort_by(|a,b| a["name"].as_str().cmp(&b["name"].as_str()));
            Ok(projects.into())
        }
        "read" => {
            let path = safe_resolve(field("path")?, &root).map_err(|e| e.to_string())?;
            match fs::read_to_string(path) {
                Ok(text) => Ok(text.into()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Null),
                Err(e) => Err(e.to_string()),
            }
        }
        "files" => {
            let recursive = match value.get("recursive") {
                None => true,
                Some(value) => value.as_bool().ok_or("recursive must be a boolean")?,
            };
            files(&root, field("path")?, recursive)
        }
        "read_bytes" => {
            match read_bytes(&root, field("path")?)? {
                Some(bytes) => Ok(json!(bytes)),
                None => Ok(Value::Null),
            }
        }
        "write" => { write(&root, field("path")?, field("text")?.as_bytes())?; Ok(Value::Null) }
        "export_bix" => {
            export_bix(&root, field("id")?, Path::new(field("dest")?))?;
            Ok(Value::Null)
        }
        "import_bix" => {
            let name = value.get("name").and_then(Value::as_str).unwrap_or("Imported Project");
            import_bix(&root, Path::new(field("src")?), field("id")?, name)
        }
        _ => Err("Unknown storage operation".into()),
    }
}

fn is_archive_ignored(rel: &str) -> bool {
    let rel = rel.trim_start_matches("./");
    rel.is_empty()
        || rel.ends_with('/')
        || rel == ".DS_Store"
        || rel.ends_with("/.DS_Store")
        || rel.starts_with("__MACOSX/")
        || rel == ".studio/sync"
        || rel.starts_with(".studio/sync/")
        || rel.ends_with(".tmp")
}

/// Package an entire project directory into a Deflate-compressed `.bix` (ZIP) archive.
pub fn export_bix(root: &Path, project_id: &str, dest_path: &Path) -> Result<(), String> {
    if project_id.is_empty() || project_id.len() > 120 || !project_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("Invalid project identifier".into());
    }
    let project_dir = file_path(root, project_id)?;
    if !project_dir.is_dir() {
        return Err("Project folder does not exist".into());
    }
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    let mut pending = vec![project_dir.clone()];
    while let Some(dir) = pending.pop() {
        let read_dir = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.to_string()),
        };
        for entry in read_dir {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() { continue; }
            let path = entry.path();
            let rel = path.strip_prefix(&project_dir).map_err(|e| e.to_string())?
                .to_str().ok_or("File path is not UTF-8")?.replace('\\', "/");
            if is_archive_ignored(&rel) { continue; }
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                entries.push((rel, fs::read(&path).map_err(|e| e.to_string())?));
            }
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let zip_bytes = zip_pack(&entries)?;
    if let Some(parent) = dest_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    fs::write(dest_path, zip_bytes).map_err(|e| e.to_string())
}

/// Extract a `.bix` (ZIP) project archive into `root/<new_id>` and return its `project.json` metadata.
pub fn import_bix(root: &Path, archive_path: &Path, new_id: &str, fallback_name: &str) -> Result<Value, String> {
    use std::io::Read;
    const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
    if new_id.is_empty() || new_id.len() > 120 || !new_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("Invalid project identifier".into());
    }
    let file = fs::File::open(archive_path).map_err(|e| format!("Could not open .bix file: {e}"))?;
    let mut raw_archive = Vec::new();
    file.take(MAX_ARCHIVE_BYTES + 1).read_to_end(&mut raw_archive).map_err(|e| e.to_string())?;
    if raw_archive.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err("Choose a .bix archive no larger than 512 MB.".into());
    }
    let mut entries = zip_unpack(&raw_archive)?;
    if entries.is_empty() {
        return Err("The .bix archive is empty.".into());
    }
    // Strip a shared top-level wrapper folder when the archive was zipped from a parent directory.
    let has_root_marker = entries.iter().any(|(p, _)| matches!(p.as_str(), "project.json" | "workspace.json" | "document.json"));
    if !has_root_marker {
        let first_seg = entries[0].0.split_once('/').map(|(seg, _)| seg.to_string());
        if let Some(prefix) = first_seg.filter(|s| !matches!(s.as_str(), "documents" | "assets" | ".studio")) {
            let prefix_slash = format!("{prefix}/");
            if entries.iter().all(|(p, _)| p.starts_with(&prefix_slash)) {
                for (p, _) in &mut entries {
                    *p = p[prefix_slash.len()..].to_string();
                }
            }
        }
    }

    let folder = safe_resolve(new_id, root).map_err(|e| e.to_string())?;
    fs::create_dir(&folder).map_err(|e| e.to_string())?;
    let populate = (|| -> Result<Value, String> {
        for (rel, bytes) in &entries {
            if is_archive_ignored(rel) { continue; }
            write(&folder, rel, bytes)?;
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs_f64();
        let existing_meta = fs::read_to_string(folder.join("project.json")).ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok());
        let name = existing_meta.as_ref()
            .and_then(|v| v.get("name")).and_then(Value::as_str)
            .map(str::trim).filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                let fb = fallback_name.trim();
                if fb.is_empty() { "Imported Project" } else { fb }
            });
        let name: String = name.chars().take(240).collect();
        let created = existing_meta.as_ref()
            .and_then(|v| v.get("created")).and_then(Value::as_f64)
            .unwrap_or(stamp);
        let metadata = json!({"id": new_id, "name": name, "schema": 1, "created": created});
        write(&folder, "project.json", metadata.to_string().as_bytes())?;
        let cache = safe_resolve(".studio/cache/ai", &folder).map_err(|e| e.to_string())?;
        fs::create_dir_all(cache).map_err(|e| e.to_string())?;
        Ok(metadata)
    })();
    if populate.is_err() {
        let _ = fs::remove_dir_all(&folder);
    }
    populate
}

pub fn zip_pack(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut out = Vec::new();
    let mut cd = Vec::new();
    for (name, raw) in entries {
        let name_bytes = name.as_bytes();
        if name_bytes.len() > u16::MAX as usize || raw.len() > u32::MAX as usize {
            return Err("File too large for .bix archive".into());
        }
        let crc = crc32fast::hash(raw);
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(raw).map_err(|e| e.to_string())?;
        let comp = enc.finish().map_err(|e| e.to_string())?;
        let local_off = out.len() as u32;

        out.extend_from_slice(&0x04034b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0x0800u16.to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(comp.len() as u32).to_le_bytes());
        out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(&comp);

        cd.extend_from_slice(&0x02014b50u32.to_le_bytes());
        cd.extend_from_slice(&20u16.to_le_bytes());
        cd.extend_from_slice(&20u16.to_le_bytes());
        cd.extend_from_slice(&0x0800u16.to_le_bytes());
        cd.extend_from_slice(&8u16.to_le_bytes());
        cd.extend_from_slice(&0u32.to_le_bytes());
        cd.extend_from_slice(&crc.to_le_bytes());
        cd.extend_from_slice(&(comp.len() as u32).to_le_bytes());
        cd.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        cd.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes());
        cd.extend_from_slice(&0u32.to_le_bytes());
        cd.extend_from_slice(&local_off.to_le_bytes());
        cd.extend_from_slice(name_bytes);
    }
    let cd_off = out.len() as u32;
    let cd_len = cd.len() as u32;
    let count = u16::try_from(entries.len()).map_err(|_| "Too many files in .bix archive")?;
    out.extend_from_slice(&cd);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_len.to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

pub fn zip_unpack(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    use std::io::Read;
    const MAX_TOTAL_OUT: usize = 512 * 1024 * 1024;
    if data.len() < 22 { return Err("Not a valid .bix archive.".into()); }
    let min_off = data.len().saturating_sub(22 + 65535);
    let eocd = (min_off..=data.len() - 22).rev()
        .find(|&i| data[i..i + 4] == 0x06054b50u32.to_le_bytes())
        .ok_or("Not a valid .bix (ZIP) archive.")?;
    let u16_at = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]) as usize;
    let u32_at = |o: usize| u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]) as usize;
    let cd_size = u32_at(eocd + 12);
    let cd_off = u32_at(eocd + 16);
    let cd_end = cd_off.checked_add(cd_size).filter(|&end| end <= eocd).ok_or("Corrupt .bix central directory.")?;

    let mut pos = cd_off;
    let mut out = Vec::new();
    let mut total_uncomp = 0usize;
    while pos < cd_end {
        if pos + 46 > cd_end || data[pos..pos + 4] != 0x02014b50u32.to_le_bytes() {
            return Err("Corrupt .bix directory entry.".into());
        }
        let method = u16_at(pos + 10);
        let crc = u32_at(pos + 16) as u32;
        let comp_size = u32_at(pos + 20);
        let uncomp_size = u32_at(pos + 24);
        let nlen = u16_at(pos + 28);
        let elen = u16_at(pos + 30);
        let clen = u16_at(pos + 32);
        let local_off = u32_at(pos + 42);
        let next_pos = pos + 46 + nlen + elen + clen;
        if next_pos > cd_end { return Err("Truncated .bix directory entry.".into()); }
        let name = std::str::from_utf8(&data[pos + 46..pos + 46 + nlen])
            .map_err(|_| "Archive entry path is not UTF-8")?.replace('\\', "/");
        pos = next_pos;

        if is_archive_ignored(&name) { continue; }
        total_uncomp = total_uncomp.checked_add(uncomp_size).filter(|&t| t <= MAX_TOTAL_OUT)
            .ok_or("Expanded .bix archive exceeds 512 MB.")?;
        if local_off + 30 > data.len() || data[local_off..local_off + 4] != 0x04034b50u32.to_le_bytes() {
            return Err("Corrupt .bix local file header.".into());
        }
        let ln = u16_at(local_off + 26);
        let lm = u16_at(local_off + 28);
        let data_start = local_off.checked_add(30 + ln + lm).ok_or("Corrupt .bix offset.")?;
        let data_end = data_start.checked_add(comp_size).filter(|&e| e <= data.len())
            .ok_or("Truncated .bix file payload.")?;
        let payload = &data[data_start..data_end];
        let raw = match method {
            0 => payload.to_vec(),
            8 => {
                let mut decoded = Vec::with_capacity(uncomp_size);
                flate2::read::DeflateDecoder::new(payload)
                    .take((uncomp_size as u64) + 1)
                    .read_to_end(&mut decoded)
                    .map_err(|e| format!("Failed to decompress {name}: {e}"))?;
                decoded
            }
            _ => return Err(format!("Unsupported compression method ({method}) in .bix archive.")),
        };
        if raw.len() != uncomp_size || crc32fast::hash(&raw) != crc {
            return Err(format!("Checksum mismatch for {name} in .bix archive."));
        }
        out.push((name, raw));
    }
    Ok(out)
}

/// Reserve a new artifact without replacing existing output, even across app
/// restarts or concurrent processes. False means the caller should pick a new name.
pub fn write_new(base: &Path, relative: &str, bytes: &[u8]) -> Result<bool, String> {
    use std::io::Write;
    let root = ensure_root(base)?;
    let path = safe_resolve(relative, &root).map_err(|e| e.to_string())?;
    fs::create_dir_all(path.parent().ok_or("Missing parent")?).map_err(|e| e.to_string())?;
    let mut file = match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(e) => return Err(e.to_string()),
    };
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(path);
        return Err(error.to_string());
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_and_import_bix_round_trips_all_project_files() {
        let root = std::env::temp_dir().join(format!(
            "bixel-bix-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();

        let created = request(&root, &json!({"op": "create", "id": "proj-1", "name": "Hero Quest"})).unwrap();
        assert_eq!(created["name"], "Hero Quest");
        write(&root, "proj-1/workspace.json", br#"{"schema":2,"documents":[{"id":"doc-1","name":"Hero","mode":"normal","width":32,"height":32}],"activeDocumentID":"doc-1","style":"16-Bit"}"#).unwrap();
        write(&root, "proj-1/documents/doc-1.json", br#"{"width":32,"height":32}"#).unwrap();
        write(&root, "proj-1/assets/tileset.png", &[137, 80, 78, 71, 13, 10, 26, 10]).unwrap();
        write(&root, "proj-1/.studio/cache/ai/gen.png", &[1, 2, 3, 4]).unwrap();

        let bix_path = root.join("hero-quest.bix");
        request(&root, &json!({"op": "export_bix", "id": "proj-1", "dest": bix_path.to_str().unwrap()})).unwrap();
        assert!(bix_path.is_file());

        let imported = request(&root, &json!({
            "op": "import_bix",
            "id": "proj-2",
            "src": bix_path.to_str().unwrap(),
            "name": "Fallback"
        })).unwrap();
        assert_eq!(imported["id"], "proj-2");
        assert_eq!(imported["name"], "Hero Quest");
        assert_eq!(read_bytes(&root, "proj-2/documents/doc-1.json").unwrap().unwrap(), br#"{"width":32,"height":32}"#);
        assert_eq!(read_bytes(&root, "proj-2/assets/tileset.png").unwrap().unwrap(), &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert_eq!(read_bytes(&root, "proj-2/.studio/cache/ai/gen.png").unwrap().unwrap(), &[1, 2, 3, 4]);

        let _ = fs::remove_dir_all(&root);
    }
}
