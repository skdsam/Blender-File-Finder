# Fix Compilation Errors and Warnings

## Goal Description
Fix compilation errors prevents the Tauri application from building. Specifically, rename the library import in `main.rs` to match usages in `Cargo.toml` and correct variable naming conventions in `lib.rs` to suppress standard Rust warnings.

## Proposed Changes

### Backend (Rust)

#### [MODIFY] [main.rs](file:///c:/Users/skdso/blend-tree-viewer-tauri/src-tauri/src/main.rs)
- Change `app_lib::run()` to `blender_file_finder_lib::run()`.

#### [MODIFY] [lib.rs](file:///c:/Users/skdso/blend-tree-viewer-tauri/src-tauri/src/lib.rs)
- Rename `folderPath` argument to `folder_path` in `start_scan` function.  
- Rename `scanId` argument to `scan_id` in `poll_scan` function.
- Update all usages of these variables within their respective functions.

## Verification Plan

### Automated Tests
- Run `npm run tauri build` or `cargo build` in `src-tauri` to verify that the compilation succeeds without errors or warnings.
