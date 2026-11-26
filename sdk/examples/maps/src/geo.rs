//! Offline Maps' geometry, no floats: the watch's GPS in the pack's frame (metres east and north of its centre, as
//! the phone's AtlasScene frame), distances, a track recorded as you go, and a path followed (a route from the
//! phone, or the track backwards to its start).

/// mm a degree of latitude (AtlasScene.xy: 110 540 m).
pub const MM_PER_DEG_LAT: i64 = 110_540_000;

/// The pack's frame: its centre and how many mm a degree of longitude is there (cos(lat) × 111 320 m).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub lat0_e7: i32,
    pub lon0_e7: i32,
    pub mm_per_deg_lon: u32,
}

impl Frame {
    /// A GPS fix (1e-7 degrees) as frame metres.
    pub fn xy(&self, lat_e7: i32, lon_e7: i32) -> (i32, i32) {
        let dx = i64::from(lon_e7) - i64::from(self.lon0_e7);
        let dy = i64::from(lat_e7) - i64::from(self.lat0_e7);
        let x = dx * i64::from(self.mm_per_deg_lon) / 10_000_000_000;
        let y = dy * MM_PER_DEG_LAT / 10_000_000_000;
        (x.clamp(-1 << 20, 1 << 20) as i32, y.clamp(-1 << 20, 1 << 20) as i32)
    }

    /// Frame metres as 1e-7 degrees (a marker dropped on the map).
    pub fn lat_lon(&self, p: (i32, i32)) -> (i32, i32) {
        let lat = i64::from(self.lat0_e7) + i64::from(p.1) * 10_000_000_000 / MM_PER_DEG_LAT;
        let lon = i64::from(self.lon0_e7) + i64::from(p.0) * 10_000_000_000 / i64::from(self.mm_per_deg_lon.max(1));
        (lat as i32, lon as i32)
    }
}

