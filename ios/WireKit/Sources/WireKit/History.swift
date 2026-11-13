import Foundation

/// A finished workout as the phone keeps it, forever. The day's name is fixed at sync time: the watch only knows
/// the day's index, and the program may change later.
public struct HistoryEntry: Codable, Hashable, Identifiable, Sendable {
    public var start: UInt32
    public var durS: Int
    public var dayName: String
    public var done: Int
    public var planned: Int
    public var id: UInt32 { start }

    public init(start: UInt32, durS: Int, dayName: String, done: Int, planned: Int) {
        (self.start, self.durS, self.dayName, self.done, self.planned) = (start, durS, dayName, done, planned)
    }
    public var date: Date { Entry(start: start, durS: durS, day: 0, done: done, planned: planned).date }
}

/// Day names of each program version the phone pushed (version → names), to name history entries.
public typealias PushedPrograms = [UInt32: [String]]

/// `history` plus the sync's new entries (by `start`, newest first), named from the program the watch had.
public func merge(_ history: [HistoryEntry], _ r: SyncResult, pushed: PushedPrograms) -> [HistoryEntry] {
    let known = Set(history.map(\.start))
    let names = pushed[r.watchVersion]
    let fresh = r.newEntries.filter { !known.contains($0.start) }.map { e in
        HistoryEntry(start: e.start, durS: e.durS,
                     dayName: names.flatMap { e.day < $0.count ? $0[e.day] : nil } ?? "День \(e.day + 1)",
                     done: e.done, planned: e.planned)
    }
    return (history + fresh).sorted { $0.start > $1.start }
}

/// Bytes a name or weight still has before the watch cuts it (32 B UTF-8, Ё counted as Е).
public func bytesLeft(_ s: String, _ max: Int = nameMax) -> Int {
    max - Data(s.replacingOccurrences(of: "Ё", with: "Е").replacingOccurrences(of: "ё", with: "е").utf8).count
}
