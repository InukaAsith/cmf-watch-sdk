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
public func traceQuery() -> Data { magic + Data("T".utf8) + Data([0]) }

/// A set of the trace: the exercise and the seconds from the workout's start to its "Сделал".
public struct SetMark: Codable, Hashable, Sendable {
    public var ex: Int
    public var t: Int
    public init(ex: Int, t: Int) { (self.ex, self.t) = (ex, t) }
}

/// The last workout's sets and heart rate (bpm per `step` seconds from the start, 0 = no reading), docs/workout.md.
public struct Trace: Codable, Hashable, Sendable {
    public static let step = 10
    public var sets: [SetMark]
    public var hr: [UInt8]
    public init(sets: [SetMark], hr: [UInt8]) { (self.sets, self.hr) = (sets, hr) }
}

/// `n_sets u8, (ex u8, t u16le) × n, n_hr u16le, bpm × n`.
public func parseTrace(_ d: Data) -> Trace? {
    let b = [UInt8](d)
    guard let n = b.first, b.count >= 1 + Int(n) * 3 + 2 else { return nil }
    let sets = (0..<Int(n)).map { i in SetMark(ex: Int(b[1 + i * 3]), t: Int(b[2 + i * 3]) | Int(b[3 + i * 3]) << 8) }
    let at = 1 + Int(n) * 3
    let nhr = Int(b[at]) | Int(b[at + 1]) << 8
    guard b.count == at + 2 + nhr else { return nil }
    return Trace(sets: sets, hr: Array(b[(at + 2)...]))
}

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
    /// P: live state of the workout that started at `start` on `day` (bpm 0 = none, ex nil = no exercise open,
    /// exDone: sets of `ex`, restLeft: seconds of rest left, 0 = not resting).
    case pulse(start: UInt32, day: Int, bpm: Int, ex: Int?, done: Int, exDone: Int = 0, restLeft: Int = 0)
    /// X: part `index` of `count` of the trace of the workout that started at `start`.
    case trace(index: Int, count: Int, start: UInt32, Data)
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
    case UInt8(ascii: "P") where d.count == 11 || d.count >= 14: // 11: workout v3; 19: + heart-rate diagnostics
        let ex = Int(d[d.startIndex + 9]), long = d.count >= 14
        return .pulse(start: d.le(UInt32.self, at: 3), day: Int(d[d.startIndex + 7]), bpm: Int(d[d.startIndex + 8]),
                      ex: ex == 0xFF ? nil : ex, done: Int(d[d.startIndex + 10]),
                      exDone: long ? Int(d[d.startIndex + 11]) : 0, restLeft: long ? Int(d.le(UInt16.self, at: 12)) : 0)
    case UInt8(ascii: "X") where d.count >= 9:
        return .trace(index: Int(d[d.startIndex + 3]), count: Int(d[d.startIndex + 4]), start: d.le(UInt32.self, at: 5),
                      Data(d.dropFirst(9)))
    default:
        return nil
    }
}
