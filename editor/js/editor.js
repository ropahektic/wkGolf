import { exportWaPngAsync, downloadBlob } from './wa-export.js';
import { buildGolfToml, cupGeomFromHoleTile, downloadText } from './golf-toml.js';
import { applyGrassFill } from './grass-fill.js';

const GRID_COLS = 7;
const GRID_ROWS = 4;
const EMPTY_TILE = 0;
const FAIRWAY_TILE_COUNT = GRID_COLS * GRID_ROWS;
const GRASS_BODY_RGB = { r: 42, g: 119, b: 70 };

/** wkGolf / prepare_golf_map.py tee marker color */
const TEE_RGB = [235, 210, 55];
const TEE_PAD_W = 12;
const TEE_PAD_H = 48;

/** @typedef {{ w: number, h: number, snapW: number, leftPad: number, label: string, narrow: boolean, overlay: boolean }} TileMeta */
/** @typedef {{ id: number, col: number, y: number, tileIndex: number, x?: number }} Placement */
/** @typedef {{ id: number, x: number, y: number }} Tee */

const state = {
  tileW: 64,
  tileH: 64,
  snapW: 64,
  tileLeftPad: 0,
  tiles: /** @type {HTMLCanvasElement[]} */ ([]),
  tileMeta: /** @type {TileMeta[]} */ ([]),
  mapCols: 64,
  mapHeight: 512,
  placements: /** @type {Placement[]} */ ([]),
  tees: /** @type {Tee[]} */ ([]),
  mode: /** @type {'tiles' | 'tee'} */ ('tiles'),
  selectedTile: 1,
  selectedPlacement: null,
  selectedTee: null,
  dragging: null,
  nextId: 1,
  nextTeeId: 1,
  autoGrassFill: false,
  grassFill: { ...GRASS_BODY_RGB },
};

const els = {
  paletteFairway: document.getElementById('palette-fairway'),
  paletteExtra: document.getElementById('palette-extra'),
  paletteExtraLabel: document.getElementById('palette-extra-label'),
  paletteWrap: document.getElementById('palette-wrap'),
  mapCanvas: /** @type {HTMLCanvasElement} */ (document.getElementById('map-canvas')),
  guides: /** @type {HTMLCanvasElement} */ (document.getElementById('guides')),
  status: document.getElementById('status'),
  inpCols: /** @type {HTMLInputElement} */ (document.getElementById('inp-cols')),
  inpHeight: /** @type {HTMLInputElement} */ (document.getElementById('inp-height')),
  tilesetInput: /** @type {HTMLInputElement} */ (document.getElementById('tileset-file')),
  btnModeTiles: document.getElementById('btn-mode-tiles'),
  btnModeTee: document.getElementById('btn-mode-tee'),
  inpPar: /** @type {HTMLInputElement} */ (document.getElementById('inp-par')),
};

const mapCtx = els.mapCanvas.getContext('2d');
const guideCtx = els.guides.getContext('2d');

const TILE_Y_MIN = -4096;
const TILE_Y_MAX = 4096;

function setStatus(msg) {
  els.status.textContent = msg;
}

function tileYClamp(y) {
  return Math.max(TILE_Y_MIN, Math.min(TILE_Y_MAX, Math.round(y)));
}

function colToX(col) {
  return col * state.snapW;
}

function maxUsedCol() {
  let m = 0;
  for (const p of state.placements) {
    const meta = metaForIndex(p.tileIndex);
    if (meta.overlay) continue;
    m = Math.max(m, p.col);
  }
  return m;
}

/** Canvas columns: committed mapCols or room to keep building rightward. */
function paintCols() {
  return Math.max(state.mapCols, maxUsedCol() + 10);
}

function mapWidthPx() {
  const cols = paintCols();
  if (cols <= 0) return state.tileW;
  return colToX(cols - 1) + state.tileW;
}

function growMapCols(minCol) {
  const need = minCol + 10;
  if (need <= state.mapCols) return;
  state.mapCols = need;
  els.inpCols.value = String(state.mapCols);
  resizeMapCanvas();
}

function metaForIndex(idx) {
  return (
    state.tileMeta[idx] || {
      w: state.tileW,
      h: state.tileH,
      snapW: state.snapW,
      leftPad: 0,
      label: `Tile ${idx}`,
      narrow: false,
      overlay: false,
    }
  );
}

