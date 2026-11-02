wkGolf Map Editor
=================

Side-view fairway builder for wkGolf. Tiles snap to fixed horizontal columns;
vertical position is free within each column.

Run
---
  ./scripts/serve-editor.sh
  # open http://<host>:8790/

Tileset
-------
  editor/assets/tileset.png  — 7×4 grid (top-left = empty air tile).
  Load another PNG via the sidebar file picker if needed.

  **Girder wall** — last palette tile (20×140); snaps every 20px horizontally.
  Use between fairway sections as vertical dividers.

  **Worm scale** — yellow-green ~24×40 reference (not exported).
  **Worm** tool to drag it; Alt+drag worm in any mode. Toggle **Show worm scale**.

Controls
--------
  **Tiles** — pick palette tile → click map (columns snap seamlessly).
  Drag tile up/down; continue dragging past the map edge to crop.
  Arrow ↑/↓ nudge 1px (Shift = 8px). Parts outside the map are hidden;
  **Save WA PNG** exports the visible rectangle only.

  **Tee spawn** — click to place, drag to move, RMB erase.
  Multiple tees OK (per-team spawns on one fairway).

Export
------
  **Save WA PNG** — black air, indexed palette, divisible by 8. Includes hole overlays.

  **Save golf.toml** — auto-pairs **tee spawns** with **hole cup** overlays
  (sorted left→right). Set default **Par** per hole. Also downloads when you
  Save WA PNG if both tees and cups exist.

  Copy `golf_map.png` + `golf.toml` next to wkGolf.dll.

  **Hole cup** palette tile — red lip + black air only (80×138). Place freely
  on fairway; drag to position. Cuts a physical hole in WA.

After export, run prepare_golf_map.py if you add tee/cup markers, or use
the PNG directly as terrain and set golf.toml origins manually.
