//! HUD text @ post-RQ hook; landing mark via stock land cursor (airstrike X path).

use std::sync::Mutex;

use crate::log;
use crate::wa_mem;

const VA_SET_TEXTBOX_TEXT: u32 = 0x004F_B070;
const OFF_WORLD_DISPLAY: usize = 0x004;
const OFF_WORLD_TEXTBOX: usize = 0x544;
const OFF_GFX_BORDER: usize = 0x7324;
const OFF_GFX_FILL: usize = 0x7328;
const OFF_DISPLAY_VT_GET_DIMS: usize = 0x04;
const OFF_DISPLAY_VT_DRAW_SCALED: usize = 0x50;
/// DisplayGfx camera scroll — DrawScaledSprite adds these (HUD text only).
const OFF_DISPLAY_VIEW_X: usize = 0x3560;
const OFF_DISPLAY_VIEW_Y: usize = 0x3564;

const MARGIN_X: i32 = 14;
const MARGIN_Y: i32 = 10;
const LINE_GAP: i32 = 3;
const HUD_DRAW_FLAGS: u32 = 0x10_0000;

type SetTextboxTextFn = unsafe extern "system" fn(
    textbox: *mut u8,
    msg: *const u8,
    font_index: i32,
    fill_color: i32,
    border_color: i32,
    out_w: *mut i32,
    out_h: *mut i32,
    scale: i32,
) -> usize;

type GetDimensionsFn = unsafe extern "thiscall" fn(*mut u8, *mut u32, *mut u32);
type DrawScaledSpriteFn = unsafe extern "thiscall" fn(
    display: *mut u8,
    x: i32,
    y: i32,
    bitmap: usize,
    src_x: i32,
    src_y: i32,
    src_w: i32,
    src_h: i32,
    flags: u32,
);

#[derive(Clone, Debug, Default)]
pub struct GolfHudLayout {
    pub hole_title: String,
    pub par_line: String,
    pub spin_line: String,
    pub player_line: String,
    pub shot_line: String,
    pub status_line: String,
    pub center_line: String,
}

static HUD_LAYOUT: Mutex<GolfHudLayout> = Mutex::new(GolfHudLayout {
    hole_title: String::new(),
    par_line: String::new(),
    spin_line: String::new(),
    player_line: String::new(),
    shot_line: String::new(),
    status_line: String::new(),
    center_line: String::new(),
});

static LANDING_MARK: Mutex<Option<(f64, f64)>> = Mutex::new(None);

static mut DRAW_WARNED: bool = false;
static mut HUD_DISABLED: bool = false;
static mut MARKERR_LOGGED: bool = false;

fn px_to_fixed(v: i32) -> i32 {
    v << 16
}

pub fn set_hud_layout(layout: GolfHudLayout) {
    if let Ok(mut g) = HUD_LAYOUT.lock() {
        *g = layout;
    }
}

pub fn clear_hud() {
    set_hud_layout(GolfHudLayout::default());
    set_landing_mark(None);
}

pub fn set_landing_mark(pos: Option<(f64, f64)>) {
    if let Ok(mut g) = LANDING_MARK.lock() {
        *g = pos;
    }
}

/// Push landing point into stock `world+0x7380` + `TeamEntity__render_cursor`.
pub fn sync_landing_mark(team: u32) {
    let pos = LANDING_MARK.try_lock().ok().and_then(|g| *g);
    match pos {
        Some((x, y)) => {
            if let Err(e) = wa_mem::show_land_mark(x, y, team) {
                if unsafe { !MARKERR_LOGGED } {
                    unsafe { MARKERR_LOGGED = true; }
                    log::line(&format!("[wkGolf] land mark FAIL: {e}"));
                }
                return;
            }
            if unsafe { !MARKERR_LOGGED } {
                unsafe { MARKERR_LOGGED = true; }
                log::line(&format!(
                    "[wkGolf] land mark stock ({x:.0},{y:.0}) sprite={}",
                    wa_mem::SPRITE_MARKERR
                ));
            }
        }
        None => {
            let _ = wa_mem::hide_land_mark(team);
        }
    }
}