function metaForCanvas(canvas, label, opts = {}) {
  const narrow = opts.narrow === true;
  const overlay = opts.overlay === true;
  const w = canvas.width;
  const h = canvas.height;
  const data = canvas.getContext('2d').getImageData(0, 0, w, h).data;
  let leftPad = w;
  for (let x = 0; x < w; x++) {
    for (let y = 0; y < h; y++) {
      if (data[(y * w + x) * 4 + 3] > 8) {
        leftPad = Math.min(leftPad, x);
        break;
      }
    }
  }
  if (leftPad >= w) leftPad = 0;
  const snapW = narrow ? w : Math.max(1, w - leftPad);
  return { w, h, snapW, leftPad, label, narrow, overlay };
}

function analyzeFairwaySnap() {
  let leftPad = state.tileW;
  for (let i = 1; i < FAIRWAY_TILE_COUNT && i < state.tiles.length; i++) {
    const m = state.tileMeta[i];
    if (m) leftPad = Math.min(leftPad, m.leftPad);
  }
  if (leftPad >= state.tileW) leftPad = 0;
  state.tileLeftPad = leftPad;
  state.snapW = Math.max(1, state.tileW - leftPad);
}

function placementDrawX(p) {
  const meta = metaForIndex(p.tileIndex);
  if ((meta.overlay || meta.narrow) && p.x != null) return p.x;
  return colToX(p.col);
}

function isOverlayPlacement(p) {
  return metaForIndex(p.tileIndex).overlay;
}

function snapNarrowX(x, meta) {
  return Math.max(0, Math.round(x / meta.snapW) * meta.snapW);
}

function loadTilesetFromImage(img) {
  state.tileW = Math.floor(img.width / GRID_COLS);
  state.tileH = Math.floor(img.height / GRID_ROWS);
  state.tiles = [];
  state.tileMeta = [];
  for (let row = 0; row < GRID_ROWS; row++) {
    for (let col = 0; col < GRID_COLS; col++) {
      const idx = row * GRID_COLS + col;
      const c = document.createElement('canvas');
      c.width = state.tileW;
      c.height = state.tileH;
      const ctx = c.getContext('2d');
      ctx.drawImage(
        img,
        col * state.tileW,
        row * state.tileH,
        state.tileW,
        state.tileH,
        0,
        0,
        state.tileW,
        state.tileH,
      );
      state.tiles[idx] = c;
      state.tileMeta[idx] = metaForCanvas(c, idx === EMPTY_TILE ? 'Empty' : `Fairway ${idx}`);
    }
  }
  analyzeFairwaySnap();
  loadExtraTiles()
    .then(() => {
      buildPalette();
      resizeMapCanvas();
      render();
      setStatus(
        `Tileset ${img.width}×${img.height} — fairway ${state.tileW}×${state.tileH} snap ${state.snapW}px`,
      );
    })
    .catch(() => {
      buildPalette();
      resizeMapCanvas();
      render();
    });
}

async function appendExtraTile(url, label, opts = {}) {
  const img = new Image();
  img.src = url;
  await img.decode();
  const c = document.createElement('canvas');
  c.width = img.width;
  c.height = img.height;
  c.getContext('2d').drawImage(img, 0, 0);
  const idx = state.tiles.length;
  state.tiles.push(c);
  state.tileMeta.push(metaForCanvas(c, label, opts));
  return idx;
}

async function loadExtraTiles() {
  try {
    await appendExtraTile('assets/wall-girder.png', 'Girder wall', { narrow: true });
  } catch {
    /* optional */
  }
  try {
    await appendExtraTile('assets/hole-cup.png', 'Hole cup', { overlay: true });
  } catch {
    /* optional */
  }
}

function paletteBtnClass(i, meta) {
  if (i < FAIRWAY_TILE_COUNT) {
    return 'tile-btn fairway' + (i === EMPTY_TILE ? ' empty' : '');
  }
  if (meta.overlay) return 'tile-btn extra-overlay';
  if (meta.narrow) return 'tile-btn extra-narrow';
  return 'tile-btn fairway';
}

function mountPaletteBtn(parent, i) {
  const meta = metaForIndex(i);
  const btn = document.createElement('button');
  btn.type = 'button';
  btn.className = paletteBtnClass(i, meta);
  btn.title = meta.label;
  btn.dataset.tileIndex = String(i);
  if (state.tiles[i]) {
    btn.style.backgroundImage = `url(${state.tiles[i].toDataURL()})`;
  }
  btn.addEventListener('click', () => {
    if (i === EMPTY_TILE) return;
    setMode('tiles');
    state.selectedTile = i;
    document.querySelectorAll('.tile-btn').forEach((b) => b.classList.remove('selected'));
    btn.classList.add('selected');
  });
  if (i === state.selectedTile) btn.classList.add('selected');
  parent.appendChild(btn);
}

