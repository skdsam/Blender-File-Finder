#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::{DateTime, Local};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, OnceLock,
};
use std::thread;
use walkdir::WalkDir;

// -----------------------------
// Models returned to frontend
// -----------------------------
#[derive(Serialize, Clone)]
pub struct BlendInfo {
    pub version: Option<String>,
    pub raw: Option<String>,
    pub pointer_size: Option<u8>,
    pub endianness: Option<String>,
    pub thumbnail: Option<String>, // Base64 RGBA
    pub thumb_width: Option<i32>,
    pub thumb_height: Option<i32>,
    pub render_engine: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct FileMeta {
    pub size_bytes: u64,
    pub created: Option<String>,
    pub modified: Option<String>,
    pub folder: String,
    pub blender: BlendInfo,
}

#[derive(Serialize, Clone)]
pub struct TreeNode {
    pub node_type: String, // "dir" | "file"
    pub name: String,
    pub path: String,
    pub meta: Option<FileMeta>,
    pub children: Option<Vec<TreeNode>>,
}

#[derive(Serialize, Clone)]
pub struct FlatFile {
    pub name: String,
    pub path: String,
    pub folder: String,
    pub size_bytes: u64,
    pub created: Option<String>,
    pub modified: Option<String>,
    pub blender_version: Option<String>,
    pub thumbnail: Option<String>,
    pub render_engine: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct ScanResult {
    pub tree: TreeNode,
    pub files: Vec<FlatFile>,
}

#[derive(Serialize, Clone)]
pub struct ScanPoll {
    pub scan_id: u64,
    pub status: String, // "scanning" | "done" | "error"
    pub scanned_entries: u64,
    pub found_blends: u64,
    pub current_path: Option<String>,
    pub error: Option<String>,
    pub result: Option<ScanResult>, // only when done
}

// -----------------------------
// Internal scan state
// -----------------------------
struct ScanState {
    scanned_entries: AtomicU64,
    found_blends: AtomicU64,
    done: AtomicBool,
    status: Mutex<String>,
    current_path: Mutex<Option<String>>,
    error: Mutex<Option<String>>,
    result: Mutex<Option<ScanResult>>,
}

impl ScanState {
    fn new() -> Self {
        Self {
            scanned_entries: AtomicU64::new(0),
            found_blends: AtomicU64::new(0),
            done: AtomicBool::new(false),
            status: Mutex::new("scanning".to_string()),
            current_path: Mutex::new(None),
            error: Mutex::new(None),
            result: Mutex::new(None),
        }
    }
}

static NEXT_SCAN_ID: AtomicU64 = AtomicU64::new(1);
static SCANS: OnceLock<Mutex<HashMap<u64, Arc<ScanState>>>> = OnceLock::new();

fn scans_map() -> &'static Mutex<HashMap<u64, Arc<ScanState>>> {
    SCANS.get_or_init(|| Mutex::new(HashMap::new()))
}

// -----------------------------
// .blend header parsing
// -----------------------------
fn parse_blend_header(path: &Path) -> BlendInfo {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            return BlendInfo {
                version: None,
                raw: None,
                pointer_size: None,
                endianness: None,
                thumbnail: None,
                thumb_width: None,
                thumb_height: None,
                render_engine: None,
                error: Some(e.to_string()),
            }
        }
    };

    let mut buf = [0u8; 12];
    if file.read_exact(&mut buf).is_err() {
        return BlendInfo {
            version: None,
            raw: None,
            pointer_size: None,
            endianness: None,
            thumbnail: None,
            thumb_width: None,
            thumb_height: None,
            render_engine: None,
            error: Some("Unable to read header".into()),
        };
    }

    if &buf[0..7] != b"BLENDER" {
        return BlendInfo {
            version: None,
            raw: None,
            pointer_size: None,
            endianness: None,
            thumbnail: None,
            thumb_width: None,
            thumb_height: None,
            render_engine: None,
            error: Some("Not a blend file".into()),
        };
    }

    let pointer_size = match buf[7] {
        b'-' => Some(64),
        b'_' => Some(32),
        _ => None,
    };

    let endianness = match buf[8] {
        b'v' => Some("little".into()),
        b'V' => Some("big".into()),
        _ => Some("unknown".into()),
    };

    let raw = String::from_utf8_lossy(&buf[9..12]).to_string();
    let chars: Vec<char> = raw.chars().collect();
    let version = if chars.len() == 3 {
        Some(format!("{}.{}.{}", chars[0], chars[1], chars[2]))
    } else {
        None
    };

    let mut info = BlendInfo {
        version,
        raw: Some(raw),
        pointer_size,
        endianness,
        thumbnail: None,
        thumb_width: None,
        thumb_height: None,
        render_engine: None,
        error: None,
    };

    // Advanced parsing for thumbnail and metadata
    if let Err(e) = parse_blocks(&mut info, &mut file, pointer_size) {
        // Non-fatal error for advanced parsing
        info.error = Some(format!("Header OK, but block scan failed: {}", e));
    }

    info
}

