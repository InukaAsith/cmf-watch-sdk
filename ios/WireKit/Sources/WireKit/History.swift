import Foundation

/// A finished workout as the phone keeps it, forever. The day's name is fixed at sync time: the watch only knows
/// the day's index, and the program may change later.
public struct HistoryEntry: Codable, Hashable, Identifiable, Sendable {
    public var start: UInt32
    public var durS: Int
    public var dayName: String
    public var done: Int
    public var planned: Int
    /// Sets and heart rate, when the watch still had them at a sync.
    public var trace: Trace?
    /// The day's exercise names when it was done (the trace's `ex` indexes them).
    public var exercises: [String]?
    public var id: UInt32 { start }

    public init(start: UInt32, durS: Int, dayName: String, done: Int, planned: Int) {
        (self.start, self.durS, self.dayName, self.done, self.planned) = (start, durS, dayName, done, planned)
    }
    public var date: Date { Entry(start: start, durS: durS, day: 0, done: done, planned: planned).date }
}

/// Day names of each program version the phone pushed (version → names), to name history entries.
public typealias PushedPrograms = [UInt32: [String]]

/// `history` plus the sync's new entries (by `start`, newest first), named from the program the watch had
/// (`exercises`: version → each day's exercise names).
public func merge(_ history: [HistoryEntry], _ r: SyncResult, pushed: PushedPrograms,
                  exercises: [UInt32: [[String]]] = [:]) -> [HistoryEntry] {
    let known = Set(history.map(\.start))
    let names = pushed[r.watchVersion]
    let fresh = r.newEntries.filter { !known.contains($0.start) }.map { e in
        var h = HistoryEntry(start: e.start, durS: e.durS,
                             dayName: names.flatMap { e.day < $0.count ? $0[e.day] : nil } ?? "День \(e.day + 1)",
                             done: e.done, planned: e.planned)
        h.exercises = exercises[r.watchVersion].flatMap { e.day < $0.count ? $0[e.day] : nil }
        return h
    }
    var all = (history + fresh).sorted { $0.start > $1.start }
    if let (start, t) = r.trace, let i = all.firstIndex(where: { $0.start == start }) {
        all[i].trace = t
    }
    return all
}

/// Bytes a name or weight still has before the watch cuts it (32 B UTF-8, Ё counted as Е).
public func bytesLeft(_ s: String, _ max: Int = nameMax) -> Int {
    max - Data(s.replacingOccurrences(of: "Ё", with: "Е").replacingOccurrences(of: "ё", with: "е").utf8).count
}
