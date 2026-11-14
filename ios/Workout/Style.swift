import SwiftUI

extension View {
    /// A rounded tile on the grouped background, like a List row but free-standing.
    func card(padding: CGFloat = 16) -> some View {
        self.padding(padding)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(.secondarySystemGroupedBackground), in: .rect(cornerRadius: 24, style: .continuous))
    }
}

/// A progress ring in the accent color.
struct Ring: View {
    let value: Double
    var width: CGFloat = 6

    var body: some View {
        ZStack {
            Circle().stroke(Color.accentColor.opacity(0.15), lineWidth: width)
            Circle().trim(from: 0, to: min(max(value, 0), 1))
                .stroke(Color.accentColor, style: StrokeStyle(lineWidth: width, lineCap: .round))
                .rotationEffect(.degrees(-90))
        }
        .padding(width / 2)
    }
}

/// A big number with its unit under it.
struct Metric: View {
    let value: String
    let unit: LocalizedStringKey

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(value).font(.system(.largeTitle, design: .rounded, weight: .bold)).monospacedDigit()
            Text(unit).font(.subheadline).foregroundStyle(.secondary)
        }
    }
}

/// Monday-first calendar for week maths, whatever the region says.
let weekCalendar: Calendar = {
    var c = Calendar.current
    c.firstWeekday = 2
    return c
}()
