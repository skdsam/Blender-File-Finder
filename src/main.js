// main.js (plain HTML, no bundler)
// Tauri v2 globals
const TAURI = window.__TAURI__;
const invoke = TAURI?.core?.invoke;

const $ = (id) => document.getElementById(id);

let state = {
  tree: null,
  files: [],
  selectedPath: null,
  expanded: new Set(),
  scanId: null,
  polling: null,
  lastFolder: null,
};

// Elements
const folderPill = $("folderPill");
const statusPill = $("statusPill");
const counts = $("counts");
const treeEl = $("tree");
const resultsEl = $("results");
const resultsCount = $("resultsCount");
const infoEl = $("info");
const infoContent = $("infoContent");
const thumbContainer = $("thumbContainer");
const searchEl = $("search");

const btnPick = $("btnPick");
const btnOpen = $("btnOpen");
const btnReveal = $("btnReveal");

const themeDark = $("themeDark");
const themeLight = $("themeLight");

const progressWrap = $("progressWrap");
const progressBar = $("progressBar");
const progressText = $("progressText");
const currentPathEl = $("currentPath");

// ------------------ Utils ------------------
const escapeHtml = (s) =>
  String(s ?? "")
  .replaceAll("&", "&amp;")
  .replaceAll("<", "&lt;")
  .replaceAll(">", "&gt;")
  .replaceAll('"', "&quot;")
  .replaceAll("'", "&#39;");

function bytesToHuman(n) {
  if (typeof n !== "number") return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let u = 0,
    x = n;
  while (x >= 1024 && u < units.length - 1) {
    x /= 1024;
    u++;
  }
  return `${x.toFixed(x < 10 && u > 0 ? 2 : 1)} ${units[u]}`;
}

function setCounts(scanned, found, elapsedMs) {
  if (!counts) return;
  const s = scanned != null ? scanned.toLocaleString() : "—";
  const f = found != null ? found.toLocaleString() : "—";
  const t = elapsedMs != null ? `${(elapsedMs / 1000).toFixed(1)}s` : "—";
  counts.textContent = `Scanned: ${s} • .blend: ${f} • Time: ${t}`;
}

function setActionButtons() {
  const enabled = !!state.selectedPath;
  btnOpen && (btnOpen.disabled = !enabled);
  btnReveal && (btnReveal.disabled = !enabled);
}

function showError(msg) {
  if (!infoEl) return;
  infoEl.innerHTML = `<div class="hint" style="color:var(--danger);">${escapeHtml(msg)}</div>`;
}

// ------------------ Theme ------------------
function applyTheme(theme) {
  const t = theme === "light" ? "light" : "dark";
  document.documentElement.setAttribute("data-theme", t);
  localStorage.setItem("theme", t);

  if (themeDark && themeLight) {
    themeDark.classList.toggle("active", t === "dark");
    themeLight.classList.toggle("active", t === "light");
  }
}

// ------------------ Commands ------------------
async function openSelected() {
  if (!state.selectedPath) return;
  try {
    await invoke("open_file", {
      path: state.selectedPath
    });
  } catch (e) {
    showError(`Open failed: ${e}`);
  }
}

async function revealSelected() {
  if (!state.selectedPath) return;
  try {
    await invoke("reveal_file", {
      path: state.selectedPath
    });
  } catch (e) {
    showError(`Reveal failed: ${e}`);
  }
}

// ------------------ Rendering ------------------
function makeRow({
  icon,
  label,
  meta,
  indent = 0,
  active = false,
  onClick,
  onDblClick,
}) {
  const row = document.createElement("div");
  row.className = "nodeRow" + (active ? " active" : "");
  row.style.paddingLeft = `${8 + indent}px`;

  const ic = document.createElement("div");
  ic.className = "icon";
  ic.innerHTML = icon;

  const la = document.createElement("div");
  la.className = "label";
  la.textContent = label ?? "";

  const me = document.createElement("div");
  me.className = "meta";
  me.textContent = meta ?? "";

  row.appendChild(ic);
  row.appendChild(la);
  row.appendChild(me);

  row.addEventListener("click", (e) => {
    e.stopPropagation();
    onClick?.();
  });
  row.addEventListener("dblclick", (e) => {
    e.stopPropagation();
    onDblClick?.();
  });

  return row;
}

