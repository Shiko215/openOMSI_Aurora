//! `tile_x_y.map` (unit `mc_mapclass`, `TMapKachel.loadMapFile`) including Chrono patch files.

use omsi_cfg::CfgFile;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MapObject {
    pub file: String,
    pub id: i64,
    pub pos: [f64; 3],
    /// Heading (about Z), then pitch (X) and bank (Y), degrees.
    pub rot: [f64; 3],
    /// Type flag written by the editor (0 plain, 1 text strings, 4 tree, 7 bus stop, …).
    pub flag: i32,
    /// Type-specific trailing lines (text-texture strings, tree parameters, bus stop data).
    pub extra: Vec<String>,
    /// For `[attachObj]`: the id of the object this one is attached to.
    pub parent_id: Option<i64>,
    /// For `[attachObj]`: which `[new_attachment]` point of the parent it hangs on (0-based;
    /// verified on all 1436 resolvable attachments of Berlin-Spandau against the parents'
    /// .sco files). `pos` is unused then: the attachment point is the position. A point
    /// beyond the parent's count is the parent's own origin (the original takes the identity
    /// matrix then; 17 of Ahlheim V5's 10 892 attachments rely on it).
    pub attach_index: usize,
    /// For `[attachObj]`: the instance of a spline-attachment row it hangs on.
    pub instance: usize,
    /// For `[varparent]` following the object.
    pub var_parent: Option<i64>,
    /// `[rule]` / `[kill_rule]` lines that follow the object: they apply to its `[path]`s.
    pub rules: Vec<MapRule>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MapSpline {
    pub file: String,
    pub id: i64,
    pub prev_id: i64,
    pub next_id: i64,
    /// Start point, tile-local metres: x east, y north, z up.
    pub pos: [f64; 3],
    /// Degrees, 0 = +y, clockwise positive.
    pub heading: f64,
    pub length: f64,
    /// 0 = straight; > 0 turns right (clockwise), < 0 left. Metres.
    pub radius: f64,
    /// Gradients in percent.
    pub grad_start: f64,
    pub grad_end: f64,
    /// `[spline_h]`: the height the spline climbs over its length. Its height runs as a
    /// cubic between the two gradients and ends exactly that much higher (the next spline
    /// of a chain starts there: 270 of 270 links on Berlin-Spandau, where the plain
    /// gradient parabola misses 182 of them by up to several metres).
    pub delta_h: Option<f64>,
    pub cant_start: f64,
    pub cant_end: f64,
    pub skew_start: f64,
    pub skew_end: f64,
    /// Last numeric field: where the spline's textures start along it, the length of the
    /// chain before it (OMSI reads it from tile version 11 on and draws `v = v_scale *
    /// (s + tex_offset)`, so a chain's textures run on across its joints; Omsi.exe
    /// sub_79a8ec puts it into TSplineSegment+0x1a0).
    pub tex_offset: f64,
    pub mirror: bool,
    pub is_h: bool,
    pub rules: Vec<MapRule>,
    pub terrain_align: Option<f64>,
    /// `[spline_terrain_align]` without parameter.
    pub terrain_align_flag: bool,
    /// Removed by a chrono `[delete]`. The spline stays in the list because spline
    /// attachments (also those of later chrono folders) name their spline by its index.
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MapRule {
    pub path_index: i32,
    pub kind: String,
    pub value: f64,
    pub extra: f64,
    pub kill: bool,
}

/// `[splineAttachement]`: objects placed along a spline of this tile and the splines after it
/// in the same tile (street lamps, parking bays, catenary masts).
/// `[splineAttachement_repeater]` carries the row on where its chain enters another tile: it
/// repeats the master's parameters and says where the master is and how many of its objects
/// came before this spline.
///
/// Record: `0`, (repeater: master tile index, first object index), file, id, spline index
/// (in this tile's spline order), lateral offset x (right positive), height above the
/// spline, start distance along the chain, heading, pitch, bank (relative to the spline
/// direction), interval, range, tilt flag, string count, strings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SplineAttachment {
    pub file: String,
    pub id: i64,
    pub spline_index: i32,
    /// Lateral offset (m, right of the spline direction), height above the spline (m) and
    /// distance of the first object from the start of the spline's chain (m; the splines
    /// before this one count, see docs/FORMATS.md).
    pub offset: [f64; 3],
    pub rot: [f64; 3],
    /// Distance between two objects (0 = a single object).
    pub interval: f64,
    /// How far the row reaches from its first object (m): object `j` exists while
    /// `j * interval <= range`. (All 185 repeaters of the stock maps satisfy this; measured
    /// from the spline start instead, 18 of them would lie beyond the end of their row.)
    pub range: f64,
    /// 1: the object leans with the spline's gradient and cant (parking bays and arrows on
    /// the road, conductor rail brackets); 0: it stands upright.
    pub tilt: bool,
    /// Text-texture strings (street names, line numbers).
    pub strings: Vec<String>,
    /// `[splineAttachement_repeater]`: (index of the master's tile in global.cfg's `[map]`
    /// list - counting the tiles whose files are missing - and index of the first object of
    /// the row that lies on this spline).
    pub repeater: Option<(usize, usize)>,
    pub var_parent: Option<i64>,
    pub rules: Vec<MapRule>,
}

/// Chrono patch: `[selobject]` / `[selspline]` name an object or spline **by id** (838 of
/// the 882 selections of Berlin-Spandau's chrono folders name an object of the same base
/// tile; the rest name objects that an earlier chrono folder added), followed by
/// `[delete]`, `[typ] <file>`, `[relabel] <count> <strings…>`, `[rule]` and `[kill_rule]`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChronoChange {
    pub is_spline: bool,
    pub id: i64,
    pub delete: bool,
    pub new_type: Option<String>,
    /// New text-texture strings of the object.
    pub relabel: Option<Vec<String>>,
    pub rules: Vec<MapRule>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tile {
    pub path: PathBuf,
    pub version: i32,
    pub has_terrain: bool,
    pub has_water: bool,
    pub variable_terrain: bool,
    pub variable_terrain_lightmap: bool,
    pub objects: Vec<MapObject>,
    pub attach_objects: Vec<MapObject>,
    pub splines: Vec<MapSpline>,
    pub spline_attachments: Vec<SplineAttachment>,
    pub chrono_changes: Vec<ChronoChange>,
    pub unknown_keywords: Vec<(String, usize)>,
}

impl Tile {
    /// A `[worldcoordinates]` tile of row `ty` scaled onto the map's grid (see
    /// `world_tile_scale`): the objects' and splines' places across and along, the splines'
    /// lengths and radii, and the spline attachments' distances along their chains. Heights
    /// and angles stay. Nothing changes on a plain map.
    pub fn fit_to_world_grid(&mut self, ty: i32) {
        let (kx, ky) = crate::world_tile_scale(ty);
        if (kx - 1.0).abs() < 1e-12 && (ky - 1.0).abs() < 1e-12 {
            return;
        }
        let k = (kx + ky) / 2.0;
        for o in self.objects.iter_mut().chain(self.attach_objects.iter_mut()) {
            o.pos[0] *= kx;
            o.pos[1] *= ky;
        }
        for s in self.splines.iter_mut() {
            s.pos[0] *= kx;
            s.pos[1] *= ky;
            s.length *= k;
            s.radius *= k;
        }
        for a in self.spline_attachments.iter_mut() {
            a.offset[2] *= k;
            a.interval *= k;
            a.range *= k;
        }
    }
}

fn read_extra(r: &mut omsi_cfg::CfgReader) -> Vec<String> {
    let mut v: Vec<String> = r
        .rest_of_block()
        .into_iter()
        .map(|s| s.trim_end().to_string())
        .collect();
    while v
        .last()
        .map(|s| s.trim().is_empty() || s.starts_with("Object Nr."))
        .unwrap_or(false)
    {
        v.pop();
    }
    v
}

fn read_rule(r: &mut omsi_cfg::CfgReader, kill: bool) -> MapRule {
    let path_index = r.i32();
    let kind = r.word().to_string();
    let value = r.f64();
    // fourth line is present in current files; absent in very old ones
    let save = r.pos();
    let l = r.str();
    let extra = if omsi_cfg::keyword_of(l).is_some() || l.trim().is_empty() {
        r.seek(save);
        0.0
    } else {
        omsi_cfg::parse_f64(l)
    };
    MapRule {
        path_index,
        kind,
        value,
        extra,
        kill,
    }
}

/// A chrono folder's rules on top of an element's own: `[kill_rule]` takes away the rule of
/// the same path, kind and vehicle class (the chrono folders pair `[kill_rule] 1
/// trafficdensity 1.000 0` with `[rule] 1 trafficdensity 1.000 4`), `[rule]` adds one.
fn apply_rules(rules: &mut Vec<MapRule>, changes: &[MapRule]) {
    for c in changes {
        if c.kill {
            rules.retain(|r| {
                r.kill
                    || !(r.path_index == c.path_index
                        && r.kind.eq_ignore_ascii_case(&c.kind)
                        && r.extra == c.extra)
            });
        } else {
            rules.push(c.clone());
        }
    }
}

impl Tile {
    pub fn load(path: &Path) -> Result<Tile, omsi_cfg::CfgError> {
        let f = CfgFile::read(path)?;
        Ok(Self::parse(&f))
    }

    pub fn parse(file: &CfgFile) -> Tile {
        let mut t = Tile {
            path: file.path.clone(),
            ..Default::default()
        };
        let mut r = file.reader();
        // what the last [rule]/[varparent]/... applies to
        #[derive(Clone, Copy)]
        enum Last {
            None,
            Object,
            Attach,
            Spline,
            SplineAttach,
            Chrono,
        }
        let mut last = Last::None;
        while let Some(k) = r.next_keyword() {
            match k.as_str() {
                "version" => t.version = r.i32(),
                "terrain" => t.has_terrain = true,
                "water" => t.has_water = true,
                "variable_terrain" => t.variable_terrain = true,
                "variable_terrainlightmap" => t.variable_terrain_lightmap = true,
                "object" | "attachobj" => {
                    let _zero = r.line();
                    let file_name = r.str().to_string();
                    let id = r.i64();
                    let parent_id = if k == "attachobj" {
                        Some(r.i64())
                    } else {
                        None
                    };
                    // [object]: x y z (z relative to terrain unless [absheight]).
                    // [attachObj]: a line that is always 0 in the stock maps, then the index
                    // of the parent's [new_attachment] point; the point is the position.
                    let mut attach_index = 0;
                    let mut instance = 0usize;
                    let pos = if k == "attachobj" {
                        instance = r.line().trim().parse().unwrap_or(0);
                        attach_index = r.i32().max(0) as usize;
                        [0.0; 3]
                    } else {
                        r.f64s::<3>()
                    };
                    // (Omsi.exe 0x792ee7: pitch and bank only from tile version 12 on, the
                    // strings from version 4 on - an older tile's string count read as a
                    // bank tilted the object and lost its strings)
                    let rot = if t.version >= 12 || t.version == 0 { r.f64s::<3>() } else { [r.f64(), 0.0, 0.0] };
                    let (flag, extra) = if t.version >= 4 || t.version == 0 { (r.i32(), read_extra(&mut r)) } else { (0, Vec::new()) };
                    let o = MapObject {
                        file: file_name,
                        id,
                        pos,
                        rot,
                        flag,
                        extra,
                        parent_id,
                        attach_index,
                        instance,
                        var_parent: None,
                        rules: Vec::new(),
                    };
                    if k == "attachobj" {
                        t.attach_objects.push(o);
                        last = Last::Attach;
                    } else {
                        t.objects.push(o);
                        last = Last::Object;
                    }
                }
                "spline" | "spline_h" => {
                    let _zero = r.line();
                    let file_name = r.str().to_string();
                    let id = r.i64();
                    let prev_id = r.i64();
                    let next_id = r.i64();
                    // Splines store x, height, y (verified by prev/next continuity of stock maps).
                    let x = r.f64();
                    let z = r.f64();
                    let y = r.f64();
                    let pos = [x, y, z];
                    let heading = r.f64();
                    let length = r.f64();
                    let radius = r.f64();
                    let grad_start = r.f64();
                    let grad_end = r.f64();
                    // [spline_h]: gradients, then the height change, then the rest as in [spline]
                    let is_h = k == "spline_h";
                    let delta_h = if is_h { Some(r.f64()) } else { None };
                    let cant_start = r.f64();
                    let cant_end = r.f64();
                    // Newer files have skew_start/skew_end; count remaining numeric lines.
                    let mut nums: Vec<f64> = Vec::new();
                    let mut mirror = false;
                    loop {
                        let save = r.pos();
                        let l = r.str();
                        let w = l.trim();
                        if w.is_empty()
                            || omsi_cfg::keyword_of(l).is_some()
                            || w.starts_with("Object Nr.")
                        {
                            r.seek(save);
                            break;
                        }
                        if w.eq_ignore_ascii_case("mirror") {
                            mirror = true;
                            break;
                        }
                        nums.push(omsi_cfg::parse_f64(w));
                    }
                    let (skew_start, skew_end, tex_offset) = match nums.len() {
                        0 => (0.0, 0.0, 0.0),
                        1 | 2 => (0.0, 0.0, nums[0]),
                        _ => (nums[0], nums[1], nums[2]),
                    };
                    t.splines.push(MapSpline {
                        file: file_name,
                        id,
                        prev_id,
                        next_id,
                        pos,
                        heading,
                        length,
                        radius,
                        grad_start,
                        grad_end,
                        delta_h,
                        cant_start,
                        cant_end,
                        skew_start,
                        skew_end,
                        tex_offset,
                        mirror,
                        is_h,
                        rules: Vec::new(),
                        terrain_align: None,
                        terrain_align_flag: false,
                        deleted: false,
                    });
                    last = Last::Spline;
                }
                "splineattachement" | "splineattachement_repeater" => {
                    let _zero = r.line();
                    let repeater = if k.ends_with("repeater") {
                        let tile = r.i64().max(0) as usize;
                        let first = r.i64().max(0) as usize;
                        Some((tile, first))
                    } else {
                        None
                    };
                    let file_name = r.str().to_string();
                    let id = r.i64();
                    let spline_index = r.i32();
                    let offset = r.f64s::<3>();
                    // (as for objects: pitch, bank and the tilt flag from version 12 on,
                    // strings from version 4 on - Omsi.exe 0x794892..0x794bd7)
                    let modern = t.version >= 12 || t.version == 0;
                    let rot = if modern { r.f64s::<3>() } else { [r.f64(), 0.0, 0.0] };
                    let interval = r.f64();
                    let range = r.f64();
                    let tilt = if modern { r.i32() != 0 } else { false };
                    let (count, mut strings) = if t.version >= 4 || t.version == 0 { (r.i32().max(0) as usize, read_extra(&mut r)) } else { (0, Vec::new()) };
                    strings.truncate(count);
                    t.spline_attachments.push(SplineAttachment {
                        file: file_name,
                        id,
                        spline_index,
                        offset,
                        rot,
                        interval,
                        range,
                        tilt,
                        strings,
                        repeater,
                        var_parent: None,
                        rules: Vec::new(),
                    });
                    last = Last::SplineAttach;
                }
                "varparent" => {
                    let id = r.i64();
                    match last {
                        Last::Object => {
                            if let Some(o) = t.objects.last_mut() {
                                o.var_parent = Some(id);
                            }
                        }
                        Last::Attach => {
                            if let Some(o) = t.attach_objects.last_mut() {
                                o.var_parent = Some(id);
                            }
                        }
                        Last::SplineAttach => {
                            if let Some(o) = t.spline_attachments.last_mut() {
                                o.var_parent = Some(id);
                            }
                        }
                        _ => {}
                    }
                }
                "spline_terrain_align" => {
                    if let Some(s) = t.splines.last_mut() {
                        s.terrain_align_flag = true;
                    }
                }
                "spline_terrain_align_2" => {
                    let v = r.f64();
                    if let Some(s) = t.splines.last_mut() {
                        s.terrain_align = Some(v);
                    }
                }
                "rule" | "kill_rule" => {
                    // A rule belongs to whatever came before it. Most of a map's rules follow
                    // an `[object]` - a junction - not a spline; giving them all to the last
                    // spline put speed limits and no_cars on roads that never had them and
                    // left the junctions unrestricted.
                    let rule = read_rule(&mut r, k == "kill_rule");
                    match last {
                        Last::Chrono => {
                            if let Some(c) = t.chrono_changes.last_mut() {
                                c.rules.push(rule);
                            }
                        }
                        Last::Object => {
                            if let Some(o) = t.objects.last_mut() {
                                o.rules.push(rule);
                            }
                        }
                        Last::Attach => {
                            if let Some(o) = t.attach_objects.last_mut() {
                                o.rules.push(rule);
                            }
                        }
                        Last::SplineAttach => {
                            if let Some(o) = t.spline_attachments.last_mut() {
                                o.rules.push(rule);
                            }
                        }
                        _ => {
                            if let Some(s) = t.splines.last_mut() {
                                s.rules.push(rule);
                            }
                        }
                    }
                }
                "selobject" | "selspline" => {
                    let id = r.i64();
                    t.chrono_changes.push(ChronoChange {
                        is_spline: k == "selspline",
                        id,
                        ..Default::default()
                    });
                    last = Last::Chrono;
                }
                "delete" => {
                    if let Some(c) = t.chrono_changes.last_mut() {
                        c.delete = true;
                    }
                }
                "typ" => {
                    let s = r.str().to_string();
                    if let Some(c) = t.chrono_changes.last_mut() {
                        c.new_type = Some(s);
                    }
                }
                "relabel" => {
                    // the object's strings anew: a count, then that many lines
                    let n = r.i64().clamp(0, 256) as usize;
                    let strings: Vec<String> = (0..n).map(|_| r.str().to_string()).collect();
                    if let Some(c) = t.chrono_changes.last_mut() {
                        c.relabel = Some(strings);
                    }
                }
                _ => t.unknown_keywords.push((k, r.block_line())),
            }
        }
        t
    }

    /// Apply a chrono patch tile. The patch's own splines, objects and attachments are added
    /// first (its spline attachments count their spline in the combined list), then the
    /// `[selobject]` / `[selspline]` changes are applied to whatever carries that id -
    /// objects added by an earlier chrono folder included. Returns how many selections named
    /// nothing in this tile.
    pub fn apply_chrono(&mut self, patch: &Tile) -> usize {
        self.splines.extend(patch.splines.iter().cloned());
        self.objects.extend(patch.objects.iter().cloned());
        self.attach_objects
            .extend(patch.attach_objects.iter().cloned());
        self.spline_attachments
            .extend(patch.spline_attachments.iter().cloned());
        let mut unmatched = 0;
        let mut deleted: std::collections::HashSet<i64> = std::collections::HashSet::new();
        for c in &patch.chrono_changes {
            if c.is_spline {
                let mut hit = false;
                for s in self
                    .splines
                    .iter_mut()
                    .filter(|s| s.id == c.id && !s.deleted)
                {
                    hit = true;
                    if let Some(t) = &c.new_type {
                        s.file = t.clone();
                    }
                    apply_rules(&mut s.rules, &c.rules);
                    s.deleted |= c.delete;
                }
                unmatched += !hit as usize;
                continue;
            }
            let mut hit = false;
            for o in self
                .objects
                .iter_mut()
                .chain(self.attach_objects.iter_mut())
                .filter(|o| o.id == c.id)
            {
                hit = true;
                if let Some(t) = &c.new_type {
                    o.file = t.clone();
                }
                if let Some(strings) = &c.relabel {
                    o.extra = strings.clone();
                    o.flag = strings.len() as i32;
                }
                apply_rules(&mut o.rules, &c.rules);
            }
            for a in self.spline_attachments.iter_mut().filter(|a| a.id == c.id) {
                hit = true;
                if let Some(t) = &c.new_type {
                    a.file = t.clone();
                }
                if let Some(strings) = &c.relabel {
                    a.strings = strings.clone();
                }
                apply_rules(&mut a.rules, &c.rules);
            }
            if c.delete {
                deleted.insert(c.id);
            }
            unmatched += !hit as usize;
        }
        if !deleted.is_empty() {
            self.objects.retain(|o| !deleted.contains(&o.id));
            self.attach_objects.retain(|o| !deleted.contains(&o.id));
            self.spline_attachments.retain(|a| !deleted.contains(&a.id));
        }
        unmatched
    }

    /// Tile coordinates from a file name like `tile_-1_12.map`.
    pub fn coords_from_name(name: &str) -> Option<(i32, i32)> {
        let stem = name.strip_suffix(".map")?.strip_prefix("tile_")?;
        let (x, y) = stem.rsplit_once('_')?;
        Some((x.parse().ok()?, y.parse().ok()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(text: &str) -> Tile {
        Tile::parse(&CfgFile::from_str("tile_0_0.map", text))
    }

    const BASE: &str = "[version]\n14\n\n\
[spline]\n0\nSplines\\road.sli\n100\n0\n101\n10\n0\n20\n0\n50\n0\n0\n0\n0\n0\n0\n0\n0\n\n\
[spline]\n0\nSplines\\road.sli\n101\n100\n0\n10\n0\n70\n0\n30\n0\n0\n0\n0\n0\n0\n0\n0\n\n\
Object Nr. 0\n[object]\n0\nSceneryobjects\\pole.sco\n7\n5\n6\n0.25\n90\n0\n0\n0\n\n\
Object Nr. 1\n[attachObj]\n0\nSceneryobjects\\plate.sco\n8\n7\n0\n3\n180\n0\n0\n1\n76\n\n\
[varparent]\n7\n\n\
Object Nr. 2\n[splineAttachement]\n0\nSceneryobjects\\lamp.sco\n9\n1\n-6.5\n0.25\n4\n180\n0\n0\n30\n400\n1\n2\nA\nB\n\n\
Object Nr. 3\n[splineAttachement_repeater]\n0\n12\n5\nSceneryobjects\\lamp.sco\n9\n0\n-6.5\n0.25\n4\n180\n0\n0\n30\n400\n0\n0\n";

    #[test]
    fn spline_h_fields() {
        let t = tile("[spline_h]\n0\nSplines\\ramp.sli\n5\n4\n6\n168.16\n-5.79\n125.08\n60\n73.86\n0\n6.19\n0\n5.85\n1.5\n0\n0\n0\n11.14\n\n[spline]\n0\nSplines\\road.sli\n6\n5\n0\n1\n0.06\n2\n60\n30\n0\n0\n0\n2\n0\n0\n0\n30\n");
        let h = &t.splines[0];
        assert!(h.is_h);
        assert_eq!(
            (
                h.grad_start,
                h.grad_end,
                h.delta_h,
                h.cant_start,
                h.tex_offset
            ),
            (6.19, 0.0, Some(5.85), 1.5, 11.14)
        );
        let s = &t.splines[1];
        assert_eq!((s.delta_h, s.cant_start, s.tex_offset), (None, 2.0, 30.0));
    }

    /// Before tile version 12 an object has only its heading; the next line is already the
    /// count of its strings (Omsi.exe 0x792ee7).
    #[test]
    fn old_tile_objects_have_only_a_heading() {
        let t = tile("[version]\n11\n\n[object]\n0\nSceneryobjects\\x.sco\n12\n10\n20\n0.5\n90\n2\nfirst\nsecond\n\n[object]\n0\nSceneryobjects\\y.sco\n13\n1\n2\n0\n45\n0\n");
        assert_eq!(t.objects.len(), 2);
        let o = &t.objects[0];
        assert_eq!((o.pos, o.rot, o.flag), ([10.0, 20.0, 0.5], [90.0, 0.0, 0.0], 2));
        assert_eq!(o.extra, vec!["first".to_string(), "second".to_string()]);
        assert_eq!((t.objects[1].rot, t.objects[1].flag), ([45.0, 0.0, 0.0], 0));
    }

    #[test]
    fn records() {
        let t = tile(BASE);
        assert_eq!(t.splines.len(), 2);
        assert_eq!(t.objects.len(), 1);
        let a = &t.attach_objects[0];
        assert_eq!(
            (a.parent_id, a.attach_index, a.rot[0], a.flag),
            (Some(7), 3, 180.0, 1)
        );
        assert_eq!(a.extra, vec!["76".to_string()]);
        assert_eq!(a.var_parent, Some(7));
        let s = &t.spline_attachments[0];
        assert_eq!(
            (s.spline_index, s.offset, s.interval, s.range, s.tilt),
            (1, [-6.5, 0.25, 4.0], 30.0, 400.0, true)
        );
        assert_eq!(s.strings, vec!["A".to_string(), "B".to_string()]);
        assert_eq!(s.repeater, None);
        assert_eq!(t.spline_attachments[1].repeater, Some((12, 5)));
        assert!(t.spline_attachments[1].strings.is_empty());
    }

    #[test]
    fn chrono_by_id() {
        let mut t = tile(BASE);
        let patch = tile(
            "[selobject]\n8\n\n[relabel]\n1\n34N\n\n[typ]\nSceneryobjects\\plate_green.sco\n\n\
[selspline]\n100\n\n[delete]\n\n\
[selobject]\n9\n\n[kill_rule]\n0\ntrafficdensity\n1.000\n0\n\n\
[selobject]\n7\n\n[rule]\n2\nspeedlimit\n30\n0\n\n\
[selobject]\n424242\n\n[delete]\n\n\
[spline]\n0\nSplines\\new.sli\n200\n0\n0\n10\n0\n100\n0\n10\n0\n0\n0\n0\n0\n0\n0\n0\n\n\
[splineAttachement]\n0\nSceneryobjects\\bay.sco\n300\n2\n3\n0\n0\n0\n0\n0\n7\n0\n0\n0\n",
        );
        assert_eq!(t.apply_chrono(&patch), 1);
        assert_eq!(t.attach_objects[0].file, "Sceneryobjects\\plate_green.sco");
        assert_eq!(t.attach_objects[0].extra, vec!["34N".to_string()]);
        // spline 100 is deleted but keeps its place: attachments count splines by index
        assert_eq!(t.splines.len(), 3);
        assert!(t.splines[0].deleted && !t.splines[1].deleted);
        assert_eq!(t.splines[2].id, 200);
        assert_eq!(t.spline_attachments.last().unwrap().spline_index, 2);
        assert_eq!(t.objects[0].rules.len(), 1);
        assert_eq!(t.objects[0].rules[0].kind, "speedlimit");
    }
}
