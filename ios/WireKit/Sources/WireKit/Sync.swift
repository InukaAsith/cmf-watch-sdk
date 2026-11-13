import Foundation

public struct SyncResult: Equatable, Sendable {
    /// Entries the phone did not have (by `start`).
    public var newEntries: [Entry]
    /// Version just stored on the watch, nil when nothing was sent.
    public var pushed: UInt32?
    /// A newer program waits: a workout is in progress on the watch.
    public var heldBack: Bool
    /// Program version the watch had when it asked (resolves the history's day indices).
    public var watchVersion: UInt32
    /// The newest entry's trace (its start), when asked for.
    public var trace: (start: UInt32, Trace)?

    public static func == (a: SyncResult, b: SyncResult) -> Bool {
        (a.newEntries, a.pushed, a.heldBack, a.watchVersion) == (b.newEntries, b.pushed, b.heldBack, b.watchVersion)
            && a.trace?.start == b.trace?.start && a.trace?.1 == b.trace?.1
    }
}

/// One sync with the open workout app: on its R read the history, then push `program` unless the watch has it, a
/// workout runs or there is none. `firstRequest`: the R that woke the phone, if already received. `tracedStarts`:
/// entries whose trace the phone has; the newest entry's trace is fetched when it is not among them (nil: never).
public func sync(channel: Channel, program: Data?, knownStarts: Set<UInt32>, tracedStarts: Set<UInt32>? = nil,
                 firstRequest: Incoming? = nil,
                 askWait: Duration = .seconds(120), ackWait: Duration = .seconds(30)) async throws -> SyncResult {
    var request = firstRequest
    while true {
        if case .request = request { break }
        request = parseIncoming(try await channel.recv(timeout: askWait))
    }
    guard case let .request(have, running) = request else { fatalError() }

    try await channel.send(historyQuery())
    var got: [Int: Entry] = [:]
    collect: while true {
        guard case let .entry(index, count, entry) = parseIncoming(try await channel.recv(timeout: ackWait)) else {
            continue // a repeated R or another message
        }
        if count == 0 { break collect }
        got[index] = entry
        if got.count == count { break collect }
    }
    let fresh = got.keys.sorted().compactMap { got[$0] }.filter { !knownStarts.contains($0.start) }

    var result = SyncResult(newEntries: fresh, pushed: nil, heldBack: false, watchVersion: have)
    // the watch keeps the trace of its last workout only, until the next one starts
    if let traced = tracedStarts, !running, let newest = got[0], !traced.contains(newest.start) {
        try await channel.send(traceQuery())
        var parts: [Int: Data] = [:]
        var start: UInt32 = 0
        while true {
            guard case let .trace(i, n, s, d) = parseIncoming(try await channel.recv(timeout: ackWait)) else { continue }
            (parts[i], start) = (d, s)
            if parts.count >= n { break }
        }
        if start == newest.start, let t = parseTrace(parts.keys.sorted().reduce(Data()) { $0 + parts[$1]! }) {
            result.trace = (start, t)
        }
    }
    guard let program, programVersion(program) != have else { return result }
    if running {
        result.heldBack = true
        return result
    }
    let v = programVersion(program)
    for m in programMessages(program) { try await channel.send(m) }
    while true {
        if case .ack(v) = parseIncoming(try await channel.recv(timeout: ackWait)) { break }
    }
    result.pushed = v
    return result
}