function renderInfo(node) {
  if (!infoContent || !thumbContainer) return;

  if (!node) {
    infoContent.innerHTML = `<div class="hint">Select a .blend file from the tree or search results to view details.</div>`;
    thumbContainer.innerHTML = "";
    thumbContainer.style.display = "none";
    setActionButtons();
    return;
  }

  const b = node.meta?.blender;
  const blenderText = b?.version ?
    `${b.version} (raw ${b.raw ?? "???"}, ${b.pointer_size ?? "?"}-bit, ${b.endianness ?? "?"} endian)` :
    b?.error ?
    `Unknown (${b.error})` :
    "Unknown";

  // Thumbnail rendering
  if (b?.thumbnail) {
    console.log(`Thumbnail found for ${node.name}: ${b.thumb_width}x${b.thumb_height}`);
    thumbContainer.style.display = "flex";
    thumbContainer.innerHTML = "";
    // Decode base64 to bytes
    const binaryString = atob(b.thumbnail);
    const bytes = new Uint8ClampedArray(binaryString.length);
    for (let i = 0; i < binaryString.length; i++) {
      bytes[i] = binaryString.charCodeAt(i);
    }

    const thumbW = b.thumb_width || 128;
    const thumbH = b.thumb_height || 128;

    // 1. Create offscreen canvas for source data
    const offscreen = document.createElement("canvas");
    offscreen.width = thumbW;
    offscreen.height = thumbH;
    const offCtx = offscreen.getContext("2d");
    const imageData = offCtx.createImageData(thumbW, thumbH);
    imageData.data.set(bytes);
    offCtx.putImageData(imageData, 0, 0);

    // 2. Setup main canvas with High DPI support
    const canvas = document.createElement("canvas");
    const dpr = window.devicePixelRatio || 1;

    // Use native resolution for display size
    const displayW = thumbW;
    const displayH = thumbH;

    canvas.width = displayW * dpr;
    canvas.height = displayH * dpr;
    canvas.style.width = `${displayW}px`;
    canvas.style.height = `${displayH}px`;

    const ctx = canvas.getContext("2d");
    ctx.imageSmoothingEnabled = false;
    ctx.webkitImageSmoothingEnabled = false;
    ctx.mozImageSmoothingEnabled = false;
    ctx.msImageSmoothingEnabled = false;

    // Draw offscreen to main
    ctx.drawImage(offscreen, 0, 0, canvas.width, canvas.height);

    thumbContainer.appendChild(canvas);
  } else {
    thumbContainer.style.display = "none";
    thumbContainer.innerHTML = "";
  }

  infoContent.innerHTML = `
    <div class="kv">
      <div class="k">File Name</div>
      <div class="v">${escapeHtml(node.name || "")}</div>
      
      <div class="k">Full Path</div>
      <div class="v">${escapeHtml(node.path || "")}</div>
      
      <div class="k">Directory</div>
      <div class="v">${escapeHtml(node.meta?.folder || "")}</div>
      
      <div class="k">File Size</div>
      <div class="v">
        ${bytesToHuman(node.meta?.size_bytes)}
        <span class="badge">${Number(node.meta?.size_bytes || 0).toLocaleString()} bytes</span>
      </div>
      
      <div class="k">Blender</div>
      <div class="v">${escapeHtml(blenderText)}</div>
      
      <div class="k">Engine</div>
      <div class="v">
        ${b?.render_engine ? `<span class="badge" style="background:var(--accent2);color:#fff;margin-left:0;margin-right:8px;text-transform:uppercase;">${b.render_engine}</span>` : "—"}
      </div>

      <div class="k">Created</div>
      <div class="v">${escapeHtml(node.meta?.created || "—")}</div>
      
      <div class="k">Modified</div>
      <div class="v">${escapeHtml(node.meta?.modified || "—")}</div>
    </div>
  `;
  setActionButtons();
}

function findNodeByPath(node, targetPath) {
  if (!node) return null;
  if (node.node_type === "file" && node.path === targetPath) return node;
  const kids = node.children || [];
  for (const k of kids) {
    const hit = findNodeByPath(k, targetPath);
    if (hit) return hit;
  }
  return null;
}

function selectPath(filePath) {
  state.selectedPath = filePath;
  const node = findNodeByPath(state.tree, filePath);
  renderTree();
  renderResults();
  renderInfo(node);
}

function toggleFolder(path) {
  if (state.expanded.has(path)) state.expanded.delete(path);
  else state.expanded.add(path);
  renderTree();
}

function renderTreeNode(node, indent = 0) {
  const rows = [];

  if (node.node_type === "dir") {
    const isExpanded = state.expanded.has(node.path);
    const caret = isExpanded ? "▾" : "▸";

    rows.push(
      makeRow({
        icon: caret,
        label: node.name,
        meta: "folder",
        indent,
        onClick: () => toggleFolder(node.path),
      })
    );

    if (isExpanded) {
      const kids = node.children || [];
      for (const child of kids) {
        rows.push(...renderTreeNode(child, indent + 12));
      }
    }
    return rows;
  }

  // file
  const ver = node.meta?.blender?.version || "";
  const isActive = state.selectedPath === node.path;

  rows.push(
    makeRow({
      icon: "🧊",
      label: node.name,
      meta: ver ? `v${ver}` : "v?",
      indent,
      active: isActive,
      onClick: () => selectPath(node.path),
      onDblClick: () => {
        selectPath(node.path);
        openSelected();
      },
    })
  );

  return rows;
}

function renderTree() {
  if (!treeEl) return;
  treeEl.innerHTML = "";
  if (!state.tree) {
    treeEl.innerHTML = `<div class="hint">No folder scanned yet</div>`;
    return;
  }

  // Ensure root expanded by default
  if (!state.expanded.has(state.tree.path))
    state.expanded.add(state.tree.path);

  const rows = renderTreeNode(state.tree, 0);
  for (const r of rows) treeEl.appendChild(r);
}

