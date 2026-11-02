/** WA air — black, not filled. */
export function isAir(r, g, b, a = 255) {
  return a <= 8 || (r <= 16 && g <= 16 && b <= 16);
}

/**
 * Fill ONLY enclosed air pockets (not connected to top edge) with solid body colour.
 * No pattern tiling. Does not fill open sky below terrain.
 *
 * @param {CanvasRenderingContext2D} ctx
 * @param {number} width
 * @param {number} height
 * @param {{ r: number, g: number, b: number }} fill
 */
export function applyGrassFill(ctx, width, height, fill) {
  const img = ctx.getImageData(0, 0, width, height);
  const { data } = img;
  const n = width * height;

  /** @param {number} i */
  function airAt(i) {
    const o = i * 4;
    return isAir(data[o], data[o + 1], data[o + 2], data[o + 3]);
  }

  const exterior = new Uint8Array(n);
  const q = [];

  /** @param {number} i */
  function tryExterior(i) {
    if (exterior[i] || !airAt(i)) return;
    exterior[i] = 1;
    q.push(i);
  }

  for (let x = 0; x < width; x++) tryExterior(x);

  while (q.length) {
    const i = q.pop();
    const x = i % width;
    const y = (i / width) | 0;
    if (x > 0) tryExterior(i - 1);
    if (x + 1 < width) tryExterior(i + 1);
    if (y > 0) tryExterior(i - width);
    if (y + 1 < height) tryExterior(i + width);
  }

  const { r, g, b } = fill;
  let filled = 0;
  for (let i = 0; i < n; i++) {
    if (!airAt(i) || exterior[i]) continue;
    const o = i * 4;
    data[o] = r;
    data[o + 1] = g;
    data[o + 2] = b;
    data[o + 3] = 255;
    filled++;
  }

  ctx.putImageData(img, 0, 0);
  return filled;
}
