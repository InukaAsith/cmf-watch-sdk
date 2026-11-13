import Foundation
import Testing
@testable import WireKit

func result(_ entries: [Entry], version: UInt32) -> SyncResult {
    SyncResult(newEntries: entries, pushed: nil, heldBack: false, watchVersion: version)
}

@Test func resolvesDayNameFromPushedProgram() {
    let h = merge([], result([e1, e2], version: 42), pushed: [42: ["Верх", "Ноги"]])
    #expect(h.map(\.dayName) == ["Ноги", "Верх"], "newest first, names of the program the watch had")
}

@Test func unknownVersionFallsBack() {
    #expect(merge([], result([e2], version: 9), pushed: [42: ["Верх"]]).map(\.dayName) == ["День 2"])
    #expect(merge([], result([e2], version: 42), pushed: [42: ["Верх"]]).map(\.dayName) == ["День 2"])
}

@Test func duplicateStartIgnoredAndOldNamesKept() {
    let first = merge([], result([e1], version: 42), pushed: [42: ["Верх", "Ноги"]])
    let again = merge(first, result([e1, e3], version: 43), pushed: [43: ["Другое"]])
    #expect(again.map(\.start) == [300, 100])
    #expect(again.last?.dayName == "Верх")
}

@Test func bytesLeftCountsUtf8() {
    #expect(bytesLeft("abc") == 29)
    #expect(bytesLeft(String(repeating: "ж", count: 17)) == -2)
    #expect(bytesLeft("Ё") == 30)
}