pub fn flush_hud_overlay() {
    unsafe {
        if HUD_DISABLED {
            return;
        }
    }
    if !wa_mem::headful_frame_active() {
        return;
    }
    let Ok(world) = wa_mem::resolve_runtime_world() else {
        return;
    };
    let Some(display) = wa_mem::read_ptr(world + OFF_WORLD_DISPLAY) else {
        return;
    };
    if display == 0 || !wa_mem::ptr_readable(display, 0x1000) {
        return;
    };
    let Some(textbox) = wa_mem::read_ptr(world + OFF_WORLD_TEXTBOX) else {
        return;
    };
    if textbox == 0 || !wa_mem::ptr_readable(textbox, 0x40) {
        return;
    }

    let layout = HUD_LAYOUT
        .try_lock()
        .map(|g| g.clone())
        .unwrap_or_default();
    let (view_w, _view_h) = unsafe { viewport_pixel_size(display) };

    unsafe {
        let mut y = MARGIN_Y;
        if !layout.hole_title.is_empty() {
            if let Some(h) = blit_text_at_screen(world, display, MARGIN_X, y, &layout.hole_title) {
                y += h + LINE_GAP;
            }
        }
        if !layout.par_line.is_empty() {
            if let Some(h) = blit_text_at_screen(world, display, MARGIN_X, y, &layout.par_line) {
                y += h + LINE_GAP;
            }
        }
        if !layout.spin_line.is_empty() {
            let _ = blit_text_at_screen(world, display, MARGIN_X, y, &layout.spin_line);
        }

        let right = view_w - MARGIN_X;
        let mut y = MARGIN_Y;
        for line in [
            &layout.player_line,
            &layout.shot_line,
            &layout.status_line,
        ] {
            if line.is_empty() {
                continue;
            }
            if let Some(h) = blit_text_at_screen_right(world, display, right, y, line) {
                y += h + LINE_GAP;
            }
        }

        if !layout.center_line.is_empty() {
            let _ = blit_text_at_screen_center(world, display, view_w, &layout.center_line);
        }
    }
}

unsafe fn viewport_pixel_size(display: usize) -> (i32, i32) {
    let Some(vt) = wa_mem::read_ptr(display) else {
        return (640, 480);
    };
    let Some(get_dim_ptr) = wa_mem::read_ptr(vt + OFF_DISPLAY_VT_GET_DIMS) else {
        return (640, 480);
    };
    let get_dim: GetDimensionsFn = std::mem::transmute(get_dim_ptr);
    let mut w = 0u32;
    let mut h = 0u32;
    get_dim(display as *mut u8, &mut w, &mut h);
    let w = w as i32;
    let h = h as i32;
    if w <= 0 {
        (640, h.max(480))
    } else {
        (w, h.max(480))
    }
}

unsafe fn screen_to_draw_xy(display: usize, screen_x: i32, screen_y: i32, w: i32, h: i32) -> (i32, i32) {
    let cam_x = wa_mem::read_i32_at(display + OFF_DISPLAY_VIEW_X).unwrap_or(0);
    let cam_y = wa_mem::read_i32_at(display + OFF_DISPLAY_VIEW_Y).unwrap_or(0);
    let x = screen_x - cam_x + w / 2;
    let y = screen_y - cam_y + h / 2;
    (x, y)
}