function buildPalette() {
  els.paletteFairway.innerHTML = '';
  els.paletteExtra.innerHTML = '';
  els.paletteFairway.style.setProperty('--tile-aspect', `${state.tileW} / ${state.tileH}`);
  const extraCount = Math.max(0, state.tiles.length - FAIRWAY_TILE_COUNT);
  els.paletteExtraLabel.style.display = extraCount > 0 ? 'block' : 'none';

  for (let i = 0; i < FAIRWAY_TILE_COUNT && i < state.tiles.length; i++) {
    mountPaletteBtn(els.paletteFairway, i);
  }
  for (let i = FAIRWAY_TILE_COUNT; i < state.tiles.length; i++) {
    mountPaletteBtn(els.paletteExtra, i);
  }
}

function setMode(mode) {
  state.mode = mode;
  state.selectedPlacement = null;
  state.selectedTee = null;
  els.btnModeTiles.classList.toggle('active', mode === 'tiles');
  els.btnModeTee.classList.toggle('active', mode === 'tee');
  els.paletteWrap.style.opacity = mode === 'tiles' ? '1' : '0.35';
  els.paletteWrap.style.pointerEvents = mode === 'tiles' ? 'auto' : 'none';
  render();
}

function resizeMapCanvas() {
  const w = mapWidthPx();
  const h = state.mapHeight;
  els.mapCanvas.width = w;
  els.mapCanvas.height = h;
  els.guides.width = w;
  els.guides.height = h;
  els.inpCols.value = String(state.mapCols);
  els.inpHeight.value = String(state.mapHeight);
}

function drawGuides() {
  const w = mapWidthPx();
  const h = state.mapHeight;
  guideCtx.clearRect(0, 0, w, h);
  guideCtx.strokeStyle = 'rgba(100,180,255,0.25)';
  guideCtx.lineWidth = 1;
  for (let c = 0; c <= paintCols(); c++) {
    const x = colToX(c) + 0.5;
    guideCtx.beginPath();
    guideCtx.moveTo(x, 0);
    guideCtx.lineTo(x, h);
    guideCtx.stroke();
  }
}

function drawTeeMarker(ctx, x, y, selected = false) {
  const hw = TEE_PAD_W / 2;
  const top = y - TEE_PAD_H / 2;
  ctx.fillStyle = `rgb(${TEE_RGB.join(',')})`;
  ctx.fillRect(Math.round(x - hw), Math.round(top), TEE_PAD_W, TEE_PAD_H);
  if (selected) {
    ctx.strokeStyle = '#fff';
    ctx.lineWidth = 2;
    ctx.strokeRect(Math.round(x - hw) + 1, Math.round(top) + 1, TEE_PAD_W - 2, TEE_PAD_H - 2);
  }
  ctx.fillStyle = '#000';
  ctx.font = '10px sans-serif';
  ctx.fillText('T', Math.round(x - 3), Math.round(top + 14));
}

function render() {
  const w = mapWidthPx();
  const h = state.mapHeight;
  mapCtx.fillStyle = '#000000';
  mapCtx.fillRect(0, 0, w, h);
  const sorted = [...state.placements].sort((a, b) => {
    const ao = isOverlayPlacement(a) ? 1 : 0;
    const bo = isOverlayPlacement(b) ? 1 : 0;
    if (ao !== bo) return ao - bo;
    return a.y - b.y;
  });
  for (const p of sorted) {
    if (isOverlayPlacement(p)) continue;
    const tile = state.tiles[p.tileIndex];
    const meta = metaForIndex(p.tileIndex);
    if (!tile) continue;
    mapCtx.drawImage(tile, placementDrawX(p), p.y);
    if (state.mode === 'tiles' && p.id === state.selectedPlacement) {
      mapCtx.strokeStyle = '#6cf';
      mapCtx.lineWidth = 2;
      mapCtx.strokeRect(placementDrawX(p) + 1, p.y + 1, meta.w - 2, meta.h - 2);
    }
  }
  if (state.autoGrassFill) {
    applyGrassFill(mapCtx, w, h, state.grassFill);
  }
  for (const p of sorted) {
    if (!isOverlayPlacement(p)) continue;
    const tile = state.tiles[p.tileIndex];
    const meta = metaForIndex(p.tileIndex);
    if (!tile) continue;
    mapCtx.drawImage(tile, placementDrawX(p), p.y);
    if (state.mode === 'tiles' && p.id === state.selectedPlacement) {
      mapCtx.strokeStyle = '#6cf';
      mapCtx.lineWidth = 2;
      mapCtx.strokeRect(placementDrawX(p) + 1, p.y + 1, meta.w - 2, meta.h - 2);
    }
  }
  for (const t of state.tees) {
    drawTeeMarker(mapCtx, t.x, t.y, t.id === state.selectedTee);
  }
  drawGuides();
}

