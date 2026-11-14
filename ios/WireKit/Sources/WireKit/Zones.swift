import Foundation

/// Heart-rate zones as the training articles define them (Garmin, Polar): the maximum by age (Tanaka, 208 − 0.7 ×
/// age), five zones from 50 % of it in steps of 10 %; with a resting rate the percentages are of the reserve
/// (Karvonen). Zone 1 recovery, 2 easy, 3 aerobic, 4 threshold, 5 maximum.
public struct HeartZones: Codable, Hashable, Sendable {
    public var age: Int
    public var resting: Int?

    public init(age: Int, resting: Int? = nil) { (self.age, self.resting) = (age, resting) }

    public var max: Int { Int((208 - 0.7 * Double(age)).rounded()) }

    /// The lowest bpm of zones 1...5.
    public var lows: [Int] {
        let top = Double(max)
        return [0.5, 0.6, 0.7, 0.8, 0.9].map { p in
            let low = resting.map { r in Double(r) + p * (top - Double(r)) } ?? p * top
            return Int(low.rounded())
        }
    }

    /// 1...5, 0 below zone 1 or without a reading.
    public func zone(_ bpm: Int) -> Int { bpm <= 0 ? 0 : lows.filter { bpm >= $0 }.count }
}

/// Z: the zones' lower bounds for the watch.
public func zonesMessage(_ z: HeartZones) -> Data {
    magic + Data("Z".utf8) + Data([0]) + Data(z.lows.map { UInt8(clamping: $0) })
}
