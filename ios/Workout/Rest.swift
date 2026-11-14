import UserNotifications

/// The end of a rest as a time-sensitive local notification, so it rings with the phone locked.
enum Rest {
    private static let id = "rest"

    static func ask() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    static func schedule(_ end: Date?, name: String) {
        cancel()
        guard let end, end.timeIntervalSinceNow > 1 else { return }
        let c = UNMutableNotificationContent()
        c.title = String(localized: "Отдых закончился")
        c.body = name.isEmpty ? String(localized: "Следующий подход") : String(localized: "Следующий подход: \(name)")
        c.sound = .default
        c.interruptionLevel = .timeSensitive
        let t = UNTimeIntervalNotificationTrigger(timeInterval: end.timeIntervalSinceNow, repeats: false)
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: id, content: c, trigger: t))
    }

    static func cancel() {
        UNUserNotificationCenter.current().removePendingNotificationRequests(withIdentifiers: [id])
    }
}
