// Map images from Apple Maps (MKMapSnapshotter): one line per image on stdin, "z x y w h out.png" (a block of w×h
// web-mercator tiles from x, y); each comes out (256·w)×(256·h) px drawn at 2×, so labels are twice the size of a
// desktop tile and sharp. One image per zoom level: labels are placed once, never cut at tile edges.
// Used by hosts/mapbuild.py --style hd. The phone app does the same on iOS.
import AppKit
import MapKit

func snap(z: Int, x: Int, y: Int, w: Int, h: Int, out: String) -> Bool {
    let side = 256.0 * pow(2.0, Double(20 - z))  // MKMapRect's world is 256·2²⁰ points
    let o = MKMapSnapshotter.Options()
    o.mapRect = MKMapRect(x: Double(x) * side, y: Double(y) * side, width: side * Double(w), height: side * Double(h))
    let pt = NSSize(width: 128 * w, height: 128 * h)
    o.size = pt
    o.pointOfInterestFilter = .includingAll
    o.appearance = NSAppearance(named: .aqua)
    var img: NSImage?
    let done = DispatchSemaphore(value: 0)
    MKMapSnapshotter(options: o).start(with: .global()) { s, e in
        if let e { FileHandle.standardError.write("\(e)\n".data(using: .utf8)!) }
        img = s?.image
        done.signal()
    }
    while done.wait(timeout: .now()) == .timedOut { RunLoop.current.run(until: Date().addingTimeInterval(0.05)) }
    guard let img else { return false }
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 256 * w, pixelsHigh: 256 * h, bitsPerSample: 8,
                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                               bytesPerRow: 0, bitsPerPixel: 0)!
    rep.size = pt
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    img.draw(in: NSRect(origin: .zero, size: pt))
    NSGraphicsContext.restoreGraphicsState()
    try? FileManager.default.createDirectory(atPath: (out as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
    return (try? rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: out))) != nil
}

while let line = readLine() {
    let p = line.split(separator: " ")
    guard p.count == 6, let z = Int(p[0]), let x = Int(p[1]), let y = Int(p[2]), let w = Int(p[3]), let h = Int(p[4])
    else { continue }
    print(snap(z: z, x: x, y: y, w: w, h: h, out: String(p[5])) ? "ok" : "fail")
    fflush(stdout)
}
