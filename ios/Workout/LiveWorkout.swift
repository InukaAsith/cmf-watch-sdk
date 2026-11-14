import ActivityKit
import CoreLocation
import UserNotifications
import WireKit

/// Keeps the Live Activity in step with the open workout: started with it, updated on every change, ended with it.
@MainActor enum LiveWorkout {
    private static var last: WorkoutAttributes.ContentState?

    static func sync(_ store: AppStore) {
        guard ActivityAuthorizationInfo().areActivitiesEnabled else { return }
        let running = Activity<WorkoutAttributes>.activities
        guard let o = store.open, o.day < store.days.count else {
            endAll(except: nil)
            last = nil
            return
        }
        let day = store.days[o.day]
        let ex = store.currentExercise()
        let state = WorkoutAttributes.ContentState(
            done: o.sets.count, planned: day.exercises.map(\.exercise.sets).reduce(0, +),
            exercise: ex.map { day.exercises[$0].exercise.name } ?? "",
            set: ex.map { String(localized: "подход \(min(o.done($0) + 1, day.exercises[$0].exercise.sets)) из \(day.exercises[$0].exercise.sets)") } ?? "",
            restEnd: store.restEnd)
        let start = startDate(o)
        if let a = running.first(where: { $0.attributes.start == start }) {
            let id = a.id
            if state != last {
                // ActivityKit's Activity is not Sendable: look it up again off the main actor
                Task.detached {
                    for a in Activity<WorkoutAttributes>.activities where a.id == id {
                        await a.update(.init(state: state, staleDate: nil))
                    }
                }
            }
        } else {
            let new = try? Activity.request(attributes: WorkoutAttributes(day: day.name, start: start),
                                            content: .init(state: state, staleDate: nil))
            endAll(except: new?.id)
        }
        last = state
    }

    private static func endAll(except id: String?) {
        Task.detached {
            for a in Activity<WorkoutAttributes>.activities where a.id != id {
                await a.end(nil, dismissalPolicy: .immediate)
            }
        }
    }
}

/// Finishes the open workout when the phone leaves the place it started at (the gym): a 150 m circle around the
/// first fix after the start, watched with CLMonitor (relaunches the app in the background; needs "Always").
@MainActor final class GymGuard {
    private static let name = "gym"
    private static let radius: CLLocationDistance = 150
    /// A workout this young is not finished by a bad first fix.
    private static let minAge: TimeInterval = 5 * 60
    private let manager = CLLocationManager()
    // one CLMonitor per name per process: a second one with the same name throws (it crashed the app at launch)
    private let made = Task { @MainActor in await CLMonitor(GymGuard.name) }
    private var armedFor: UInt32?
    private let onLeave: () -> Void

    init(onLeave: @escaping () -> Void) {
        self.onLeave = onLeave
        Task { await listen() } // a relaunch for an exit event lands here
    }

    /// Arms the circle for a new open workout, disarms it without one.
    func sync(_ open: OpenWorkout?) {
        guard open?.start != armedFor else { return }
        armedFor = open?.start
        Task {
            let m = await monitor()
            await m.remove(Self.name)
            guard open != nil else { return }
            manager.requestAlwaysAuthorization()
            for try await u in CLLocationUpdate.liveUpdates() {
                guard let loc = u.location else { continue }
                if loc.horizontalAccuracy > 100 { continue } // indoors: wait for a usable fix
                await m.add(CLMonitor.CircularGeographicCondition(center: loc.coordinate, radius: Self.radius),
                            identifier: Self.name, assuming: .satisfied)
                break
            }
        }
    }

    private func monitor() async -> CLMonitor { await made.value }

    private func listen() async {
        let m = await monitor()
        do {
            for try await e in await m.events where e.identifier == Self.name && e.state == .unsatisfied {
                await m.remove(Self.name)
                armedFor = nil
                onLeave()
            }
        } catch {}
    }

    static func tooYoung(_ o: OpenWorkout) -> Bool { Date.now.timeIntervalSince(startDate(o)) < minAge }

    static func notifyFinished() {
        let c = UNMutableNotificationContent()
        c.title = String(localized: "Тренировка завершена")
        c.body = String(localized: "Вы ушли из зала: тренировка сохранена в истории.")
        c.sound = .default
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: "left-gym", content: c, trigger: nil))
    }
}
