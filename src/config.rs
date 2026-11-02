//! Course definition from golf.toml next to the DLL.

use std::fs;
use std::path::{Path, PathBuf};

use crate::log;

#[derive(Clone, Debug)]
pub struct Hole {
    /// Cell origin on the PNG (each hole lives in its own square).
    pub origin_x: f64,
    pub origin_y: f64,
    pub tee_x: f64,
    pub tee_y: f64,
    pub cup_x: f64,
    pub cup_y: f64,
    pub radius: f64,
    pub par: u32,
}

impl Hole {
    pub fn tee_world(&self, team: u32) -> (f64, f64) {
        (
            self.origin_x + crate::worm_move::tee_x_for_team(self.tee_x, team),
            self.origin_y + self.tee_y,
        )
    }

    pub fn cup_world(&self) -> (f64, f64) {
        (self.origin_x + self.cup_x, self.origin_y + self.cup_y)
    }

    /// Cup capture: horizontal band at/below the lip. Grenade dets under terrain still count.
    pub fn cup_contains(&self, x: f64, y: f64) -> bool {
        let (cx, cy) = self.cup_world();
        let dx = x - cx;
        let half_w = self.radius + 16.0;
        if dx.abs() > half_w {
            return false;
        }
        // WA detonation center often sits below visible terrain inside the cup.
        let surface = cy - self.radius * 0.55;
        y >= surface
    }
}

#[derive(Clone, Debug)]
pub struct Course {
    pub map_png: String,
    pub hud: bool,
    /// Fraction of stock bazooka wind while airborne (0..1). Marker ignores wind.
    pub wind_scale: f64,
    pub holes: Vec<Hole>,
}

impl Course {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        parse(&text)
    }

    pub fn default_test() -> Self {
        Self {
            map_png: "GolfTest1.png".into(),
            hud: true,
            wind_scale: 0.10,
            holes: vec![
                Hole {
                    origin_x: 0.0,
                    origin_y: 0.0,
                    tee_x: 120.0,
                    tee_y: 300.0,
                    cup_x: 720.0,
                    cup_y: 351.0,
                    radius: 22.0,
                    par: 2,
                },
                Hole {
                    origin_x: 960.0,
                    origin_y: 0.0,
                    tee_x: 120.0,
                    tee_y: 300.0,
                    cup_x: 720.0,
                    cup_y: 351.0,
                    radius: 22.0,
                    par: 2,
                },
            ],
        }
    }
}

fn parse(text: &str) -> Result<Course, String> {
    let mut map_png = String::from("GolfTest1.png");
    let mut hud = true;
    let mut wind_scale = 0.10_f64;
    let mut holes: Vec<Hole> = Vec::new();
    let mut in_holes = false;
    let mut cur: Option<(f64, f64, f64, f64, f64, f64, f64, u32)> = None;

    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line == "[[holes]]" {
            if let Some((ox, oy, tx, ty, cx, cy, r, p)) = cur.take() {
                holes.push(Hole {
                    origin_x: ox,
                    origin_y: oy,
                    tee_x: tx,
                    tee_y: ty,
                    cup_x: cx,
                    cup_y: cy,
                    radius: r,
                    par: p,
                });
            }
            in_holes = true;
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim();
            let v = v.trim().trim_matches('"');
            if k == "map" {
                map_png = v.to_string();
                continue;
            }
            if k == "hud" {
                hud = matches!(v, "1" | "true" | "yes" | "on");
                continue;
            }
            if k == "wind_scale" {
                wind_scale = v
                    .parse::<f64>()
                    .map_err(|_| format!("bad wind_scale: {v}"))?
                    .clamp(0.0, 1.0);
                continue;
            }
            if in_holes {
                let slot = cur.get_or_insert((0.0, 0.0, 120.0, 300.0, 720.0, 351.0, 22.0, 2));
                match k {
                    "origin_x" => slot.0 = v.parse().map_err(|_| format!("bad origin_x: {v}"))?,
                    "origin_y" => slot.1 = v.parse().map_err(|_| format!("bad origin_y: {v}"))?,
                    "tee_x" => slot.2 = v.parse().map_err(|_| format!("bad tee_x: {v}"))?,
                    "tee_y" => slot.3 = v.parse().map_err(|_| format!("bad tee_y: {v}"))?,
                    "cup_x" => slot.4 = v.parse().map_err(|_| format!("bad cup_x: {v}"))?,
                    "cup_y" => slot.5 = v.parse().map_err(|_| format!("bad cup_y: {v}"))?,
                    "radius" => slot.6 = v.parse().map_err(|_| format!("bad radius: {v}"))?,
                    "par" => slot.7 = v.parse().map_err(|_| format!("bad par: {v}"))?,
                    _ => {}
                }
            }
        }
    }
    if let Some((ox, oy, tx, ty, cx, cy, r, p)) = cur.take() {
        holes.push(Hole {
            origin_x: ox,
            origin_y: oy,
            tee_x: tx,
            tee_y: ty,
            cup_x: cx,
            cup_y: cy,
            radius: r,
            par: p,
        });
    }
    if holes.is_empty() {
        return Err("golf.toml: no [[holes]] entries".into());
    }
    Ok(Course {
        map_png,
        hud,
        wind_scale,
        holes,
    })
}

pub fn load_beside_dll(module: usize) -> Course {
    let dir = crate::paths::dll_dir(module);
    let path = dir.join("golf.toml");
    match Course::load(&path) {
        Ok(c) => {
            log::line(&format!(
                "[wkGolf] course: map={} holes={} hud={} wind_scale={:.0}%",
                c.map_png,
                c.holes.len(),
                c.hud,
                c.wind_scale * 100.0
            ));
            c
        }
        Err(e) => {
            log::line(&format!("[wkGolf] {e} — using built-in test hole"));
            Course::default_test()
        }
    }
}

pub fn config_paths(module: usize) -> PathBuf {
    crate::paths::dll_dir(module)
}
