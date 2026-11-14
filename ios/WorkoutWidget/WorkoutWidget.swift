import ActivityKit
import AppIntents
import SwiftUI
import WidgetKit

@main
struct WorkoutWidgets: WidgetBundle {
    var body: some Widget { WorkoutLive() }
}

/// The open workout: the elapsed time or the rest left, the current set, "Сделал" and "Завершить".
struct WorkoutLive: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: WorkoutAttributes.self) { ctx in
            LockScreen(a: ctx.attributes, s: ctx.state)
                .padding(16)
                .activityBackgroundTint(Color.black.opacity(0.75))
                .activitySystemActionForegroundColor(.white)
        } dynamicIsland: { ctx in
            DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    Label(ctx.attributes.day, systemImage: "figure.strengthtraining.traditional").font(.caption).lineLimit(1)
                }
                DynamicIslandExpandedRegion(.trailing) { Clock(a: ctx.attributes, s: ctx.state).font(.title3.bold()) }
                DynamicIslandExpandedRegion(.bottom) {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(ctx.state.exercise.isEmpty ? "Все подходы сделаны" : "\(ctx.state.exercise) · \(ctx.state.set)")
                            .font(.subheadline).lineLimit(1)
                        Buttons(s: ctx.state)
                    }
                }
            } compactLeading: {
                Image(systemName: "figure.strengthtraining.traditional").foregroundStyle(.orange)
            } compactTrailing: {
                Clock(a: ctx.attributes, s: ctx.state).frame(maxWidth: 52).font(.caption.monospacedDigit())
            } minimal: {
                Image(systemName: "figure.strengthtraining.traditional").foregroundStyle(.orange)
            }
        }
    }
}

/// The rest left while resting, else the time since the start.
private struct Clock: View {
    let a: WorkoutAttributes, s: WorkoutAttributes.ContentState

    var body: some View {
        if let end = s.restEnd, end > .now {
            Text(timerInterval: Date.now...end, countsDown: true).foregroundStyle(.orange).monospacedDigit()
        } else {
            Text(a.start, style: .timer).monospacedDigit()
        }
    }
}

private struct Buttons: View {
    let s: WorkoutAttributes.ContentState

    var body: some View {
        HStack {
            if !s.exercise.isEmpty {
                Button(intent: DoneSetIntent()) { Label("Сделал", systemImage: "checkmark").frame(maxWidth: .infinity) }
                    .tint(.orange)
            }
            Button(intent: FinishWorkoutIntent()) { Label("Завершить", systemImage: "flag.checkered").frame(maxWidth: .infinity) }
                .tint(.gray)
        }
        .buttonStyle(.borderedProminent)
        .font(.subheadline.bold())
    }
}

private struct LockScreen: View {
    let a: WorkoutAttributes, s: WorkoutAttributes.ContentState

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(a.day).font(.headline)
                    Text(s.exercise.isEmpty ? "Все подходы сделаны — завершите тренировку" : "\(s.exercise) · \(s.set)")
                        .font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer()
                VStack(alignment: .trailing, spacing: 2) {
                    Clock(a: a, s: s).font(.title2.bold())
                    Text(s.restEnd.map { $0 > .now } == true ? "отдых" : "\(s.done) из \(s.planned)")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
            ProgressView(value: Double(s.done), total: Double(max(s.planned, 1))).tint(.orange)
            Buttons(s: s)
        }
        .foregroundStyle(.white)
    }
}