unsafe fn blit_text_at_screen(
    world: usize,
    display: usize,
    screen_x: i32,
    screen_y: i32,
    text: &str,
) -> Option<i32> {
    let (bm, w, h) = bake_textbox(world, text)?;
    if w <= 0 || h <= 0 || bm == 0 {
        return None;
    }
    let (x, y) = screen_to_draw_xy(display, screen_x, screen_y, w, h);
    if blit_scaled_sprite_vtable(display, px_to_fixed(x), px_to_fixed(y), bm, 0, 0, w, h, HUD_DRAW_FLAGS)
    {
        Some(h)
    } else {
        disable_hud("display vtable blit failed");
        None
    }
}

unsafe fn blit_text_at_screen_right(
    world: usize,
    display: usize,
    screen_right: i32,
    screen_y: i32,
    text: &str,
) -> Option<i32> {
    let (bm, w, h) = bake_textbox(world, text)?;
    if w <= 0 || h <= 0 || bm == 0 {
        return None;
    }
    let screen_x = (screen_right - w).max(MARGIN_X);
    blit_text_at_screen(world, display, screen_x, screen_y, text)
}

unsafe fn blit_text_at_screen_center(
    world: usize,
    display: usize,
    view_w: i32,
    text: &str,
) -> Option<i32> {
    let (bm, w, h) = bake_textbox(world, text)?;
    if w <= 0 || h <= 0 || bm == 0 {
        return None;
    }
    let screen_x = ((view_w - w) / 2).max(MARGIN_X);
    let screen_y = 72;
    let (x, y) = screen_to_draw_xy(display, screen_x, screen_y, w, h);
    if blit_scaled_sprite_vtable(display, px_to_fixed(x), px_to_fixed(y), bm, 0, 0, w, h, HUD_DRAW_FLAGS)
    {
        Some(h)
    } else {
        disable_hud("display vtable blit failed");
        None
    }
}

unsafe fn disable_hud(reason: &str) {
    if !DRAW_WARNED {
        DRAW_WARNED = true;
        log::line(&format!("[wkGolf] hud: {reason} — on-screen HUD off"));
        HUD_DISABLED = true;
    }
}

unsafe fn blit_scaled_sprite_vtable(
    display: usize,
    x: i32,
    y: i32,
    bm: usize,
    src_x: i32,
    src_y: i32,
    w: i32,
    h: i32,
    flags: u32,
) -> bool {
    let Some(vt) = wa_mem::read_ptr(display) else {
        return false;
    };
    let Some(slot) = wa_mem::read_ptr(vt + OFF_DISPLAY_VT_DRAW_SCALED) else {
        return false;
    };
    if slot == 0 {
        return false;
    }
    let draw: DrawScaledSpriteFn = std::mem::transmute(slot);
    draw(
        display as *mut u8,
        x,
        y,
        bm,
        src_x,
        src_y,
        w,
        h,
        flags,
    );
    true
}

unsafe fn bake_textbox(world: usize, msg: &str) -> Option<(usize, i32, i32)> {
    let textbox = wa_mem::read_ptr(world + OFF_WORLD_TEXTBOX).filter(|p| *p != 0)?;
    if !wa_mem::ptr_readable(textbox, 0x40) {
        return None;
    }
    let fill = wa_mem::read_i32_at(world + OFF_GFX_FILL).unwrap_or(0);
    let border = wa_mem::read_i32_at(world + OFF_GFX_BORDER).unwrap_or(0);
    let mut buf = msg.as_bytes().to_vec();
    buf.push(0);
    let mut w = 0i32;
    let mut h = 0i32;
    let set_text: SetTextboxTextFn = std::mem::transmute(wa_mem::ghidra_va(VA_SET_TEXTBOX_TEXT));
    let bm = set_text(
        textbox as *mut u8,
        buf.as_ptr(),
        6,
        fill,
        border,
        &mut w,
        &mut h,
        0x1_0000,
    );
    if bm == 0 {
        if !DRAW_WARNED {
            DRAW_WARNED = true;
            log::line("[wkGolf] hud: textbox bake failed — on-screen HUD off");
            HUD_DISABLED = true;
        }
        return None;
    }
    Some((bm, w, h))
}
