import Foundation
import Testing
@testable import WireKit

/// Against the real watch app in the emulator through `wp2apps bridge` (tests/test_ios_e2e.py):
/// a K session, no bind.
@Test(.enabled(if: ProcessInfo.processInfo.environment["WP2_TCP_PORT"] != nil))
func syncOverTcp() async throws {
    let port = UInt16(ProcessInfo.processInfo.environment["WP2_TCP_PORT"]!)!
    let ch = SimChannel(port: port)
    defer { ch.close() }
    let days = try JSONDecoder().decode([Day].self, from: JSONEncoder().encode(Vectors.shared.days))
    let program = try encodeProgram(days)
    let first = try await sync(channel: ch, program: program, knownStarts: [], askWait: .seconds(20))
    #expect(first.pushed == programVersion(program))
    #expect(first.newEntries.isEmpty)
    let second = try await sync(channel: ch, program: program, knownStarts: [], askWait: .seconds(20))
    #expect(second.watchVersion == programVersion(program) && second.pushed == nil)
}
