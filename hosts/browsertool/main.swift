// The phone's Browser work on a Mac, for testing the watch app without the phone (hosts/browserhost.py --web):
// HubKit's Reader and picture code (cmfx ios/HubKit, compiled in) and a WebKit view for page view and Google.
// One command a line on stdin, one JSON answer a line on stdout:
//   reader URL | google URL | page URL ("-" the current) | scroll DY | tap X Y | type TEXT | back | playing | frame
// Answers: {"msgs": [hex…], "links": [url…]} (watch messages), {"fallback": url}, {"field": true}, {"none": true}.
import AppKit
import Foundation
import WebKit

@MainActor final class Tool: NSObject, WKNavigationDelegate, NSWindowDelegate, NSTextFieldDelegate {
    /// shown to sign in by hand: an address bar over the page
    var shown = false
    let bar = NSTextField(frame: .zero)
    let box = NSView(frame: .zero)
    static let side: CGFloat = 360
    let window: NSWindow
    let web: WKWebView
    var waiters: [CheckedContinuation<Void, Never>] = []
    /// the navigation under way has its new document (its text may still be coming)
    var committed = true
    /// how the last page went to the watch (debugging)
    var lastMode = ""
    /// the simple screen on the watch (YouTube, Instagram): its site, where its search bar and rows are
    var simple: (site: Simple.Site, screen: Simple.Screen)?

    /// A site's simple screen (Simple): what it lists, read once it is there (3 s at most), thumbnails fetched.
    /// the feed walked so far (its step, 450 px each) and the posts sent: the next ones come on "more"
    var feedStep = 0
    var feedSeen: Set<URL> = []

    /// The feed's next posts (endless, as the app's): from where the last ones ended.
    func simpleMore() async -> [String: Any] {
        guard let (site, _) = simple, web.url.map(Simple.isFeed) ?? false else { return ["msgs": hex([BrowserProto.unchanged])] }
        return await simplePage(site)
    }