fn parse_blocks(
    info: &mut BlendInfo,
    file: &mut File,
    ptr_size: Option<u8>,
) -> Result<(), Box<dyn std::error::Error>> {
    use base64::prelude::*;
    use std::io::{Read, Seek, SeekFrom};

    let is_little = info.endianness.as_deref() != Some("big");
    let ptr_size = ptr_size.unwrap_or(64) / 8;

    // Header is 12 bytes
    file.seek(SeekFrom::Start(12))?;

    let header_len = 4 + 4 + ptr_size as usize + 4 + 4;
    let mut header_buf = vec![0u8; header_len];

    let mut searched_blocks = 0;
    loop {
        if file.read_exact(&mut header_buf).is_err() {
            break;
        }
        searched_blocks += 1;

        let id = String::from_utf8_lossy(&header_buf[0..4]);
        let size = if is_little {
            u32::from_le_bytes(header_buf[4..8].try_into()?)
        } else {
            u32::from_be_bytes(header_buf[4..8].try_into()?)
        } as u64;

        if id.starts_with("TEST") {
            let mut thumb_header = [0u8; 8];
            if file.read_exact(&mut thumb_header).is_ok() {
                let (width, height) = if is_little {
                    (
                        i32::from_le_bytes(thumb_header[0..4].try_into()?),
                        i32::from_le_bytes(thumb_header[4..8].try_into()?),
                    )
                } else {
                    (
                        i32::from_be_bytes(thumb_header[0..4].try_into()?),
                        i32::from_be_bytes(thumb_header[4..8].try_into()?),
                    )
                };

                let data_size = (width * height * 4) as usize;
                if data_size > 0 && data_size < 1024 * 1024 * 10 {
                    let mut rgba = vec![0u8; data_size];
                    if file.read_exact(&mut rgba).is_ok() {
                        info.thumbnail = Some(BASE64_STANDARD.encode(&rgba));
                        info.thumb_width = Some(width);
                        info.thumb_height = Some(height);
                    }
                }

                // Ensure we skip the rest of the block if size was different
                let read_so_far = 8 + data_size as u64;
                if size > read_so_far {
                    file.seek(SeekFrom::Current((size - read_so_far) as i64))?;
                }
            }
        } else if id.starts_with("SC") {
            let mut sc_data = vec![0u8; size as usize];
            if file.read_exact(&mut sc_data).is_ok() {
                let sc_str = String::from_utf8_lossy(&sc_data).to_uppercase();
                if sc_str.contains("CYCLES") {
                    info.render_engine = Some("Cycles".into());
                } else if sc_str.contains("EEVEE") {
                    info.render_engine = Some("Eevee".into());
                } else if sc_str.contains("WORKBENCH") {
                    info.render_engine = Some("Workbench".into());
                }
            }
        } else if id.starts_with("DNA1") || id.starts_with("ENDB") || searched_blocks > 3000 {
            break;
        } else {
            file.seek(SeekFrom::Current(size as i64))?;
        }
    }

    Ok(())
}

// -----------------------------
// Tree builder (recursive)
// -----------------------------
#[derive(Default)]
struct DirNode {
    dirs: BTreeMap<String, DirNode>,
    files: Vec<(String, PathBuf, FileMeta)>, // (name, full_path, meta)
}

fn insert_file(
    root: &mut DirNode,
    rel_parts: &[String],
    file_name: &str,
    full_path: &Path,
    meta: FileMeta,
) {
    let mut cur = root;
    for part in rel_parts {
        cur = cur
            .dirs
            .entry(part.clone())
            .or_insert_with(DirNode::default);
    }
    cur.files
        .push((file_name.to_string(), full_path.to_path_buf(), meta));
}

fn build_tree_nodes(dir: &DirNode, name: &str, path: &Path) -> TreeNode {
    let mut children: Vec<TreeNode> = Vec::new();

    // Directories first
    for (dname, dnode) in dir.dirs.iter() {
        let child_path = path.join(dname);
        children.push(build_tree_nodes(dnode, dname, &child_path));
    }

    // Files
    for (fname, fpath, meta) in dir.files.iter() {
        children.push(TreeNode {
            node_type: "file".into(),
            name: fname.clone(),
            path: fpath.to_string_lossy().to_string(),
            meta: Some(meta.clone()),
            children: None,
        });
    }

    TreeNode {
        node_type: "dir".into(),
        name: name.to_string(),
        path: path.to_string_lossy().to_string(),
        meta: None,
        children: Some(children),
    }
}

// -----------------------------
// Commands
// -----------------------------
#[tauri::command]
fn pick_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let res = app.dialog().file().blocking_pick_folder();

    if let Some(fp) = res {
        let path = fp.into_path().map_err(|e| e.to_string())?;
        Ok(Some(path.to_string_lossy().to_string()))
    } else {
        Ok(None)
    }
}

