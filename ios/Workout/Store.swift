import Foundation
import Observation
import WireKit

/// A day and its exercises as the editor holds them: WireKit's types plus stable ids for lists.
struct DayItem: Codable, Identifiable, Hashable {
    var id = UUID()
    var name: String
    var exercises: [ExerciseItem]

    var day: Day { Day(name: name, exercises: exercises.map(\.exercise)) }
}

struct ExerciseItem: Codable, Identifiable, Hashable {
    var id = UUID()
    var exercise: Exercise

    var summary: String {
        let e = exercise
        let reps = e.repsMin == e.repsMax ? "\(e.repsMin)" : "\(e.repsMin)–\(e.repsMax)"
        return ([ "\(e.sets)×\(reps)", e.weight.isEmpty ? nil : e.weight, "отдых \(restText(e.restS))"] as [String?])
            .compactMap { $0 }.joined(separator: " · ")
    }
}

/// 1 упражнение, 2 упражнения, 5 упражнений.
func plural(_ n: Int, _ one: String, _ few: String, _ many: String) -> String {
    let (d, h) = (n % 10, n % 100)
    return "\(n) " + (d == 1 && h != 11 ? one : (2...4).contains(d) && !(12...14).contains(h) ? few : many)
}

func restText(_ s: Int) -> String { String(format: "%d:%02d", s / 60, s % 60) }

/// Everything the app keeps: the program (the only copy), the history (forever), the day names of each pushed
/// version and the cached pairing secret. One JSON file in Application Support.
@MainActor @Observable
final class AppStore {
    struct Saved: Codable {
        var days: [DayItem] = []
        var history: [HistoryEntry] = []
        var pushed: PushedPrograms = [:]
        var watchVersion: UInt32?
        var heldBack = false
        var lastSync: Date?
    }

    private(set) var saved: Saved
    private let url: URL

    init(url: URL = URL.applicationSupportDirectory.appending(path: "workout.json")) {
        self.url = url
        saved = (try? JSONDecoder().decode(Saved.self, from: Data(contentsOf: url))) ?? Saved()
    }

    var days: [DayItem] {
        get { saved.days }
        set { saved.days = newValue; save() }
    }

    var history: [HistoryEntry] { saved.history }
    var lastSync: Date? { saved.lastSync }
    var heldBack: Bool { saved.heldBack }
    var watchVersion: UInt32? { saved.watchVersion }

    /// The encoded program, or why the watch would not take it.
    var encoded: Result<Data, ProgramError> {
        Result { try encodeProgram(days.map(\.day)) }.mapError { $0 as! ProgramError }
    }

    /// The watch has an older program (or none) than the phone.
    var pending: Bool {
        guard case let .success(p) = encoded else { return false }
        return saved.watchVersion != programVersion(p)
    }

    func apply(_ r: SyncResult) {
        saved.history = merge(saved.history, r, pushed: saved.pushed)
        saved.watchVersion = r.pushed ?? r.watchVersion
        saved.heldBack = r.heldBack
        saved.lastSync = .now
        save()
    }

    /// Before a push: remember the day names of this version (history names its entries from them).
    func willPush(_ program: Data) {
        saved.pushed[programVersion(program)] = days.map(\.name)
        save()
    }

    func replaceProgram(_ imported: [Day]) {
        days = imported.map { d in DayItem(name: d.name, exercises: d.exercises.map { ExerciseItem(exercise: $0) }) }
    }

    #if DEBUG
    /// Made-up program and history for screenshots: launch with `-demo`.
    func loadDemo() {
        let ex = { (n: String, s: Int, a: Int, b: Int, w: String, r: Int) in
            ExerciseItem(exercise: Exercise(name: n, sets: s, repsMin: a, repsMax: b, weight: w, restS: r))
        }
        saved.days = [
            DayItem(name: "Жим и присед", exercises: [ex("Жим лёжа", 4, 5, 8, "60 кг", 120), ex("Присед", 4, 5, 5, "80 кг", 150),
                                                     ex("Скручивания", 3, 20, 20, "свой вес", 60)]),
            DayItem(name: "Тяга", exercises: [ex("Становая тяга", 3, 5, 5, "90 кг", 180), ex("Подтягивания", 3, 6, 10, "", 90)]),
            DayItem(name: "Ноги", exercises: [ex("Выпады", 3, 10, 12, "2×10 кг", 90)]),
        ]
        let base = UInt32(Date.now.timeIntervalSince1970 - 946_684_800 + Double(TimeZone.current.secondsFromGMT()))
        saved.history = (0..<9).map { i in
            HistoryEntry(start: base - UInt32(i) * 2 * 86_400 - 3600, durS: 3300 + i * 97,
                         dayName: saved.days[i % 3].name, done: 11 - i % 2, planned: 11)
        }
        saved.lastSync = .now.addingTimeInterval(-300)
        save()
    }
    #endif

    private func save() {
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? JSONEncoder().encode(saved).write(to: url, options: .atomic)
    }
}

extension ProgramError {
    var text: String {
        switch self {
        case let .days(n): n == 0 ? "Добавьте хотя бы один день" : "Дней \(n), часы берут до \(daysMax)"
        case let .exercises(day, n): n == 0 ? "В дне «\(day)» нет упражнений" :
            "В дне «\(day)» \(n) упражнений, часы берут до \(exercisesMax)"
        case let .range(name): "«\(name)»: проверьте подходы и повторы"
        case let .size(n): "Программа \(n) Б, часы берут \(programMax) Б: сократите названия"
        }
    }
}