    func simplePage(_ site: Simple.Site) async -> [String: Any] {
        var items: [Simple.Item] = []
        let feed = web.url.map(Simple.isFeed) ?? false
        var more = false
        if feed {
            // Instagram builds only the posts near the view: walked down in steps, the posts gathered on the way
            var got: [URL: Simple.Item] = [:], order: [URL] = []
            let from = feedStep
            for k in from..<(from + 24) {
                feedStep = k + 1
                _ = await js("window.scrollTo(0, \(k * 450))")
                try? await Task.sleep(for: .milliseconds(k == 0 ? 800 : 450))
                for it in Simple.items(await js(Simple.feedScript) ?? "[]") where !it.byline.isEmpty && !feedSeen.contains(it.link) {
                    if got[it.link] == nil { order.append(it.link) }
                    got[it.link] = it
                }
                if order.count >= 3 { break }
            }
            items = order.compactMap { got[$0] }
            feedSeen.formUnion(order)
            more = !items.isEmpty
        } else {
            for _ in 0..<15 {
                items = Simple.items(await js(Simple.script(site)) ?? "[]")
                if !items.isEmpty { break }
                try? await Task.sleep(for: .milliseconds(200))
            }
        }
        let thumbs = await Simple.thumbs(items, site: site, feed: feed)
        let q = web.url.flatMap { URLComponents(url: $0, resolvingAgainstBaseURL: false)?.queryItems?
            .first { $0.name == "search_query" || $0.name == "q" }?.value } ?? ""
        let sc = Simple.screen(site, query: q, items: items, thumbs: thumbs, feed: feed, tab: web.url.flatMap(Simple.tab))
        simple = (site, sc)
        let boxes = [(x: sc.field.x, y: sc.field.y, w: sc.field.w, h: sc.field.h, kind: UInt8(1))]
            + sc.rows.prefix(150).map { (x: $0.x, y: $0.y, w: $0.w, h: $0.h, kind: UInt8(0)) }
        lastMode = "simple"
        var msgs = sc.messages
        if more, let j = msgs.firstIndex(where: { $0.first == UInt8(ascii: "J") }) {
            // more posts below (the watch asks when it gets to the end)
            var y = Data([UInt8(ascii: "y")]); y.le(UInt16(0)); y.append(1)
            msgs.insert(y, at: j + 1)
        }
        return ["msgs": hex(msgs + BrowserProto.boxes(boxes)), "url": web.url?.absoluteString ?? "", "mode": lastMode]
    }

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
        window.contentView = box
        box.addSubview(web)
        bar.placeholderString = "Address or search, then Enter"
        bar.isHidden = true
        bar.delegate = self
        bar.target = self
        bar.action = #selector(go)
        box.addSubview(bar)
        box.frame = NSRect(x: 0, y: 0, width: Self.side, height: Self.side)
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
    /// Cookies exported from the owner's own browser (Netscape cookies.txt, e.g. ~/cmf-browser-cookies.txt): signed in
    /// where an embedded browser can't sign in. Read once at start; nothing is written back.
    func importCookies(_ path: String) async -> Int {
        guard let text = try? String(contentsOfFile: path, encoding: .utf8) else { return 0 }
        let store = web.configuration.websiteDataStore.httpCookieStore
        var n = 0
        for raw in text.split(whereSeparator: \.isNewline) {
            var line = String(raw)
            // HttpOnly cookies are written "#HttpOnly_domain…"
            if line.hasPrefix("#HttpOnly_") { line.removeFirst("#HttpOnly_".count) } else if line.hasPrefix("#") { continue }
            var f = line.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
            if f.count < 7 {
                // tabs pasted as spaces: the first six fields have none, the value none here either
                f = line.split(separator: " ", maxSplits: 6, omittingEmptySubsequences: true).map { $0.trimmingCharacters(in: .whitespaces) }
            }
            guard f.count >= 7 else { continue }
            var p: [HTTPCookiePropertyKey: Any] = [.domain: f[0], .path: f[2], .name: f[5], .value: f[6]]
            if f[3].uppercased() == "TRUE" { p[.secure] = "TRUE" }
            if let t = Double(f[4]), t > 0 { p[.expires] = Date(timeIntervalSince1970: t) }
            guard let c = HTTPCookie(properties: p) else { continue }
            await store.setCookie(c)
            n += 1
        }
        return n
    }

    @objc func go() {
        let t = bar.stringValue.trimmingCharacters(in: .whitespaces)
        guard !t.isEmpty else { return }
        web.load(URLRequest(url: BrowserProto.url(for: t)))
    }

    nonisolated func webView(_ v: WKWebView, didFinish n: WKNavigation!) {
        Task { @MainActor in
            self.done()
            if self.shown, let u = v.url, self.window.firstResponder != self.bar.currentEditor() { self.bar.stringValue = u.absoluteString }
        }
    }

    /// The shown window's layout: the bar on top, the page under it.
    func lay() {
        let (w, h): (CGFloat, CGFloat) = shown ? (420, 760) : (Self.side, Self.side)
        window.setContentSize(NSSize(width: w, height: h))
        box.frame = NSRect(x: 0, y: 0, width: w, height: h)
        if shown {
            bar.frame = NSRect(x: 6, y: h - 30, width: w - 12, height: 24)
            web.frame = NSRect(x: 0, y: 0, width: w, height: h - 36)
        } else {
            web.frame = NSRect(x: 0, y: 0, width: w, height: h)
        }
    }

