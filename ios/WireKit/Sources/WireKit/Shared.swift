import Foundation

// One workout on the watch and the phone (docs/specs/2026-11-14-shared-workout-design.md): a start, a day and a
// grow-only set of set events; the phone holds the rules, the watch applies D, F and H.

/// `date` as the watch counts time: local seconds since 2000-01-01 (the inverse of `Entry.date`).
public func watchSeconds(_ date: Date = .now) -> UInt32 {
    let local = date.timeIntervalSince1970 + Double(TimeZone.current.secondsFromGMT(for: date))
    return UInt32(max(0, local - 946_684_800))
}

/// The open workout as the phone keeps it.
public struct OpenWorkout: Codable, Hashable, Sendable {
    public var start: UInt32
    public var day: Int
    /// The program version it runs on: the watch shares it only with the same one.
    public var version: UInt32
    /// In time order, at most one per (ex, t).
    public var sets: [SetMark]
    /// Sets taken back: a merge never brings them in again.
    public var removed: [SetMark]?

    public init(start: UInt32, day: Int, version: UInt32, sets: [SetMark] = []) {
        (self.start, self.day, self.version, self.sets) = (start, day, version, sets)
    }

    public func done(_ ex: Int) -> Int { sets.filter { $0.ex == ex }.count }

    /// Adds the events it lacks, each exercise up to `planned(ex)` sets; returns the ones added.
    @discardableResult
    public mutating func union(_ other: [SetMark], planned: (Int) -> Int) -> [SetMark] {
        var added: [SetMark] = []
        for s in other.sorted(by: { $0.t < $1.t })
        where !sets.contains(s) && !(removed ?? []).contains(s) && done(s.ex) < planned(s.ex) {
            sets.append(s)
            added.append(s)
        }
        sets.sort { $0.t < $1.t }
        return added
    }

    /// Takes the newest set back; returns it.
    public mutating func undoLast() -> SetMark? {
        guard let s = sets.popLast() else { return nil }
        removed = (removed ?? []) + [s]
        return s
    }
}

/// D: a set (ex nil: the workout only runs).
public func setMessage(_ w: OpenWorkout, _ s: SetMark?) -> Data {
    var d = magic + Data("D".utf8) + Data([0])
    d.le(w.start)
    d.append(UInt8(w.day))
    d.le(w.version)
    d.append(s.map { UInt8($0.ex) } ?? 0xFF)
    d.le(UInt16(clamping: s?.t ?? 0))
    return d
}

/// U: take a set back.
public func unsetMessage(_ w: OpenWorkout, _ s: SetMark) -> Data {
    var d = magic + Data("U".utf8) + Data([0])
    d.le(w.start)
    d.append(UInt8(s.ex))
    d.le(UInt16(clamping: s.t))
    return d
}

/// F: finish the workout that started at `start`.
public func finishMessage(start: UInt32, end: UInt32) -> Data {
    var d = magic + Data("F".utf8) + Data([0])
    d.le(start)
    d.le(end)
    return d
}

/// H: a workout done on the phone alone.
public func entryMessage(_ e: Entry) -> Data {
    var d = magic + Data("H".utf8) + Data([0])
    d.le(e.start)
    d.le(UInt16(clamping: e.durS))
    d.append(contentsOf: [UInt8(clamping: e.day), UInt8(clamping: e.done), UInt8(clamping: e.planned)])
    return d
}

/// The watch's open workout, from its P.
public struct WatchWorkout: Equatable, Sendable {
    public var start: UInt32, day: Int, done: Int
    public init(start: UInt32, day: Int, done: Int) { (self.start, self.day, self.done) = (start, day, done) }
}

/// What a meeting of the phone's and the watch's open workouts calls for.
public enum Meeting: Equatable, Sendable {
    case nothing
    /// The watch has none: it adopts the phone's (D per set).
    case pushOwn
    /// The same workout: fetch the watch's sets (X), send it the phone's it lacks.
    case exchange
    /// The phone has none: take the watch's (X).
    case adoptWatch
    /// Two workouts, the phone's has more sets (or as many, started earlier): F the watch's, then pushOwn.
    case replaceWatch
    /// Two workouts, the watch's wins: the phone finishes its own into the history (H), then adoptWatch.
    case yieldToWatch
    /// Different programs: no sharing, each finishes its own (the phone's goes to the watch with H).
    case independent
}

public func meet(phone: OpenWorkout?, watch: WatchWorkout?, watchVersion: UInt32) -> Meeting {
    switch (phone, watch) {
    case (nil, nil): .nothing
    case let (p?, nil): p.version == watchVersion ? .pushOwn : .independent
    case (nil, _?): .adoptWatch
    case let (p?, w?):
        if p.start == w.start { .exchange }
        else if p.version != watchVersion { .independent }
        else if p.sets.count > w.done || (p.sets.count == w.done && p.start < w.start) { .replaceWatch }
        else { .yieldToWatch }
    }
}