function pointerOnMap(e) {
  const rect = els.mapCanvas.getBoundingClientRect();
  const scaleX = els.mapCanvas.width / rect.width;
  const scaleY = els.mapCanvas.height / rect.height;
  return {
    x: (e.clientX - rect.left) * scaleX,
    y: (e.clientY - rect.top) * scaleY,
  };
}

function colFromX(x) {
  const col = Math.max(0, Math.floor(x / state.snapW));
  growMapCols(col);
  return col;
}

/** Fixed row for every new tile — drag afterward to move vertically. Never follows drags/neighbors. */
function defaultPlacementY(tileIndex) {
  const meta = metaForIndex(tileIndex);
  return Math.max(0, state.mapHeight - meta.h);
}

function placementAtPoint(x, y) {
  for (let i = state.placements.length - 1; i >= 0; i--) {
    const p = state.placements[i];
    const meta = metaForIndex(p.tileIndex);
    const px = placementDrawX(p);
    if (x >= px && x < px + meta.w && y >= p.y && y < p.y + meta.h) return p;
  }
  return null;
}

function teeAt(x, y) {
  const hw = TEE_PAD_W / 2;
  const top = (t) => t.y - TEE_PAD_H / 2;
  return state.tees.find(
    (t) => x >= t.x - hw && x <= t.x + hw && y >= top(t) && y <= top(t) + TEE_PAD_H,
  );
}

function removePlacement(id) {
  state.placements = state.placements.filter((p) => p.id !== id);
  if (state.selectedPlacement === id) state.selectedPlacement = null;
}

function removeTee(id) {
  state.tees = state.tees.filter((t) => t.id !== id);
  if (state.selectedTee === id) state.selectedTee = null;
}

function onDragMove(e) {
  if (!state.dragging) return;
  const { x, y } = pointerOnMap(e);
  if (state.dragging.kind === 'tee') {
    const t = state.tees.find((v) => v.id === state.dragging.id);
    if (!t) return;
    t.x = Math.max(0, Math.min(mapWidthPx(), Math.round(x - state.dragging.offsetX)));
    t.y = Math.max(0, Math.min(state.mapHeight, Math.round(y - state.dragging.offsetY)));
    render();
    return;
  }
  const p = state.placements.find((v) => v.id === state.dragging.id);
  if (!p) return;
  const meta = metaForIndex(p.tileIndex);
  p.y = tileYClamp(y - state.dragging.offsetY);
  if (meta.overlay || meta.narrow) {
    p.x = meta.overlay
      ? Math.round(x - state.dragging.offsetX)
      : snapNarrowX(x - state.dragging.offsetX, meta);
  }
  render();
}

function endDrag() {
  state.dragging = null;
  window.removeEventListener('mousemove', onDragMove);
}

function startDrag(dragState) {
  state.dragging = dragState;
  window.addEventListener('mousemove', onDragMove);
}

window.addEventListener('mouseup', endDrag);

window.addEventListener('keydown', (e) => {
  if (state.mode !== 'tiles' || !state.selectedPlacement) return;
  const p = state.placements.find((x) => x.id === state.selectedPlacement);
  if (!p) return;
  const meta = metaForIndex(p.tileIndex);
  const step = e.shiftKey ? 8 : 1;
  if (e.key === 'ArrowUp') {
    p.y = tileYClamp(p.y - step);
    render();
    e.preventDefault();
  } else if (e.key === 'ArrowDown') {
    p.y = tileYClamp(p.y + step);
    render();
    e.preventDefault();
  } else if ((meta.overlay || meta.narrow) && e.key === 'ArrowLeft') {
    p.x = meta.overlay
      ? (p.x ?? 0) - step
      : snapNarrowX((p.x ?? 0) - step, meta);
    render();
    e.preventDefault();
  } else if ((meta.overlay || meta.narrow) && e.key === 'ArrowRight') {
    p.x = meta.overlay
      ? (p.x ?? 0) + step
      : snapNarrowX((p.x ?? 0) + step, meta);
    render();
    e.preventDefault();
  }
});