    func show(_ on: Bool) {
        shown = on
        bar.isHidden = !on
        if on {
            NSApp.setActivationPolicy(.regular)
            window.styleMask = [.titled, .closable, .resizable, .miniaturizable]
            window.title = "Watch browser"
            window.alphaValue = 1
            window.ignoresMouseEvents = false
            window.level = .normal
            lay()
            bar.stringValue = web.url?.absoluteString ?? ""
            window.center()
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            // nothing open yet: Google's sign-in (YouTube's too), from where any site can be reached
            if web.url == nil && ProcessInfo.processInfo.environment["BROWSERTOOL_SHOW"] == nil {
                web.load(URLRequest(url: URL(string: "https://accounts.google.com/ServiceLogin?continue=https://m.youtube.com/")!))
            }
        } else {
            window.styleMask = .borderless
            window.alphaValue = 0.02
            window.ignoresMouseEvents = true
            window.level = .floating
            lay()
            window.setFrameOrigin(.zero)
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
        coder = nil
        var u = u
        let host = (u.host ?? "").lowercased()
        let youtube = host.hasSuffix("youtube.com") || u.host == "youtu.be"
        // Fandom: behind Cloudflare, whose check passes with this engine's own name only (a Mac's); its phone layout
        // asked for by name
        let fandom = host.hasSuffix("fandom.com")
        if fandom, var c = URLComponents(url: u, resolvingAgainstBaseURL: false),
           !(c.queryItems ?? []).contains(where: { $0.name == "useskin" }) {
            c.queryItems = (c.queryItems ?? []) + [URLQueryItem(name: "useskin", value: "fandommobile")]
            u = c.url ?? u
        }
        // elsewhere an iPhone's Safari: sites' phone layouts (Google's)
        let ua = youtube ? chromeUA : fandom ? nil : Self.phoneUA
        if web.customUserAgent != (ua ?? "") { web.customUserAgent = ua }
        let p = u.path.lowercased()
        if [".mp4", ".webm", ".mov", ".m4v", ".m3u8"].contains(where: { p.hasSuffix($0) }) {
            // a video file: in a page of its own (a bare file has no video element to play or read)
            let src = u.absoluteString.replacingOccurrences(of: "\"", with: "%22")
            web.loadHTMLString("<!doctype html><title>\(u.lastPathComponent)</title><body style=margin:0;background:#000>"
                + "<video src=\"\(src)\" autoplay playsinline style=width:100%></video>", baseURL: u)
        } else {
            web.load(URLRequest(url: u))
        }
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
        // a redirect in the page (script) or an app still on its splash (Instagram's logo): till the address holds
        // and there is something to show, 3 s at most
        var last = ""
        for _ in 0..<15 {
            let st = await js("JSON.stringify([location.href, (document.body ? document.body.innerText.length : 0), document.images.length])") ?? ""
            let parts = (try? JSONSerialization.jsonObject(with: Data(st.utf8)) as? [Any]) ?? []
            let href = parts.first as? String ?? "", chars = parts.count > 1 ? (parts[1] as? Int ?? 0) : 0
            if href == last && chars > 120 { break }
            last = href
            try? await Task.sleep(for: .milliseconds(200))
        }
        // a bot check ("Just a moment…", Cloudflare's): it passes by itself in a few seconds
        // (it checks the name against the engine: with the phone's name it may not pass; then Safari's own, again)
        for i in 0..<60 where (await js("document.title") ?? "").hasPrefix("Just a moment") {
            if i == 15, web.customUserAgent != nil, web.customUserAgent != "" {
                web.customUserAgent = nil
                web.reload()
                try? await Task.sleep(for: .milliseconds(500))
            }
            try? await Task.sleep(for: .milliseconds(200))
        }
        // Google's results are in its HTML: no settling time
        let google = web.url.map(BrowserProto.isGoogleSearch) ?? false
        try? await Task.sleep(for: .milliseconds(google ? 60 : 400))
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
        simple = nil
        sectioned = false
        feedStep = 0
        feedSeen = []
        if let u = web.url, let site = Simple.site(for: u) { return await simplePage(site) }
        if let u = web.url, BrowserProto.isGoogleSearch(u) {
            // plain results (udm=14): a button for the full ones, with Google's AI Overview
            _ = await js(BrowserProto.overviewButton)
        }
        if pictureMode || !Self.drawnHere(web.url) || ProcessInfo.processInfo.environment["BROWSERTOOL_PICTURE"] != nil {
            return await section(from: 0)
        }
        do {
            // the watch's font in the page (once a page): its text measured as the watch draws it
            if let f = Self.watchFont, await js("window.__wf ? 1 : 0") != "1" {
                _ = await js("""
                (() => { const s = document.createElement('style');
                  s.textContent = "@font-face { font-family: '__watch'; src: url(data:font/ttf;base64,\(f)); font-weight: 100 900; }";
                  document.head.appendChild(s); return document.fonts.load("500 16px __watch").then(() => 1); })()
                """)
                for _ in 0..<20 where await js("document.fonts.check('500 16px __watch') ? 1 : 0") != "1" {
                    try? await Task.sleep(for: .milliseconds(50))
                }
                _ = await js("window.__wf = document.fonts.check('500 16px __watch'); 1")
            }
            _ = await js(Self.watchText)
            try? await Task.sleep(for: .milliseconds(50))
        }
        let full = Double(await js(BrowserProto.pageHeight) ?? "") ?? Double(Self.side)
        let tall = min(max(full, Double(Self.side)), Double(Self.side) * 12)
        // laid out tall at the watch's width (the window as tall; shown or not), then back as it was
        window.setContentSize(NSSize(width: Self.side, height: tall))
        box.frame = NSRect(x: 0, y: 0, width: Self.side, height: tall)
        web.frame = NSRect(x: 0, y: 0, width: Self.side, height: tall)
        _ = await js("window.scrollTo(0, 0)")
        let google = web.url.map(BrowserProto.isGoogleSearch) ?? false
        try? await Task.sleep(for: .milliseconds(google ? 60 : 150))
        // its images in (lazy ones told to load: all of the tall page is in view), 1.5 s at most (Google: 0.5 s)
        for _ in 0..<(google ? 5 : 15) where (Int(await js(BrowserProto.imagesPending(tall)) ?? "0") ?? 0) > 0 {
            try? await Task.sleep(for: .milliseconds(100))
        }
        try? await Task.sleep(for: .milliseconds(100))
        let layout = await js(Layout.script(width: Double(Self.side), height: tall))
        let rows = (await js(BrowserProto.pageBoxes)).flatMap { try? JSONSerialization.jsonObject(with: Data($0.utf8)) as? [[Double]] } ?? []
        let c = WKSnapshotConfiguration()
        c.rect = web.bounds
        c.snapshotWidth = 233
        let img = try? await web.takeSnapshot(configuration: c)
        lay()
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
        if ProcessInfo.processInfo.environment["BROWSERTOOL_PICTURE"] == nil, !pictureMode, Self.drawnHere(web.url),
           let lay = layout {
            drawn = Layout.messages(json: lay, rgb: rgb, pw: w, ph: h, cssWidth: Double(Self.side), cssHeight: tall, always: true)
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

    /// picture pages: the part of the real page sent (its top and height, page css px), more below it, above it
    var sectionTop = 0.0, sectionHeight = 0.0
    var sectioned = false

    /// A picture page in parts: the real page scrolled (so lazy and endless pages load as they would), a screen at a
    /// time, `steps` screens stitched into one picture from `y0`. Headers that stay on screen (fixed, sticky) show in
    /// the first screen of the page only. Each video gets a play button (a tap plays it on the watch's player).
    /// Then `y scroll:u16 flags:u8` after `J` (flags: 1 more below, 2 more above, 4 not drawn here).
    func section(from y0: Double, scroll: Int = 0) async -> [String: Any] {
        let vh = Double(Self.side), steps = 6, w = 233
        var rgb: [UInt8] = []
        var rows: [[Double]] = []
        var top = max(0, y0), end = false
        lay()
        for i in 0..<steps {
            let want = top + Double(i) * vh
            let got = Double(await js("window.scrollTo(0, \(want)); \(Self.playButtons); scrollY") ?? "") ?? want
            if i == 0 { top = got }
            if top > 0 || i > 0 { _ = await js(Self.hideFixed) }
            try? await Task.sleep(for: .milliseconds(i == 0 ? 150 : 120))
            for _ in 0..<6 where (Int(await js(BrowserProto.imagesPending(vh)) ?? "0") ?? 0) > 0 {
                try? await Task.sleep(for: .milliseconds(100))
            }
            // web fonts in (text in one is blank till it loads), 1.5 s at most
            for _ in 0..<15 where await js("document.fonts.status") == "loading" {
                try? await Task.sleep(for: .milliseconds(100))
            }
            let at = Double(await js("scrollY") ?? "") ?? got
            // the page ended: only what is below the last screen
            let already = i == 0 ? 0 : Int(((top + Double(i) * vh) - at) / vh * Double(w))
            if at + 1 < top + Double(i) * vh, already >= w { end = true; break }
            let c = WKSnapshotConfiguration()
            c.rect = web.bounds
            c.snapshotWidth = NSNumber(value: w)
            guard let img = try? await web.takeSnapshot(configuration: c),
                  let cg = img.cgImage(forProposedRect: nil, context: nil, hints: nil) else { break }
            var px = [UInt8](repeating: 0, count: w * w * 4)
            px.withUnsafeMutableBytes { b in
                let ctx = CGContext(data: b.baseAddress, width: w, height: w, bitsPerComponent: 8, bytesPerRow: w * 4,
                                    space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
                ctx.draw(cg, in: CGRect(x: 0, y: 0, width: w, height: w))
            }
            for y in max(0, already)..<w {
                for x in 0..<w { let p = 4 * (y * w + x); rgb += [px[p], px[p + 1], px[p + 2]] }
            }
            let shift = at - top
            let found = (await js(BrowserProto.pageBoxes)).flatMap { try? JSONSerialization.jsonObject(with: Data($0.utf8)) as? [[Double]] } ?? []
            rows += found.filter { $0.count == 5 && $0[1] >= Double(already) * vh / Double(w) - 1 && $0[1] < vh }
                .map { [$0[0], $0[1] + shift, $0[2], $0[3], $0[4]] }
            if at + 1 < top + Double(i) * vh { end = true; break }
        }
        let h = rgb.count / (3 * w)
        sectionTop = top
        sectionHeight = Double(h) * vh / Double(w)
        sectioned = true
        let full = Double(await js("document.documentElement.scrollHeight") ?? "") ?? 0
        var (msgs, came) = Rle.longPicture(rgb: rgb, width: w, height: h, scale: 2)
        // what the watch keeps (a busy picture fills its store early): the next part from there
        let short = came < h * 2
        sectionHeight = Double(came / 2) * vh / Double(w)
        let below = short || (!end && top + sectionHeight + 4 < full)
        var y = Data([UInt8(ascii: "y")])
        y.le(UInt16(clamping: scroll))
        y.append((below ? 1 : 0) | (top > 0 ? 2 : 0) | (Self.drawnHere(web.url) ? 0 : 4))
        msgs.insert(y, at: 1)
        let k = 466 / vh
        var seen = Set<[Int]>()
        let boxes = rows.compactMap { r -> (x: Int, y: Int, w: Int, h: Int, kind: UInt8)? in
            let b = (Int(r[0] * k), Int(r[1] * k), Int(r[2] * k), Int(r[3] * k))
            guard b.1 < came, seen.insert([b.0, b.1, b.2, b.3]).inserted else { return nil }
            return (b.0, b.1, b.2, b.3, UInt8(r[4]))
        }
        lastMode = "picture"
        return ["msgs": hex(msgs + BrowserProto.boxes(boxes)), "url": web.url?.absoluteString ?? "", "mode": lastMode]
    }

    /// The next part of a picture page (`dir` 1 down, -1 up), its first screen what the watch shows now.
    func more(_ dir: Int) async -> [String: Any] {
        if simple != nil { return await simpleMore() }
        guard sectioned else { return ["msgs": hex([BrowserProto.unchanged])] }
        // the watch shows the last 300 px (going down) or the first (going up): the new part keeps them in view
        let keep = 300 * Double(Self.side) / 466
        if dir > 0 { return await section(from: sectionTop + sectionHeight - keep) }
        let y0 = max(0, sectionTop - sectionHeight + keep)
        return await section(from: y0, scroll: Int((sectionTop - y0) * 466 / Double(Self.side)))
    }

    /// Headers and bars that stay on screen, hidden (not to repeat down the stitched picture); their room kept.
    static let hideFixed = """
    (() => { for (const e of document.querySelectorAll('body *')) { const p = getComputedStyle(e).position;
      if (p == 'fixed' || p == 'sticky') e.style.setProperty('visibility', 'hidden', 'important'); } return 1; })()
    """

    /// A play button on each video (it plays on a tap: the watch's player), its moving pictures still; on the page,
    /// so it is in the picture. Videos that play by themselves (muted) don't go to the player.
    static let playButtons = """
    (() => { document.querySelectorAll('.__wpb').forEach(e => e.remove());
      for (const v of document.querySelectorAll('video')) { const r = v.getBoundingClientRect();
        if (r.width < 40 || r.height < 30) continue;
        const b = document.createElement('div'); b.className = '__wpb';
        const d = Math.min(64, r.width / 3, r.height / 2);
        b.style.cssText = `position:absolute;z-index:2147483647;pointer-events:none;left:${r.left + scrollX + r.width / 2 - d / 2}px;top:${r.top + scrollY + r.height / 2 - d / 2}px;width:${d}px;height:${d}px;border-radius:50%;background:rgba(0,0,0,.7);border:2px solid #fff;box-sizing:border-box`;
        const t = document.createElement('div');
        t.style.cssText = `position:absolute;left:${d * 0.38}px;top:${d * 0.27}px;width:0;height:0;border-style:solid;border-width:${d * 0.22}px 0 ${d * 0.22}px ${d * 0.34}px;border-color:transparent transparent transparent #fff`;
        b.appendChild(t); document.body.appendChild(b); }
      return 1; })()
    """

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
        if let (_, sc) = simple {
            // the simple screen: its search bar, its rows
            let f = sc.field
            if (f.x...f.x + f.w).contains(x), (f.y...f.y + f.h).contains(y) { return ["field": true] }
            if let r = sc.rows.first(where: { ($0.x...$0.x + $0.w).contains(x) && ($0.y...$0.y + $0.h).contains(y) }) {
                return ["open": r.link.absoluteString]
            }
            return ["msgs": hex([BrowserProto.unchanged])]
        }
        let before = await js(BrowserProto.pageState)
        let top = lastMode == "picture" && sectioned ? Int(sectionTop * 466 / Double(Self.side)) : 0
        let r = await js(BrowserProto.tapScript(x: x, y: y + top, side: Double(Self.side)))
        if r == "2" { return ["field": true] }
        try? await Task.sleep(for: .milliseconds(800))
        await waitLoad()
        if await js(BrowserProto.pageState) == before { return ["msgs": hex([BrowserProto.unchanged])] }
        // the same page changed (a menu, a section opened): from where the watch is
        if lastMode == "picture", sectioned, let s = before?.split(separator: "|").first, web.url?.absoluteString == String(s) {
            return await section(from: sectionTop)
        }
        return await page()
    }

    /// A playing video's frame, 155 px (the page's picture when the video won't give its pixels), as the watch's picture.
    /// the video's coder (VideoCoder): a new one for each video (or a new shape)
    var coder: VideoCoder?
    /// The sites the watch draws (their text sharp in its font); the rest come as a picture of the page.
    static func drawnHere(_ u: URL?) -> Bool {
        let h = (u?.host ?? "").lowercased()
        return ["google.", "youtube.com", "instagram.com", "fandom.com", "wikipedia.org", "wikimedia.org", "wiktionary.org"]
            .contains { h.contains($0) }
    }

    /// Before a drawn page is laid out: its text as wide as the watch draws it (about 10.5 panel px a character, 8.1
    /// of the page's at 360 wide; one size, as the watch has one), so lines wrap where the watch's do and nothing
    /// runs into the next; icon fonts left alone. Banners over the page (an app's "get the app", a cookie wall) gone.
    static let watchText = """
    (() => { if (window.__wt) return 0; window.__wt = 1;
      // collapsed sections open (Wikipedia's on phones): the whole article
      document.querySelectorAll('[hidden="until-found"]').forEach(e => e.removeAttribute('hidden'));
      document.querySelectorAll('.collapsible-heading,.mf-collapsible-heading').forEach(e => e.setAttribute('aria-expanded', 'true'));
      // the watch's font: Inter Medium, 20 panel px (15.45 page px at 360 wide)
      const fs = (20 * 360 / 466).toFixed(2) + 'px';
      const fam = window.__wf ? '__watch' : '-apple-system,Helvetica,Arial,sans-serif';
      const tw = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT); const done = new Set(); let n;
      while ((n = tw.nextNode())) { const e = n.parentElement; if (!e || done.has(e) || !n.data.trim()) continue; done.add(e);
        const st = getComputedStyle(e); if (/icon|symbol|material|awesome/i.test(st.fontFamily)) continue;
        // (the watch's bold is its medium a pixel over: as wide)
        if ((parseInt(st.fontWeight) || 400) >= 600) e.dataset.wb = '1';
        e.style.setProperty('font-family', fam, 'important');
        e.style.setProperty('font-weight', '500', 'important');
        e.style.setProperty('font-style', 'normal', 'important');
        e.style.setProperty('font-size', fs, 'important');
        e.style.setProperty('line-height', '19px', 'important');
        e.style.setProperty('letter-spacing', '0', 'important'); }
      for (const e of document.querySelectorAll('body *')) { const st = getComputedStyle(e);
        if (st.position != 'fixed' && st.position != 'sticky') continue;
        const r = e.getBoundingClientRect();
        if (r.width * r.height > innerWidth * innerHeight * 0.25) e.style.setProperty('display', 'none', 'important'); }
      return 1; })()
    """

    /// Inter (the watch's font is baked from it: hosts/fontgen.py), base64: BROWSER_FONT or runs/font/Inter.ttf
    static let watchFont: String? = {
        let env = ProcessInfo.processInfo.environment["BROWSER_FONT"]
        let exe = URL(fileURLWithPath: CommandLine.arguments[0]).deletingLastPathComponent()
        let path = env ?? exe.appendingPathComponent("../runs/font/Inter.ttf").path
        return (try? Data(contentsOf: URL(fileURLWithPath: path)))?.base64EncodedString()
    }()

    static let phoneUA = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Mobile/15E148 Safari/604.1"

    /// the browser's name to YouTube (the cookies' browser), nil: Safari's own
    var chromeUA: String?

    /// page view as a picture (the watch's picture mode)
    var pictureMode = false
    var coderSize = ""

    /// The playing video's next frame for the watch (VideoCoder: the part that changed, few colours); `msgs` empty
    /// when nothing changed, `none` when nothing plays.
    func frame() async -> [String: Any] {
        guard let size = await js(VideoCoder.sizeScript), !size.isEmpty else { return ["none": true] }
        if coder == nil || size != coderSize {
            let p = size.split(separator: ",").compactMap { Int($0) }
            let scale = Int(ProcessInfo.processInfo.environment["BROWSER_VSCALE"] ?? "") ?? 8
            let (w, h) = VideoCoder.grid(videoWidth: p.first ?? 16, videoHeight: p.last ?? 9, scale: scale)
            coder = VideoCoder(width: w, height: h, scale: scale)
            coder?.colours = Int(ProcessInfo.processInfo.environment["BROWSER_VCOLOURS"] ?? "") ?? 16
            coder?.whole = ProcessInfo.processInfo.environment["BROWSER_VKEEP"] == nil
            // colour by default (BROWSER_VGREY=5: black and white, 5 greys)
            coder?.grey = Int(ProcessInfo.processInfo.environment["BROWSER_VGREY"] ?? "") ?? 0
            coder?.budget = Int(ProcessInfo.processInfo.environment["BROWSER_VBUDGET"] ?? "") ?? 750
            coderSize = size
        }
        guard let c = coder, let s = await js(VideoCoder.grabScript(width: c.width, height: c.height)),
              s.hasPrefix("data:"), let comma = s.firstIndex(of: ","),
              let d = Data(base64Encoded: String(s[s.index(after: comma)...])),
              let rgb = Simple.rgb(d, w: c.width, h: c.height) else { return ["none": true] }
        return ["msgs": hex(c.encode(rgb))]
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
        if let (site, _) = simple {
            await load(Simple.search(site, t))
            return await page()
        }
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
        case "more": return await more(Int(arg) ?? 1)
        case "playing":
            // (one that plays by itself, muted, is the page's: not the player's)
            let r = await js("[...document.querySelectorAll('video')].some(v => !v.paused && !v.ended && v.readyState > 2 && !v.muted)")
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
        case "ua":
            // the browser's name to sites (the cookies' browser: a session moved from Chrome stays signed in as Chrome)
            web.customUserAgent = arg.isEmpty ? nil : arg
            return [:]
        case "picture":
            // page view as a picture of the page (the watch's mode button), not drawn
            pictureMode = arg == "1"
            return [:]
        case "vkey":
            // the watch drew over the picture (paused sign): a whole frame next
            coder = nil
            return [:]
        case "vstate":
            let a = (await js(BrowserProto.videoState)).flatMap { try? JSONSerialization.jsonObject(with: Data($0.utf8)) as? [Any] } ?? []
            guard a.count >= 6 else { return ["exists": false] }
            // an ad: its title says so (a tap skips it once it can be)
            let ad = a.count > 6 && (a[6] as? Int) == 1, skip = a.count > 7 && (a[7] as? Int) == 1
            let title = ad ? (skip ? "Ad \u{00B7} tap to skip" : "Ad") : (a[5] as? String ?? "")
            return ["exists": true, "paused": (a[1] as? Int) == 1, "ended": (a[2] as? Int) == 1, "ad": ad,
                    "msgs": hex([BrowserProto.progress(cur: a[3] as? Double ?? 0, dur: a[4] as? Double ?? 0, paused: (a[1] as? Int) == 1)]),
                    "title": hex([BrowserProto.videoTitle(title)])]
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
    let cookies = ProcessInfo.processInfo.environment["BROWSER_COOKIES"]
        ?? (NSHomeDirectory() + "/cmf-browser-cookies.txt")
    let n = await tool.importCookies(cookies)
    if n > 0 {
        FileHandle.standardError.write(Data("browsertool: \(n) cookies from \(cookies)\n".utf8))
        // the cookies' browser by name (Chrome's export): YouTube drops a session that moves to another browser
        // (YouTube only: elsewhere a Chrome name on Safari's engine trips bot checks, Cloudflare's)
        tool.chromeUA = ProcessInfo.processInfo.environment["BROWSER_UA"]
            ?? "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36"
    }
    if ProcessInfo.processInfo.environment["BROWSERTOOL_SHOW"] != nil {
        // shown from the start (what the watch is shown, live)
        tool.show(true)
    }
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