#[tauri::command]
fn start_scan(folder_path: String) -> Result<u64, String> {
    let root = PathBuf::from(&folder_path);
    if !root.exists() {
        return Err("Folder does not exist".into());
    }

    let scan_id = NEXT_SCAN_ID.fetch_add(1, Ordering::Relaxed);
    let state = Arc::new(ScanState::new());

    // Store scan state
    {
        let mut map = scans_map().lock().unwrap();
        map.insert(scan_id, state.clone());
    }

    // Background scan thread
    thread::spawn(move || {
        let mut status = state.status.lock().unwrap();
        *status = "scanning".to_string();
        drop(status);

        let mut files: Vec<FlatFile> = Vec::new();
        let mut builder = DirNode::default();

        // Root node name (folder name)
        let root_name = root
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| folder_path.clone());

        for entry in WalkDir::new(&root).into_iter() {
            match entry {
                Ok(e) => {
                    state.scanned_entries.fetch_add(1, Ordering::Relaxed);

                    // Current path (for UI)
                    if let Ok(mut cp) = state.current_path.lock() {
                        *cp = Some(e.path().to_string_lossy().to_string());
                    }

                    let p = e.path();
                    if !p.is_file() {
                        continue;
                    }

                    if p.extension()
                        .and_then(|x| x.to_str())
                        .unwrap_or("")
                        .to_lowercase()
                        != "blend"
                    {
                        continue;
                    }

                    state.found_blends.fetch_add(1, Ordering::Relaxed);

                    let meta_fs = match p.metadata() {
                        Ok(m) => m,
                        Err(_) => continue,
                    };

                    let created = meta_fs
                        .created()
                        .ok()
                        .map(|t| DateTime::<Local>::from(t).to_rfc3339());
                    let modified = meta_fs
                        .modified()
                        .ok()
                        .map(|t| DateTime::<Local>::from(t).to_rfc3339());

                    let blend = parse_blend_header(p);
                    let folder = p.parent().unwrap_or(&root).to_string_lossy().to_string();
                    let path_str = p.to_string_lossy().to_string();
                    let name = p
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    let file_meta = FileMeta {
                        size_bytes: meta_fs.len(),
                        created: created.clone(),
                        modified: modified.clone(),
                        folder: folder.clone(),
                        blender: blend.clone(),
                    };

                    // Flat list for search
                    files.push(FlatFile {
                        name: name.clone(),
                        path: path_str.clone(),
                        folder,
                        size_bytes: meta_fs.len(),
                        created,
                        modified,
                        blender_version: blend.version.clone(),
                        thumbnail: blend.thumbnail.clone(),
                        render_engine: blend.render_engine.clone(),
                    });

                    // Tree insert (relative directories)
                    let rel = p.strip_prefix(&root).unwrap_or(p);
                    let mut parts: Vec<String> = Vec::new();
                    if let Some(parent) = rel.parent() {
                        for comp in parent.components() {
                            parts.push(comp.as_os_str().to_string_lossy().to_string());
                        }
                    }
                    insert_file(&mut builder, &parts, &name, p, file_meta);
                }
                Err(err) => {
                    // Non-fatal: keep scanning
                    if let Ok(mut cp) = state.current_path.lock() {
                        *cp = Some(format!("(walk error) {}", err));
                    }
                }
            }
        }

        // Build final tree
        let tree = build_tree_nodes(&builder, &root_name, &root);
        let result = ScanResult { tree, files };

        if let Ok(mut r) = state.result.lock() {
            *r = Some(result);
        }
        if let Ok(mut st) = state.status.lock() {
            *st = "done".to_string();
        }
        state.done.store(true, Ordering::Relaxed);
    });

    Ok(scan_id)
}

#[tauri::command]
fn poll_scan(scan_id: u64) -> Result<ScanPoll, String> {
    let state = {
        let map = scans_map().lock().unwrap();
        map.get(&scan_id).cloned()
    };

    let Some(state) = state else {
        return Err("Scan id not found".into());
    };

    let status = state.status.lock().unwrap().clone();
    let current_path = state.current_path.lock().unwrap().clone();
    let error = state.error.lock().unwrap().clone();

    let result = if status == "done" {
        state.result.lock().unwrap().clone()
    } else {
        None
    };

    Ok(ScanPoll {
        scan_id,
        status,
        scanned_entries: state.scanned_entries.load(Ordering::Relaxed),
        found_blends: state.found_blends.load(Ordering::Relaxed),
        current_path,
        error,
        result,
    })
}

#[tauri::command]
fn open_file(app: tauri::AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn reveal_file(app: tauri::AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    // Cross-platform: open the parent folder
    let p = PathBuf::from(&path);
    let folder = p.parent().map(|x| x.to_path_buf()).unwrap_or(p);

    app.opener()
        .open_path(folder.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

// -----------------------------
// Entry point
// -----------------------------
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            pick_folder,
            start_scan,
            poll_scan,
            open_file,
            reveal_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