pub fn isqrt(v: i64) -> i64 {
    if v <= 0 {
        return 0;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

/// Metres between two frame points.
pub fn dist(a: (i32, i32), b: (i32, i32)) -> i32 {
    let (dx, dy) = (i64::from(b.0 - a.0), i64::from(b.1 - a.1));
    isqrt(dx * dx + dy * dy) as i32
}

/// The track: a point each time you moved `gap` metres; when it is full every other point goes and the gap
/// doubles, so a long walk still fits (coarser). Kept in RAM only: gone when the app closes.
pub struct Track<const N: usize> {
    pub pts: [(i16, i16); N],
    pub n: usize,
    pub gap: i32,
    /// metres walked
    pub metres: i32,
}

pub const TRACK_GAP: i32 = 6;

impl<const N: usize> Track<N> {
    pub fn clear(&mut self) {
        self.n = 0;
        self.gap = TRACK_GAP;
        self.metres = 0;
    }

    pub fn last(&self) -> Option<(i32, i32)> {
        (self.n > 0).then(|| p32(self.pts[self.n - 1]))
    }

    /// A fix while recording; true when it became a point.
    pub fn add(&mut self, p: (i32, i32)) -> bool {
        if let Some(l) = self.last() {
            let d = dist(l, p);
            if d < self.gap {
                return false;
            }
            self.metres += d;
        }
        if self.n == N {
            // every other point (the newest kept)
            let mut k = 0;
            for i in ((N - 1) % 2..N).step_by(2) {
                self.pts[k] = self.pts[i];
                k += 1;
            }
            self.n = k;
            self.gap *= 2;
        }
        self.pts[self.n] = (p.0.clamp(-32767, 32767) as i16, p.1.clamp(-32767, 32767) as i16);
        self.n += 1;
        true
    }
}

pub fn p32(p: (i16, i16)) -> (i32, i32) {
    (i32::from(p.0), i32::from(p.1))
}

/// Where you are on a path: the segment you are on (from its start point), how far off it, how far there is to
/// go to its end, and the point to head for (`AHEAD` metres along).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnPath {
    pub seg: usize,
    pub off: i32,
    pub left: i32,
    pub head: (i32, i32),
}

/// The point to head for, this far along the path from where you are on it.
pub const AHEAD: i32 = 25;
/// Closer than this to the end: arrived.
pub const ARRIVED: i32 = 15;
/// Farther than this from the path: off it (said on screen).
pub const OFF: i32 = 35;

/// You at `p` on `path`, searching from segment `from` on (you go forward), the whole path when you are far off.
pub fn follow(path: &[(i16, i16)], p: (i32, i32), from: usize) -> Option<OnPath> {
    if path.is_empty() {
        return None;
    }
    if path.len() == 1 {
        let d = dist(p, p32(path[0]));
        return Some(OnPath { seg: 0, off: d, left: d, head: p32(path[0]) });
    }
    let nearest = |lo: usize, hi: usize| {
        let mut best = (i64::MAX, lo, (0, 0), 0i64);
        for i in lo..hi {
            let (a, b) = (p32(path[i]), p32(path[i + 1]));
            let (q, t) = project(a, b, p);
            let d = sq(p, q);
            if d < best.0 {
                best = (d, i, q, t);
            }
        }
        best
    };
    let last = path.len() - 1;
    let from = from.min(last - 1);
    // the next 40 segments first; the whole path if that is far off (a short cut, a wrong turn)
    let mut best = nearest(from, (from + 40).min(last));
    if isqrt(best.0) as i32 > OFF {
        let all = nearest(0, last);
        if all.0 < best.0 {
            best = all;
        }
    }
    let (d2, seg, q, _) = best;
    // to go: the rest of this segment, then the others
    let mut left = dist(q, p32(path[seg + 1]));
    for i in seg + 1..last {
        left += dist(p32(path[i]), p32(path[i + 1]));
    }
    // AHEAD metres on from q
    let mut head = p32(path[last]);
    let mut need = AHEAD;
    let mut at = q;
    for i in seg + 1..=last {
        let nx = p32(path[i]);
        let d = dist(at, nx);
        if d >= need {
            head = (at.0 + (nx.0 - at.0) * need / d.max(1), at.1 + (nx.1 - at.1) * need / d.max(1));
            break;
        }
        need -= d;
        at = nx;
    }
    Some(OnPath { seg, off: isqrt(d2) as i32, left, head })
}

fn sq(a: (i32, i32), b: (i32, i32)) -> i64 {
    let (dx, dy) = (i64::from(b.0 - a.0), i64::from(b.1 - a.1));
    dx * dx + dy * dy
}

/// The point of a→b nearest p, and where on it (0…1024).
fn project(a: (i32, i32), b: (i32, i32), p: (i32, i32)) -> ((i32, i32), i64) {
    let (dx, dy) = (i64::from(b.0 - a.0), i64::from(b.1 - a.1));
    let l2 = dx * dx + dy * dy;
    if l2 == 0 {
        return (a, 0);
    }
    let dot = (i64::from(p.0 - a.0) * dx + i64::from(p.1 - a.1) * dy).clamp(0, l2);
    ((a.0 + (dx * dot / l2) as i32, a.1 + (dy * dot / l2) as i32), dot * 1024 / l2)
}

/// Corners to keep so the path stays within `tol` metres of what it was (Douglas–Peucker): `keep[i]` set for
/// them, the ends always. A walked track's GPS wobble goes; its real turns stay.
pub fn corners(path: &[(i16, i16)], tol: i32, keep: &mut [bool]) {
    let n = path.len().min(keep.len());
    keep[..n].fill(false);
    if n == 0 {
        return;
    }
    keep[0] = true;
    keep[n - 1] = true;
    // spans still to look at, without recursion
    let mut todo = [(0u16, 0u16); 64];
    let mut k = 0;
    if n > 2 {
        todo[0] = (0, (n - 1) as u16);
        k = 1;
    }
    let t2 = i64::from(tol) * i64::from(tol);
    while k > 0 {
        k -= 1;
        let (a, b) = (usize::from(todo[k].0), usize::from(todo[k].1));
        let (pa, pb) = (p32(path[a]), p32(path[b]));
        let mut far = (0i64, 0usize);
        for i in a + 1..b {
            let q = p32(path[i]);
            let d = sq(project(pa, pb, q).0, q);
            if d > far.0 {
                far = (d, i);
            }
        }
        if far.0 > t2 {
            keep[far.1] = true;
            for span in [(a, far.1), (far.1, b)] {
                if span.1 - span.0 > 1 && k < todo.len() {
                    todo[k] = (span.0 as u16, span.1 as u16);
                    k += 1;
                }
            }
        }
    }
}

/// The turn from heading `a` to heading `b` (degrees) as nav's `dir` (1 slight left, 2 left, 3 sharp left, 4 slight
/// right, 5 right, 6 sharp right); None under 30°: straight on.
pub fn turn_dir(a: u16, b: u16) -> Option<u8> {
    let t = (i32::from(b) - i32::from(a) + 540) % 360 - 180;
    let size = match t.abs() {
        0..30 => return None,
        30..60 => 1,
        60..135 => 2,
        _ => 3,
    };
    Some(if t < 0 { size } else { size + 3 })
}

/// Compass direction of b from a, degrees (0 north, 90 east).
pub fn bearing(a: (i32, i32), b: (i32, i32)) -> u16 {
    let a16 = cmf_wp2_ui::motion::angle(b.0 - a.0, a.1 - b.1);
    // motion::angle: 0 = +x (east), turns clockwise on screen (y down): north (screen up) is 3/4 of a turn
    ((u32::from(a16) * 360 / 65536 + 90) % 360) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_keep_turns_and_drop_wobble() {
        // east 60 m with ±2 m wobble, then north 60 m: one corner at (60, 0)
        let mut p = [(0i16, 0i16); 21];
        for i in 0..=10 {
            p[i] = ((i * 6) as i16, if i % 2 == 1 { 2 } else { -1 });
        }
        for i in 1..=10 {
            p[10 + i] = (60, (i * 6) as i16);
        }
        let mut keep = [false; 21];
        corners(&p, 8, &mut keep);
        let kept: Vec<usize> = (0..21).filter(|&i| keep[i]).collect();
        assert_eq!(kept, [0, 10, 20]);
        assert_eq!(turn_dir(bearing(p32(p[0]), p32(p[10])), bearing(p32(p[10]), p32(p[20]))), Some(2), "east then north: left");
    }

    #[test]
    fn turn_sizes() {
        assert_eq!(turn_dir(0, 20), None);
        assert_eq!(turn_dir(350, 10), None);
        assert_eq!(turn_dir(0, 45), Some(4));
        assert_eq!(turn_dir(0, 90), Some(5));
        assert_eq!(turn_dir(0, 150), Some(6));
        assert_eq!(turn_dir(90, 0), Some(2));
        assert_eq!(turn_dir(10, 330), Some(1));
    }

    const F: Frame = Frame { lat0_e7: 69_271_000, lon0_e7: 798_612_000, mm_per_deg_lon: 110_511_000 };

    #[test]
    fn gps_to_frame_and_back() {
        // 0.001° north is 110.5 m, 0.001° east 110.5 m here (cos 6.9° ≈ 0.9927)
        assert_eq!(F.xy(69_281_000, 798_612_000), (0, 110));
        assert_eq!(F.xy(69_271_000, 798_622_000), (110, 0));
        assert_eq!(F.xy(69_271_000, 798_612_000), (0, 0));
        let (lat, lon) = F.lat_lon((1000, -2000));
        let (x, y) = F.xy(lat, lon);
        assert!((x - 1000).abs() <= 1 && (y + 2000).abs() <= 1);
    }

    #[test]
    fn track_keeps_a_point_each_gap_and_thins_when_full() {
        let mut t = Track::<8> { pts: [(0, 0); 8], n: 0, gap: TRACK_GAP, metres: 0 };
        t.clear();
        assert!(t.add((0, 0)));
        assert!(!t.add((3, 0)));
        assert!(t.add((10, 0)));
        assert_eq!(t.metres, 10);
        for k in 2..8 {
            assert!(t.add((10 * k, 0)));
        }
        assert_eq!(t.n, 8);
        // full: every other point, the newest kept, the gap doubled
        assert!(t.add((100, 0)));
        assert_eq!(t.gap, 2 * TRACK_GAP);
        assert_eq!(t.n, 5);
        assert_eq!(p32(t.pts[t.n - 1]), (100, 0));
        assert_eq!(p32(t.pts[t.n - 2]), (70, 0));
        assert!(!t.add((105, 0)));
    }

    #[test]
    fn following_a_path() {
        // east 100 m, then north 100 m
        let path = [(0, 0), (100, 0), (100, 100)];
        let o = follow(&path, (50, 5), 0).unwrap();
        assert_eq!((o.seg, o.off, o.left), (0, 5, 150));
        assert_eq!(o.head, (75, 0));
        // round the corner: the head goes north
        let o = follow(&path, (90, 0), 0).unwrap();
        assert_eq!(o.head.0, 100);
        assert!((14..=15).contains(&o.head.1));
        let o = follow(&path, (100, 95), 1).unwrap();
        assert_eq!((o.seg, o.left), (1, 5));
        assert!(o.left < ARRIVED);
        // far off: the whole path searched
        let o = follow(&path, (300, 50), 0).unwrap();
        assert_eq!((o.seg, o.off), (1, 200));
    }

    #[test]
    fn bearings() {
        assert_eq!(bearing((0, 0), (0, 100)), 0);
        assert!((89..=91).contains(&bearing((0, 0), (100, 0))));
        assert!((179..=181).contains(&bearing((0, 0), (0, -100))));
        assert!((269..=271).contains(&bearing((0, 0), (-100, 0))));
    }
}