els.mapCanvas.addEventListener('mousedown', (e) => {
  if (!state.tiles.length) return;
  const { x, y } = pointerOnMap(e);

  if (state.mode === 'tee') {
    const hit = teeAt(x, y);
    if (e.button === 2) {
      if (hit) {
        removeTee(hit.id);
        render();
      }
      return;
    }
    if (hit) {
      state.selectedTee = hit.id;
      startDrag({ kind: 'tee', id: hit.id, offsetX: x - hit.x, offsetY: y - hit.y });
      render();
      return;
    }
    state.tees.push({ id: state.nextTeeId++, x: Math.round(x), y: Math.round(y) });
    state.selectedTee = state.nextTeeId - 1;
    render();
    return;
  }

  const py = Math.round(y);
  const hit = placementAtPoint(x, py);
  const selMeta = metaForIndex(state.selectedTile);

  if (e.button === 2) {
    if (hit) {
      removePlacement(hit.id);
      render();
    }
    return;
  }

  if (hit) {
    state.selectedPlacement = hit.id;
    const hmeta = metaForIndex(hit.tileIndex);
    const drag = { kind: 'tile', id: hit.id, offsetY: py - hit.y };
    if (hmeta.overlay || hmeta.narrow) {
      drag.offsetX = x - placementDrawX(hit);
    }
    startDrag(drag);
    render();
    return;
  }

  /** @type {Placement} */
  const col = colFromX(x);
  let placement;
  if (selMeta.overlay) {
    placement = {
      id: state.nextId++,
      col: 0,
      x: Math.round(x - selMeta.w / 2),
      y: tileYClamp(Math.round(y)),
      tileIndex: state.selectedTile,
    };
  } else {
    placement = {
      id: state.nextId++,
      col,
      y: defaultPlacementY(state.selectedTile),
      tileIndex: state.selectedTile,
    };
    if (selMeta.narrow) {
      placement.x = snapNarrowX(x, selMeta);
    }
  }
  state.placements.push(placement);
  state.selectedPlacement = placement.id;
  setStatus(
    selMeta.overlay
      ? `Hole placed — add tee + Save golf.toml`
      : `Placed col ${col + 1} @ y=${placement.y} (${state.mapCols} cols)`,
  );
  render();
});

els.mapCanvas.addEventListener('mousemove', (e) => {
  if (state.dragging) onDragMove(e);
});

els.mapCanvas.addEventListener('contextmenu', (e) => e.preventDefault());

els.btnModeTiles.addEventListener('click', () => setMode('tiles'));
els.btnModeTee.addEventListener('click', () => setMode('tee'));

document.getElementById('btn-fill-grass')?.addEventListener('click', () => {
  const ok = confirm(
    'Fill enclosed grass gaps?\n\nOnly run this after your map layout is finished. ' +
      'It fills air pockets surrounded by grass and affects PNG export until you reload the project.',
  );
  if (!ok) return;
  state.autoGrassFill = true;
  render();
  setStatus('Grass fill applied — export PNG when ready');
});

document.getElementById('btn-add-col').addEventListener('click', () => {
  state.mapCols += 1;
  resizeMapCanvas();
  render();
});

document.getElementById('btn-add-col-10').addEventListener('click', () => {
  state.mapCols += 10;
  resizeMapCanvas();
  render();
  setStatus(`Map width → ${state.mapCols} columns (${mapWidthPx()}px)`);
});

els.inpCols.addEventListener('change', () => {
  const n = parseInt(els.inpCols.value, 10);
  if (Number.isFinite(n) && n >= 1) {
    state.mapCols = n;
    resizeMapCanvas();
    render();
    setStatus(`Map width → ${state.mapCols} columns`);
  }
});

document.getElementById('btn-del-col').addEventListener('click', () => {
  if (state.mapCols <= 1) return;
  state.mapCols -= 1;
  state.placements = state.placements.filter((p) => {
    const meta = metaForIndex(p.tileIndex);
    if (meta.narrow) return true;
    return p.col < state.mapCols;
  });
  resizeMapCanvas();
  render();
});

document.getElementById('btn-taller').addEventListener('click', () => {
  state.mapHeight += 8;
  resizeMapCanvas();
  render();
});

document.getElementById('btn-shorter').addEventListener('click', () => {
  if (state.mapHeight <= 64) return;
  state.mapHeight -= 8;
  state.tees = state.tees.filter((t) => t.y >= 0 && t.y <= state.mapHeight);
  resizeMapCanvas();
  render();
});

