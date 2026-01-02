# Fix Errors and Add Icon

## Changes

### 1. Fixed Backend Compilation Errors
- **`src-tauri/src/main.rs`**: Renamed the crate import from `app_lib` to `blender_file_finder_lib`.
- **`src-tauri/src/lib.rs`**: Renamed `folderPath` to `folder_path` and `scanId` to `scan_id`.

### 2. Frontend Updates
- **`src/main.js`**: 
    - Kept camelCase arguments (`folderPath`, `scanId`) to match Tauri's binding.
    - Updated `makeRow` to render icons as HTML.
    - Replaced the "🧊" emoji with the downloaded Blender icon (`assets/blender_icon.png`).

### 3. Added Assets
- **`src/assets/blender_icon.png`**: Downloaded the Blender logo for use as the file icon.

## Verification Results
- **Automated Tests**: `cargo check` passed.
- **Manual Verification**: The app should now run (`npm run tauri dev`) and display the Blender logo next to .blend files in the tree and search results.
