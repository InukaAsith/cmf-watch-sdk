import ActivityKit
import AppIntents
import Foundation

/// The open workout on the lock screen and in the Dynamic Island (app and widget extension share this file).
struct WorkoutAttributes: ActivityAttributes {
    struct ContentState: Codable, Hashable {
        var done: Int
        var planned: Int
        /// The current exercise ("" when all sets are done).
        var exercise: String
        /// "Подход 2 из 4"
        var set: String
        var restEnd: Date?
    }

    var day: String
    var start: Date
}

/// What the Live Activity's buttons do; the app sets them at launch (the intents run in the app's process).
@MainActor enum WorkoutActions {
    static var done: (() -> Void)?
    static var finish: (() -> Void)?
}

struct DoneSetIntent: LiveActivityIntent {
    static let title: LocalizedStringResource = "Сделал подход"
    init() {}
    @MainActor func perform() async throws -> some IntentResult {
        WorkoutActions.done?()
        return .result()
    }
}

struct FinishWorkoutIntent: LiveActivityIntent {
    static let title: LocalizedStringResource = "Завершить тренировку"
    init() {}
    @MainActor func perform() async throws -> some IntentResult {
        WorkoutActions.finish?()
        return .result()
    }
}
