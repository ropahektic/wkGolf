/**
 * Export RGBA canvas to WA-compatible 8-bit indexed PNG.
 * Black (air) = palette index 0. Max 64 colors. Dimensions divisible by 8.
 */
export function padDiv8(n) {
  return Math.ceil(n / 8) * 8;
}

/** @param {ImageData} img */
export function buildWaPalette(img) {
  const { data, width, height } = img;
  const isAir = (r, g, b) => r <= 16 && g <= 16 && b <= 16;
  const key = (r, g, b) => `${r},${g},${b}`;
  const map = new Map();
  /** @type {number[][]} */
  const colors = [[0, 0, 0]];

  for (let i = 0; i < width * height; i++) {
    const o = i * 4;
    const r = data[o];
    const g = data[o + 1];
    const b = data[o + 2];
    if (isAir(r, g, b)) continue;
    const k = key(r, g, b);
    if (!map.has(k)) {
      if (colors.length >= 64) continue;
      map.set(k, colors.length);
      colors.push([r, g, b]);
    }
  }
  return { colors, map, isAir };
}

/** @param {ImageData} img */
export function rgbaToIndices(img, palette) {
  const { data, width, height } = img;
  const { colors, map, isAir } = palette;
  const out = new Uint8Array(width * height);
  for (let i = 0; i < width * height; i++) {
    const o = i * 4;
    const r = data[o];
    const g = data[o + 1];
    const b = data[o + 2];
    if (isAir(r, g, b)) {
      out[i] = 0;
      continue;
    }
    const k = `${r},${g},${b}`;
    let idx = map.get(k);
    if (idx === undefined) {
      // nearest color if palette full
      let best = 1;
      let bestD = Infinity;
      for (let c = 1; c < colors.length; c++) {
        const [cr, cg, cb] = colors[c];
        const d = (r - cr) ** 2 + (g - cg) ** 2 + (b - cb) ** 2;
        if (d < bestD) {
          bestD = d;
          best = c;
        }
      }
      idx = best;
    }
    out[i] = idx;
  }
  return out;
}

function crc32(buf) {
  let c = 0xffffffff;
  const table = crc32.table || (crc32.table = (() => {
    const t = new Uint32Array(256);
    for (let n = 0; n < 256; n++) {
      let c = n;
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      t[n] = c;
    }
    return t;
  })());
  for (let i = 0; i < buf.length; i++) c = table[(c ^ buf[i]) & 0xff] ^ (c >>> 1);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = data.length;
  const out = new Uint8Array(8 + len + 4);
  const dv = new DataView(out.buffer);
  dv.setUint32(0, len);
  out[4] = type.charCodeAt(0);
  out[5] = type.charCodeAt(1);
  out[6] = type.charCodeAt(2);
  out[7] = type.charCodeAt(3);
  out.set(data, 8);
  dv.setUint32(8 + len, crc32(out.subarray(4, 8 + len)));
  return out;
}

/** @param {number[][]} colors up to 256 entries */
export function encodeIndexedPng(width, height, indices, colors) {
  const ihdr = new Uint8Array(13);
  const dv = new DataView(ihdr.buffer);
  dv.setUint32(0, width);
  dv.setUint32(4, height);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 3; // indexed
  ihdr[10] = 0;
  ihdr[11] = 0;
  ihdr[12] = 0;

  const plte = new Uint8Array(768);
  for (let i = 0; i < 256; i++) {
    const [r = 0, g = 0, b = 0] = colors[i] || [0, 0, 0];
    plte[i * 3] = r;
    plte[i * 3 + 1] = g;
    plte[i * 3 + 2] = b;
  }

  const rowSize = 1 + width;
  const raw = new Uint8Array(rowSize * height);
  for (let y = 0; y < height; y++) {
    raw[y * rowSize] = 0; // filter none
    raw.set(indices.subarray(y * width, (y + 1) * width), y * rowSize + 1);
  }

  // deflate — use CompressionStream if available, else store uncompressed IDAT (PNG allows zlib)
  const compressed = deflateSync(raw);

  const sig = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]);
  const parts = [
    sig,
    chunk('IHDR', ihdr),
    chunk('PLTE', plte.subarray(0, colors.length * 3)),
    chunk('IDAT', compressed),
    chunk('IEND', new Uint8Array(0)),
  ];
  let total = 0;
  for (const p of parts) total += p.length;
  const out = new Uint8Array(total);
  let off = 0;
  for (const p of parts) {
    out.set(p, off);
    off += p.length;
  }
  return out;
}

function deflateSync(data) {
  // Minimal zlib wrapper around raw deflate using CompressionStream (modern browsers / node 18+)
  if (typeof CompressionStream !== 'undefined') {
    throw new Error('async deflate required — use exportWaPngAsync');
  }
  return zlibStore(data);
}

/** Store-only fallback (works everywhere, slightly larger files). */
function zlibStore(data) {
  const adler = adler32(data);
  const len = data.length;
  const out = new Uint8Array(2 + 4 + len + 4);
  out[0] = 0x78;
  out[1] = 0x01;
  out[2] = len & 0xff;
  out[3] = (len >> 8) & 0xff;
  out[4] = ~out[2] & 0xff;
  out[5] = ~out[3] & 0xff;
  out.set(data, 6);
  const dv = new DataView(out.buffer);
  dv.setUint32(6 + len, adler);
  return out;
}

function adler32(data) {
  let a = 1;
  let b = 0;
  for (let i = 0; i < data.length; i++) {
    a = (a + data[i]) % 65521;
    b = (b + a) % 65521;
  }
  return ((b << 16) | a) >>> 0;
}

/** @param {HTMLCanvasElement} canvas */
export async function exportWaPngAsync(canvas) {
  const w = padDiv8(canvas.width);
  const h = padDiv8(canvas.height);
  const tmp = document.createElement('canvas');
  tmp.width = w;
  tmp.height = h;
  const ctx = tmp.getContext('2d');
  ctx.fillStyle = '#000000';
  ctx.fillRect(0, 0, w, h);
  ctx.drawImage(canvas, 0, 0);
  const img = ctx.getImageData(0, 0, w, h);
  const palette = buildWaPalette(img);
  const indices = rgbaToIndices(img, palette);
  const flat = palette.colors.concat();
  while (flat.length < 256) flat.push([0, 0, 0]);
  const png = encodeIndexedPng(w, h, indices, flat);
  return { png, width: w, height: h, colors: palette.colors.length };
}

export function downloadBlob(blob, filename) {
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = filename;
  a.click();
  URL.revokeObjectURL(a.href);
}
