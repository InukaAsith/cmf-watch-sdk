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
}

/// One sync with the open workout app: on its R read the history, then push `program` unless the watch has it, a
/// workout runs or there is none. `firstRequest`: the R that woke the phone, if already received.
public func sync(channel: Channel, program: Data?, knownStarts: Set<UInt32>, firstRequest: Incoming? = nil,
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
