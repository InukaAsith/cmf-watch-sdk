// Emulator frame vs simulator frame: cargo run --release --example compare -- OUT.png LABEL EMU.png SIM.png ...
// One row per pair: emulator | simulator | difference after scaling the simulator by the fitted gain (the emulator
// capture scales white to ~0.627). Prints the gain and how many pixels still differ.
use std::fs::File;
use wp2_core::gfx::{Canvas, H, W};

fn load(path: &str) -> Vec<[f32; 3]> {
    let mut d = png::Decoder::new(std::io::BufReader::new(File::open(path).expect(path)));
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = d.read_info().expect(path);
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).expect(path);
    assert_eq!((info.width as usize, info.height as usize), (W, H), "{path}");
    let n = info.color_type.samples();
    buf.chunks(n).map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect()
}

fn label(row: &mut [[u8; 3]], text: &str) {
    let mut px = vec![0u16; W * H];
    Canvas { px: &mut px }.text(4, 2, text.as_bytes(), 0xFFFF);
    for (i, p) in px[..W * 20].iter().enumerate() {
        if *p != 0 {
            row[i] = [255; 3];
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = &args[0];
    let rows = args[1..].chunks(3).collect::<Vec<_>>();
    let (cw, rh) = (3 * W, H + 20);
    let mut img = vec![[24u8; 3]; cw * rh * rows.len()];
    for (r, pair) in rows.iter().enumerate() {
        let (emu, sim) = (load(&pair[1]), load(&pair[2]));
        // least squares gain over lit simulator pixels
        let (mut num, mut den) = (0.0f64, 0.0f64);
        for (e, s) in emu.iter().zip(&sim) {
            for c in 0..3 {
                num += f64::from(e[c] * s[c]);
                den += f64::from(s[c] * s[c]);
            }
        }
        let k = if den > 0.0 { (num / den) as f32 } else { 1.0 };
        let (mut off, mut lit_e, mut lit_s, mut both) = (0, 0, 0, 0);
        let base = r * rh * cw;
        for i in 0..W * H {
            let (e, s) = (emu[i], sim[i]);
            let d = (0..3).map(|c| (e[c] - k * s[c]).abs()).fold(0.0, f32::max);
            off += (d > 24.0) as usize;
            let (le, ls) = (e.iter().sum::<f32>() > 60.0, s.iter().sum::<f32>() > 60.0);
            lit_e += le as usize;
            lit_s += ls as usize;
            both += (le && ls) as usize;
            let (x, y) = (i % W, i / W);
            let at = base + (y + 20) * cw;
            img[at + x] = e.map(|v| v as u8);
            img[at + W + x] = s.map(|v| v as u8);
            img[at + 2 * W + x] = if d > 24.0 { [255, 0, 255] } else { [(e[0] / 3.0) as u8; 3] };
        }
        let iou = both as f32 / (lit_e + lit_s - both).max(1) as f32;
        let line = format!("{}: gain {k:.3}, {off} px differ >24 after gain, lit-pixel IoU {iou:.3}", pair[0]);
        println!("{line}");
        let mut head = vec![[24u8; 3]; W * 20];
        label(&mut head, &format!("{} x{k:.2} IoU{iou:.2}", pair[0]));
        for y in 0..20 {
            img[base + y * cw..base + y * cw + W].copy_from_slice(&head[y * W..(y + 1) * W]);
        }
    }
    let mut enc = png::Encoder::new(std::io::BufWriter::new(File::create(out).unwrap()), cw as u32, (rh * rows.len()) as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.write_header().unwrap().write_image_data(img.as_flattened()).unwrap();
}
