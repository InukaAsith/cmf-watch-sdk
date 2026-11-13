import Foundation

// The workout app's "w1" messages (docs/workout.md, Protocol).

let magic = Data("w1".utf8)
let chunk = 120

/// B, C…, E with seq 0, 1, … (`workout.messages`).
public func programMessages(_ program: Data) -> [Data] {
    var out: [Data] = []
    var b = magic + Data("B".utf8) + Data([0])
    b.le(UInt16(program.count))
    out.append(b)
    for off in stride(from: 0, to: program.count, by: chunk) {
        var c = magic + Data("C".utf8) + Data([UInt8(out.count)])
        c.le(UInt16(off))
        out.append(c + program[program.startIndex + off..<program.startIndex + min(off + chunk, program.count)])
    }
    var e = magic + Data("E".utf8) + Data([UInt8(out.count)])
    e.le(programVersion(program))
    out.append(e)
    return out
}

public func historyQuery() -> Data { magic + Data("Q".utf8) + Data([0]) }

/// A finished workout on the watch. `start`: seconds since 2000-01-01 in the watch's local time.
public struct Entry: Codable, Hashable, Sendable {
    public var start: UInt32
    public var durS: Int
    public var day: Int
    public var done: Int
    public var planned: Int

    public init(start: UInt32, durS: Int, day: Int, done: Int, planned: Int) {
        (self.start, self.durS, self.day, self.done, self.planned) = (start, durS, day, done, planned)
    }

    /// The watch counts local seconds since 2000-01-01: read them as a date in the phone's time zone.
    public var date: Date {
        let epoch2000 = 946_684_800.0
        let local = epoch2000 + Double(start)
        return Date(timeIntervalSince1970: local - Double(TimeZone.current.secondsFromGMT(
            for: Date(timeIntervalSince1970: local))))
    }
}

public enum Incoming: Equatable, Sendable {
    /// R: the stored version; running = a workout is in progress (hold a new program back).
    case request(version: UInt32, running: Bool)
    case ack(UInt32)
    /// Y: entry `index` of `count`, newest first; count 0 (entry nil) = empty history.
    case entry(index: Int, count: Int, Entry?)
}

public func parseIncoming(_ d: Data) -> Incoming? {
    guard d.count >= 3, d.prefix(2) == magic else { return nil }
    switch d[d.startIndex + 2] {
    case UInt8(ascii: "R") where d.count == 7 || d.count == 8:
        return .request(version: d.le(UInt32.self, at: 3), running: d.count == 8 && d[d.startIndex + 7] & 1 != 0)
    case UInt8(ascii: "A") where d.count == 7:
        return .ack(d.le(UInt32.self, at: 3))
    case UInt8(ascii: "Y") where d.count >= 5:
        let index = Int(d[d.startIndex + 3]), count = Int(d[d.startIndex + 4])
        if count == 0 { return .entry(index: 0, count: 0, nil) }
        guard d.count == 14 else { return nil }
        return .entry(index: index, count: count, Entry(
            start: d.le(UInt32.self, at: 5), durS: Int(d.le(UInt16.self, at: 9)), day: Int(d[d.startIndex + 11]),
            done: Int(d[d.startIndex + 12]), planned: Int(d[d.startIndex + 13])))
    default:
        return nil
    }
}