function renderResults() {
  if (!resultsEl || !resultsCount) return;

  const q = (searchEl?.value || "").trim().toLowerCase();
  resultsEl.innerHTML = "";

  if (!state.files.length) {
    resultsCount.textContent = "—";
    resultsEl.innerHTML = `<div class="hint">No results yet. Select a folder to scan.</div>`;
    return;
  }

  let list = state.files;
  if (q) {
    list = state.files.filter(
      (f) =>
      (f.name || "").toLowerCase().includes(q) ||
      (f.path || "").toLowerCase().includes(q)
    );
  }

  resultsCount.textContent = `${list.length.toLocaleString()}`;

  const max = Math.min(list.length, 2000);
  for (let i = 0; i < max; i++) {
    const f = list[i];
    const isActive = state.selectedPath === f.path;

    resultsEl.appendChild(
      makeRow({
        icon: `<img src="assets/blender_icon.png" style="width:18px;height:18px;vertical-align:text-bottom">`,
        label: f.name,
        meta: `${bytesToHuman(f.size_bytes)} • ${
          f.blender_version ? "v" + f.blender_version : "v?"
        }`,
        active: isActive,
        indent: 0,
        onClick: () => selectPath(f.path),
        onDblClick: () => {
          selectPath(f.path);
          openSelected();
        },
      })
    );

    const sub = document.createElement("div");
    sub.className = "subPath";
    sub.textContent = f.path;
    resultsEl.appendChild(sub);
  }

  if (list.length > max) {
    const note = document.createElement("div");
    note.className = "hint";
    note.textContent = `Showing first ${max.toLocaleString()} results. Refine search to see more.`;
    resultsEl.appendChild(note);
  }
}

// ------------------ Scanning + Progress ------------------
function showProgress(on) {
  if (!progressWrap) return;
  progressWrap.style.display = on ? "block" : "none";
}

function setProgressIndeterminate(on) {
  if (!progressBar) return;
  progressBar.classList.toggle("indeterminate", !!on);
}

async function startScan(folder) {
  state.lastFolder = folder;
  localStorage.setItem("lastFolder", folder);

  folderPill && (folderPill.textContent = folder);
  showProgress(true);
  setProgressIndeterminate(true);

  if (currentPathEl) currentPathEl.textContent = "";

  // reset UI state
  state.selectedPath = null;
  state.tree = null;
  state.files = [];
  state.expanded = new Set();
  renderTree();
  renderResults();
  renderInfo(null);
  setActionButtons();

  const startedAt = performance.now();

  try {
    const scanId = await invoke("start_scan", {
      folderPath: folder
    });
    state.scanId = scanId;

    // poll
    if (state.polling) clearInterval(state.polling);
    state.polling = setInterval(async () => {
      try {
        const p = await invoke("poll_scan", {
          scanId: state.scanId
        });

        setCounts(
          p.scanned_entries,
          p.found_blends,
          performance.now() - startedAt
        );
        if (progressText)
          progressText.textContent = `${p.found_blends.toLocaleString()} .blend files found`;

        if (currentPathEl && p.current_path)
          currentPathEl.textContent = p.current_path;

        if (p.status === "done") {
          clearInterval(state.polling);
          state.polling = null;

          setProgressIndeterminate(false);
          showProgress(false);

          // Apply results
          state.tree = p.result.tree;
          state.files = p.result.files;

          // expand root by default
          state.expanded.add(state.tree.path);

          renderTree();
          renderResults();
          renderInfo(null);
          setActionButtons();
        }

        if (p.status === "error") {
          clearInterval(state.polling);
          state.polling = null;

          setProgressIndeterminate(false);
          showProgress(false);
          showError(p.error || "Scan failed");
        }
      } catch (err) {
        clearInterval(state.polling);
        state.polling = null;
        setProgressIndeterminate(false);
        showProgress(false);
        showError(`Polling failed: ${err}`);
      }
    }, 200);
  } catch (e) {
    setProgressIndeterminate(false);
    showProgress(false);
    showError(`Start scan failed: ${e}`);
  }
}

// ------------------ UI wiring ------------------
btnPick?.addEventListener("click", async () => {
  try {
    const folder = await invoke("pick_folder");
    if (typeof folder === "string" && folder.length) startScan(folder);
  } catch (e) {
    showError(`Folder picker failed: ${e}`);
  }
});

btnOpen?.addEventListener("click", openSelected);
btnReveal?.addEventListener("click", revealSelected);

searchEl?.addEventListener("input", renderResults);

themeDark?.addEventListener("click", () => applyTheme("dark"));
themeLight?.addEventListener("click", () => applyTheme("light"));

// ------------------ Boot ------------------
applyTheme(localStorage.getItem("theme") || "dark");
renderTree();
renderResults();
renderInfo(null);
setActionButtons();
setCounts(null, null, null);

// Remember last folder on startup
const last = localStorage.getItem("lastFolder");
if (last && typeof last === "string" && last.length) {
  folderPill && (folderPill.textContent = last);
  // Auto-scan shortly after load (lets UI paint first)
  setTimeout(() => startScan(last), 150);
}