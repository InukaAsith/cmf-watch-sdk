> **Works only with the iPhone app from the [wp2-apps repository](https://github.com/cmf-tech-community/wp2-apps/tree/main/ios).** The program comes from it — without it the watch has nothing to show. There's no App Store build: you build and install it yourself with Xcode (`scripts/ios-install`).

Your training program on the watch: pick a day (a tap previews it, **Начать** starts it), tick off sets, and a rest timer buzzes when it's time for the next one. Heart rate shows while you train, and the watch offers the next day after each workout.

- Progress survives the screen going off or the app being closed
- The button asks before it leaves a workout in progress
- A set can be undone
- The program and the history sync with the iPhone app; a workout can be started and walked on the phone too

Limits: 7 days, 12 exercises a day. Sets are ticked as done — reps and weights aren't recorded.

## Requirements

- The [iPhone app](https://github.com/cmf-tech-community/wp2-apps/tree/main/ios) from the repository, built and installed with Xcode
- The [patched firmware](/firmware)
- The [App Loader](/firmware) of 2026-11-14 or newer (heart rate, the card lists)

Open it from the **Apps** row at the end of the watch's app list (list style, not the waterfall).