document.getElementById('btn-clear').addEventListener('click', () => {
  if (!confirm('Clear all tiles and tees?')) return;
  state.placements = [];
  state.tees = [];
  state.selectedPlacement = null;
  state.selectedTee = null;
  render();
});

function holePlacements() {
  return state.placements.filter((p) => metaForIndex(p.tileIndex).overlay);
}

function exportGolfToml(mapName = 'golf_map.png') {
  const holes = holePlacements();
  const par = Math.max(1, parseInt(els.inpPar.value, 10) || 3);
  const cups = holes.map((p) => {
    const px = placementDrawX(p);
    const canvas = state.tiles[p.tileIndex];
    return { x: px, y: p.y, geom: cupGeomFromHoleTile(canvas, px, p.y) };
  });
  if (state.tees.length === 0 || cups.length === 0) {
    setStatus('Need at least one tee and one hole cup overlay');
    return null;
  }
  const toml = buildGolfToml({
    mapName,
    defaultPar: par,
    tees: state.tees.map((t) => ({ x: t.x, y: t.y })),
    cups,
  });
  downloadText(toml, 'golf.toml');
  const n = Math.min(state.tees.length, cups.length);
  if (state.tees.length !== cups.length) {
    setStatus(`golf.toml — ${n} pairs (warning: ${state.tees.length} tees, ${cups.length} cups)`);
  } else {
    setStatus(`golf.toml — ${n} hole(s), par ${par}`);
  }
  return toml;
}

document.getElementById('btn-save-toml').addEventListener('click', () => {
  exportGolfToml('golf_map.png');
});

document.getElementById('btn-save-png').addEventListener('click', async () => {
  render();
  const { png, width, height, colors } = await exportWaPngAsync(els.mapCanvas);
  downloadBlob(new Blob([png], { type: 'image/png' }), 'golf_map.png');
  if (state.tees.length > 0 && holePlacements().length > 0) {
    exportGolfToml('golf_map.png');
  } else {
    setStatus(`Exported golf_map.png ${width}×${height} — add tee + hole for golf.toml`);
  }
});

document.getElementById('btn-save-project').addEventListener('click', () => {
  const data = {
    version: 3,
    mapCols: state.mapCols,
    mapHeight: state.mapHeight,
    tileW: state.tileW,
    tileH: state.tileH,
    snapW: state.snapW,
    tileLeftPad: state.tileLeftPad,
    placements: state.placements,
    tees: state.tees,
    autoGrassFill: state.autoGrassFill,
    defaultPar: parseInt(els.inpPar.value, 10) || 3,
  };
  downloadBlob(
    new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' }),
    'golf_map.wkgolf.json',
  );
});

document.getElementById('btn-load-project').addEventListener('change', async (e) => {
  const file = /** @type {HTMLInputElement} */ (e.target).files?.[0];
  if (!file) return;
  const data = JSON.parse(await file.text());
  state.mapCols = data.mapCols;
  state.mapHeight = data.mapHeight;
  if (data.snapW) state.snapW = data.snapW;
  if (data.tileLeftPad != null) state.tileLeftPad = data.tileLeftPad;
  state.placements = data.placements || [];
  state.tees = data.tees || [];
  if (data.defaultPar != null) els.inpPar.value = String(data.defaultPar);
  if (data.autoGrassFill != null) state.autoGrassFill = data.autoGrassFill;
  state.nextId = Math.max(0, ...state.placements.map((p) => p.id), 0) + 1;
  state.nextTeeId = Math.max(0, ...state.tees.map((t) => t.id), 0) + 1;
  growMapCols(maxUsedCol());
  resizeMapCanvas();
  render();
  setStatus(`Project loaded — ${state.tees.length} tee(s), ${state.mapCols} cols`);
});

els.tilesetInput.addEventListener('change', (e) => {
  const file = /** @type {HTMLInputElement} */ (e.target).files?.[0];
  if (!file) return;
  const img = new Image();
  img.onload = () => loadTilesetFromImage(img);
  img.src = URL.createObjectURL(file);
});

async function boot() {
  for (const url of ['assets/tileset.png', 'assets/side_view_golf_tileset_-_tiles.png']) {
    try {
      const img = new Image();
      img.src = url;
      await img.decode();
      loadTilesetFromImage(img);
      setMode('tiles');
      return;
    } catch {
      /* next */
    }
  }
  setStatus('Load tileset PNG (7×4 grid) — top-left tile is empty');
}

boot();
