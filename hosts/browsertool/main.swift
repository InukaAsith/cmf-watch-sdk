// The phone's Browser work on a Mac, for testing the watch app without the phone (hosts/browserhost.py --web):
// HubKit's Reader and picture code (cmfx ios/HubKit, compiled in) and a WebKit view for page view and Google.
// One command a line on stdin, one JSON answer a line on stdout:
//   reader URL | google URL | page URL ("-" the current) | scroll DY | tap X Y | type TEXT | back | playing | frame
// Answers: {"msgs": [hex…], "links": [url…]} (watch messages), {"fallback": url}, {"field": true}, {"none": true}.
import AppKit
import Foundation
import WebKit

@MainActor final class Tool: NSObject, WKNavigationDelegate, NSWindowDelegate {
    static let side: CGFloat = 360
    let window: NSWindow
    let web: WKWebView
    var waiters: [CheckedContinuation<Void, Never>] = []
    /// the navigation under way has its new document (its text may still be coming)
    var committed = true
    /// how the last page went to the watch (debugging)
    var lastMode = ""

    override init() {
        let c = WKWebViewConfiguration()
        c.websiteDataStore = .default()
        c.mediaTypesRequiringUserActionForPlayback = []
        web = WKWebView(frame: NSRect(x: 0, y: 0, width: Self.side, height: Self.side), configuration: c)
        web.customUserAgent = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1"
        // on screen (WebKit holds media back in a window off it), all but invisible, clicks go through
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: Self.side, height: Self.side), styleMask: .borderless,
                          backing: .buffered, defer: false)
        super.init()
        web.navigationDelegate = self
        window.contentView = web
        // the close button hides it again (closing would free the window the watch's pages are drawn in)
        window.isReleasedWhenClosed = false
        window.delegate = self
        window.alphaValue = 0.02
        window.ignoresMouseEvents = true
        window.level = .floating
        window.orderFrontRegardless()
    }

    /// Shown as an ordinary window (SIGUSR1) to sign in by hand: its cookies stay in this browser's store and the
    /// watch's pages use them. SIGUSR2 hides it again.
    func show(_ on: Bool) {
        if on {
            NSApp.setActivationPolicy(.regular)
            window.styleMask = [.titled, .closable, .resizable, .miniaturizable]
            window.title = "Watch browser: sign in here"
            window.alphaValue = 1
            window.ignoresMouseEvents = false
            window.level = .normal
            window.setContentSize(NSSize(width: 420, height: 760))
            web.frame = NSRect(x: 0, y: 0, width: 420, height: 760)
            window.center()
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            // nothing open yet: Google's sign-in (YouTube's too), from where any site can be reached
            if web.url == nil {
                web.load(URLRequest(url: URL(string: "https://accounts.google.com/ServiceLogin?continue=https://m.youtube.com/")!))
            }
        } else {
            window.styleMask = .borderless
            window.alphaValue = 0.02
            window.ignoresMouseEvents = true
            window.level = .floating
            window.setContentSize(NSSize(width: Self.side, height: Self.side))
            window.setFrameOrigin(.zero)
            web.frame = NSRect(x: 0, y: 0, width: Self.side, height: Self.side)
            NSApp.setActivationPolicy(.prohibited)
        }
    }

    nonisolated func windowShouldClose(_ w: NSWindow) -> Bool {
        MainActor.assumeIsolated { show(false) }
        return false
    }

    func js(_ s: String) async -> String? {
        await withCheckedContinuation { (c: CheckedContinuation<String?, Never>) in
            web.evaluateJavaScript(s) { r, _ in c.resume(returning: r.map { "\($0)" }) }
        }
    }

    func load(_ u: URL) async {
        committed = false
        web.load(URLRequest(url: u))
        await waitLoad()
    }

    /// Till the page's text is in (its document parsed), not every image and ad: a picture in about a second.
    func waitLoad() async {
        try? await Task.sleep(for: .milliseconds(150))
        guard web.isLoading || !committed else { return }
        // the new document, not the one before it
        for _ in 0..<100 where !committed {
            try? await Task.sleep(for: .milliseconds(100))
        }
        for _ in 0..<60 where await js("document.readyState") == "loading" {
            try? await Task.sleep(for: .milliseconds(100))
        }
        try? await Task.sleep(for: .milliseconds(400))
    }

    nonisolated func webView(_ v: WKWebView, didStartProvisionalNavigation n: WKNavigation!) {
        Task { @MainActor in self.committed = false }
    }

    nonisolated func webView(_ v: WKWebView, didCommit n: WKNavigation!) {
        Task { @MainActor in self.committed = true }
    }

    func done() {
        let w = waiters
        waiters = []
        for c in w { c.resume() }
    }

    nonisolated func webView(_ v: WKWebView, didFinish n: WKNavigation!) { Task { @MainActor in self.done() } }
    nonisolated func webView(_ v: WKWebView, didFail n: WKNavigation!, withError e: Error) { Task { @MainActor in self.done() } }
    nonisolated func webView(_ v: WKWebView, didFailProvisionalNavigation n: WKNavigation!, withError e: Error) {
        Task { @MainActor in self.committed = true; self.done() }
    }

    func reader(_ u: URL) async -> [String: Any] {
        var r = URLRequest(url: u)
        r.setValue(web.customUserAgent, forHTTPHeaderField: "User-Agent")
        r.timeoutInterval = 20
        guard let (d, resp) = try? await URLSession.shared.data(for: r) else { return ["msgs": hex([BrowserProto.note("Couldn't load \(u.host ?? "it")")])] }
        let final = resp.url ?? u
        var p = Reader.parse(String(decoding: d, as: UTF8.self), base: final)
        if p.title.isEmpty { p.title = final.host ?? "Page" }
        if p.blocks.isEmpty { return ["empty": true, "url": final.absoluteString] }
        return ["msgs": hex(Reader.messages(p)), "links": p.links.map(\.absoluteString), "url": final.absoluteString]
    }

    func google(_ u: URL) async -> [String: Any] {
        await load(u)
        let script = """
        (() => { const out = [];
          document.querySelectorAll('a h3, a [role=heading]').forEach(h => { const a = h.closest('a'); if (!a || !a.href) return;
            let s = ''; const box = a.closest('div.MjjYud, div[data-hveid], div.g');
            if (box) { const sn = box.querySelector('div[data-sncf], div.VwiC3b, div[style*="-webkit-line-clamp"]');
              s = sn ? sn.innerText : box.innerText.replace(a.innerText, '').split('\\n').map(x => x.trim()).filter(x => x.length > 40).join(' ').slice(0, 300); }
            out.push([h.innerText, a.href, s]); });
          return JSON.stringify(out); })()
        """
        let rows = (await js(script)).flatMap { try? JSONSerialization.jsonObject(with: Data($0.utf8)) as? [[String]] } ?? []
        let q = URLComponents(url: u, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "q" }?.value ?? ""
        let p = BrowserProto.results(rows, query: q)
        if p.blocks.isEmpty, let f = BrowserProto.fallback(for: u) { return ["fallback": f.absoluteString] }
        return ["msgs": hex(Reader.messages(p)), "links": p.links.map(\.absoluteString)]
    }

    /// The whole page (as much as the watch keeps): laid out tall, one picture, its tappable places in page px.
    func page() async -> [String: Any] {
        let full = Double(await js(BrowserProto.pageHeight) ?? "") ?? Double(Self.side)
        let tall = min(max(full, Double(Self.side)), Double(Self.side) * 12)
        window.setContentSize(NSSize(width: Self.side, height: tall))
        web.frame = NSRect(x: 0, y: 0, width: Self.side, height: tall)
        _ = await js("window.scrollTo(0, 0)")
        try? await Task.sleep(for: .milliseconds(150))
        // its images in (lazy ones told to load: all of the tall page is in view), 1.5 s at most
        for _ in 0..<15 where (Int(await js(BrowserProto.imagesPending(tall)) ?? "0") ?? 0) > 0 {
            try? await Task.sleep(for: .milliseconds(100))
        }
        try? await Task.sleep(for: .milliseconds(100))
        let layout = await js(Layout.script(width: Double(Self.side), height: tall))
        let rows = (await js(BrowserProto.pageBoxes)).flatMap { try? JSONSerialization.jsonObject(with: Data($0.utf8)) as? [[Double]] } ?? []
        let c = WKSnapshotConfiguration()
        c.rect = web.bounds
        c.snapshotWidth = 233
        let img = try? await web.takeSnapshot(configuration: c)
        window.setContentSize(NSSize(width: Self.side, height: Self.side))
        web.frame = NSRect(x: 0, y: 0, width: Self.side, height: Self.side)
        guard let img, let cg = img.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
            return ["msgs": hex([BrowserProto.note("No picture")])]
        }
        if let dir = ProcessInfo.processInfo.environment["BROWSERTOOL_DUMP"], let t = img.tiffRepresentation,
           let png = NSBitmapImageRep(data: t)?.representation(using: .png, properties: [:]) {
            try? png.write(to: URL(fileURLWithPath: dir).appendingPathComponent("page-\(Int(Date().timeIntervalSince1970 * 10) % 100000).png"))
        }
        let (w, h) = (233, max(1, Int(233 * tall / Double(Self.side))))
        var px = [UInt8](repeating: 0, count: w * h * 4)
        px.withUnsafeMutableBytes { b in
            let ctx = CGContext(data: b.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
            ctx.draw(cg, in: CGRect(x: 0, y: 0, width: w, height: h))
        }
        var rgb = [UInt8](repeating: 0, count: w * h * 3)
        for i in 0..<(w * h) { (rgb[3 * i], rgb[3 * i + 1], rgb[3 * i + 2]) = (px[4 * i], px[4 * i + 1], px[4 * i + 2]) }
        // drawn on the watch as it is laid out; a fancy page (or BROWSERTOOL_PICTURE): its picture
        var drawn: (messages: [Data], height: Int)?
        if ProcessInfo.processInfo.environment["BROWSERTOOL_PICTURE"] == nil, let lay = layout {
            drawn = Layout.messages(json: lay, rgb: rgb, pw: w, ph: h, cssWidth: Double(Self.side), cssHeight: tall)
        }
        let (msgs, came) = drawn ?? Rle.longPicture(rgb: rgb, width: w, height: h, scale: 2)
        lastMode = drawn == nil ? "picture" : "drawn"
        let k = 466 / Double(Self.side)
        let boxes = rows.compactMap { r -> (x: Int, y: Int, w: Int, h: Int, kind: UInt8)? in
            guard r.count == 5, Int(r[1] * k) < came else { return nil }
            return (Int(r[0] * k), Int(r[1] * k), Int(r[2] * k), Int(r[3] * k), UInt8(r[4]))
        }
        return ["msgs": hex(msgs + BrowserProto.boxes(boxes)), "url": web.url?.absoluteString ?? "", "mode": lastMode]
    }

    func screenPage() async -> [String: Any] {
        let boxScript = """
        (() => { document.querySelectorAll('details:not([open])').forEach(d => d.open = true);
          const out = [], W = innerWidth, H = innerHeight;
          document.querySelectorAll('a[href], button, input, textarea, select, [role=button], [onclick], summary').forEach(e => {
            const r = e.getBoundingClientRect(); if (r.width < 4 || r.height < 4 || r.bottom < 0 || r.top > H || r.right < 0 || r.left > W) return;
            const t = e.tagName.toLowerCase(), ty = (e.type || '').toLowerCase();
            const k = (t == 'textarea' || (t == 'input' && !['button','submit','checkbox','radio','image','reset','hidden'].includes(ty))) ? 1 : (t == 'a' ? 0 : 2);
            out.push([r.left, r.top, r.width, r.height, k]); });
          return JSON.stringify(out.slice(0, 48)); })()
        """
        let rows = (await js(boxScript)).flatMap { try? JSONSerialization.jsonObject(with: Data($0.utf8)) as? [[Double]] } ?? []
        let k = 466 / Self.side
        let boxes = rows.compactMap { r in r.count == 5 ? (x: Int(r[0] * k), y: Int(r[1] * k), w: Int(r[2] * k), h: Int(r[3] * k), kind: UInt8(r[4])) : nil }
        let c = WKSnapshotConfiguration()
        c.rect = web.bounds
        c.snapshotWidth = 233
        guard let img = try? await web.takeSnapshot(configuration: c),
              let cg = img.cgImage(forProposedRect: nil, context: nil, hints: nil) else { return ["msgs": hex([BrowserProto.note("No picture")])] }
        let side = 233
        var px = [UInt8](repeating: 0, count: side * side * 4)
        px.withUnsafeMutableBytes { b in
            let ctx = CGContext(data: b.baseAddress, width: side, height: side, bitsPerComponent: 8, bytesPerRow: side * 4,
                                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
            ctx.draw(cg, in: CGRect(x: 0, y: 0, width: side, height: side))
        }
        var rgb = [UInt8](repeating: 0, count: side * side * 3)
        for i in 0..<(side * side) { (rgb[3 * i], rgb[3 * i + 1], rgb[3 * i + 2]) = (px[4 * i], px[4 * i + 1], px[4 * i + 2]) }
        return ["msgs": hex(Rle.bandedPicture(rgb: rgb, width: side, height: side, scale: 2) + BrowserProto.boxes(boxes)),
                "url": web.url?.absoluteString ?? ""]
    }

    /// A tap at page px: a field brings the keyboard; the page changed (a link, a section opened): all of it again.
    func tapPage(_ x: Int, _ y: Int) async -> [String: Any] {
        let before = await js(BrowserProto.pageState)
        let r = await js(BrowserProto.tapScript(x: x, y: y, side: Double(Self.side)))
        if r == "2" { return ["field": true] }
        try? await Task.sleep(for: .milliseconds(800))
        await waitLoad()
        if await js(BrowserProto.pageState) == before { return ["msgs": hex([BrowserProto.unchanged])] }
        return await page()
    }

    /// A playing video's frame, 155 px (the page's picture when the video won't give its pixels), as the watch's picture.
    func frame() async -> [String: Any] {
        let grab = """
        (() => { const v = [...document.querySelectorAll('video')].find(v => !v.paused && !v.ended && v.readyState > 2);
          if (!v || !v.videoWidth) return null;
          const c = document.createElement('canvas'), w = 155, h = Math.round(155 * v.videoHeight / v.videoWidth);
          c.width = w; c.height = Math.min(h, 155);
          try { c.getContext('2d').drawImage(v, 0, 0, c.width, c.height); return c.toDataURL('image/jpeg', 0.6); }
          catch (e) { return null; } })()
        """
        var cg: CGImage?
        if let s = await js(grab), s.hasPrefix("data:"), let comma = s.firstIndex(of: ","),
           let d = Data(base64Encoded: String(s[s.index(after: comma)...])), let img = NSImage(data: d) {
            cg = img.cgImage(forProposedRect: nil, context: nil, hints: nil)
        }
        if cg == nil {
            let c = WKSnapshotConfiguration()
            c.rect = web.bounds
            c.snapshotWidth = 155
            cg = (try? await web.takeSnapshot(configuration: c))?.cgImage(forProposedRect: nil, context: nil, hints: nil)
        }
        guard let cg else { return ["none": true] }
        let side = 155
        var px = [UInt8](repeating: 0, count: side * side * 4)
        px.withUnsafeMutableBytes { b in
            let ctx = CGContext(data: b.baseAddress, width: side, height: side, bitsPerComponent: 8, bytesPerRow: side * 4,
                                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
            ctx.setFillColor(.black)
            ctx.fill(CGRect(x: 0, y: 0, width: side, height: side))
            // the frame's whole width, in the middle
            let h = CGFloat(side) * CGFloat(cg.height) / CGFloat(max(cg.width, 1))
            ctx.draw(cg, in: CGRect(x: 0, y: (CGFloat(side) - h) / 2, width: CGFloat(side), height: h))
        }
        var rgb = [UInt8](repeating: 0, count: side * side * 3)
        for i in 0..<(side * side) { (rgb[3 * i], rgb[3 * i + 1], rgb[3 * i + 2]) = (px[4 * i], px[4 * i + 1], px[4 * i + 2]) }
        return ["msgs": hex(Rle.picture(rgb: rgb, width: side, height: side, scale: 3))]
    }

    func tap(_ x: Double, _ y: Double) async -> [String: Any] {
        let k = Self.side / 466
        let (px, py) = (x * k, y * k)
        let r = await js("""
        (() => { const e = document.elementFromPoint(\(px), \(py)); if (!e) return 0;
          const f = e.closest('input,textarea,[contenteditable="true"]');
          if (f && !['button','submit','checkbox','radio','image','reset'].includes((f.type||'').toLowerCase())) { f.focus(); return 2; }
          const v = e.closest('video') || (e.closest('div,figure') || document.body).querySelector('video');
          if (v && v.paused && v.getBoundingClientRect().height > 20) { v.muted = false; v.play(); }
          e.click(); return 1; })()
        """)
        if r == "2" { return ["field": true] }
        try? await Task.sleep(for: .milliseconds(700))
        await waitLoad()
        return await page()
    }

    func type(_ t: String) async -> [String: Any] {
        let s = String(data: try! JSONSerialization.data(withJSONObject: [t]), encoding: .utf8)!.dropFirst().dropLast()
        _ = await js("""
        (() => { const f = document.activeElement; if (!f) return;
          if (f.isContentEditable) f.textContent = \(s); else f.value = \(s);
          f.dispatchEvent(new Event('input', { bubbles: true }));
          const k = { key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true };
          f.dispatchEvent(new KeyboardEvent('keydown', k));
          if (f.form) { f.form.requestSubmit ? f.form.requestSubmit() : f.form.submit(); } })()
        """)
        try? await Task.sleep(for: .milliseconds(900))
        await waitLoad()
        return await page()
    }

    func run(_ line: String) async -> [String: Any] {
        let parts = line.split(separator: " ", maxSplits: 1).map(String.init)
        let arg = parts.count > 1 ? parts[1] : ""
        switch parts.first ?? "" {
        case "reader":
            guard let u = URL(string: arg) else { return [:] }
            return await reader(u)
        case "google":
            guard let u = URL(string: arg) else { return [:] }
            return await google(u)
        case "page":
            if arg != "-", let u = URL(string: arg), web.url != u { await load(u) }
            return await page()
        case "scroll":
            _ = await js("window.scrollBy(0, \((Double(arg) ?? 0) * Self.side / 466))")
            try? await Task.sleep(for: .milliseconds(250))
            return await page()
        case "tap":
            let xy = arg.split(separator: " ").compactMap { Int($0) }
            return xy.count == 2 ? await tapPage(xy[0], xy[1]) : [:]
        case "type": return await type(arg)
        case "playing":
            let r = await js("[...document.querySelectorAll('video')].some(v => !v.paused && !v.ended && v.readyState > 2)")
            return ["playing": r == "1" || r == "true"]
        case "frame": return await frame()
        case "load":
            if let u = URL(string: arg), web.url != u { await load(u) }
            return [:]
        case "play":
            // a video page: its video started once the player is there
            for _ in 0..<12 {
                if await js(BrowserProto.playScript) == "1" { break }
                try? await Task.sleep(for: .milliseconds(500))
            }
            try? await Task.sleep(for: .seconds(2))
            return [:]
        case "vstate":
            let a = (await js(BrowserProto.videoState)).flatMap { try? JSONSerialization.jsonObject(with: Data($0.utf8)) as? [Any] } ?? []
            guard a.count == 6 else { return ["exists": false] }
            return ["exists": true, "paused": (a[1] as? Int) == 1, "ended": (a[2] as? Int) == 1,
                    "msgs": hex([BrowserProto.progress(cur: a[3] as? Double ?? 0, dur: a[4] as? Double ?? 0, paused: (a[1] as? Int) == 1)]),
                    "title": hex([BrowserProto.videoTitle(a[5] as? String ?? "")])]
        case "toggle":
            _ = await js(BrowserProto.toggleScript)
            return [:]
        case "seek":
            _ = await js(BrowserProto.seekScript(Int(arg) ?? 0))
            return [:]
        case "isvideo":
            return ["video": URL(string: arg).map(BrowserProto.isVideo) ?? false]
        case "stop":
            _ = await js("document.querySelectorAll('video,audio').forEach(v => v.pause())")
            return [:]
        case "js":
            return ["text": await js(arg) ?? "nil"]
        case "wait":
            try? await Task.sleep(for: .seconds(Double(arg) ?? 1))
            return [:]
        case "text":
            // what the browser shows now (debugging)
            return ["text": await js("document.title + ' | ' + location.href + ' | ' + document.body.innerText.slice(0, 600)") ?? ""]
        case "back":
            guard web.canGoBack else { return ["none": true] }
            web.goBack()
            await waitLoad()
            return await page()
        default: return [:]
        }
    }
}

nonisolated(unsafe) var sources: [DispatchSourceSignal] = []

func hex(_ m: [Data]) -> [String] { m.map { $0.map { String(format: "%02x", $0) }.joined() } }

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
Task { @MainActor in
    let tool = Tool()
    for (sig, on) in [(SIGUSR1, true), (SIGUSR2, false)] {
        signal(sig, SIG_IGN)
        let src = DispatchSource.makeSignalSource(signal: sig, queue: .main)
        src.setEventHandler { MainActor.assumeIsolated { tool.show(on) } }
        src.resume()
        sources.append(src)
    }
    while let line = await Task.detached(operation: { readLine() }).value {
        let out = await tool.run(line)
        let d = try! JSONSerialization.data(withJSONObject: out)
        FileHandle.standardOutput.write(d + Data("\n".utf8))
    }
    exit(0)
}
app.run()
