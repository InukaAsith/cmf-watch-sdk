import Foundation

// The workout program and its encoding for the watch (src/wp2apps/workout.py `encode`, docs/workout.md).

public let nameMax = 32, weightMax = 32, programMax = 2048, daysMax = 7, exercisesMax = 12, restDefault = 90

public struct Exercise: Codable, Hashable, Sendable {
    public var name: String
    public var sets: Int
    public var repsMin: Int
    public var repsMax: Int
    public var weight: String
    public var restS: Int

    public init(name: String, sets: Int, repsMin: Int, repsMax: Int, weight: String, restS: Int = restDefault) {
        (self.name, self.sets, self.repsMin, self.repsMax, self.weight, self.restS) =
            (name, sets, repsMin, repsMax, weight, restS)
    }
}

public struct Day: Codable, Hashable, Sendable {
    public var name: String
    public var exercises: [Exercise]

    public init(name: String, exercises: [Exercise]) {
        (self.name, self.exercises) = (name, exercises)
    }
}

public enum ProgramError: Error, Equatable {
    case days(Int)
    case exercises(day: String, Int)
    case range(String)
    case size(Int)
}

/// At most `max` bytes of UTF-8 cut on a character boundary, Ё/ё as Е/е (the system font lacks them).
public func clip(_ s: String, _ max: Int) -> Data {
    var out = Data()
    for ch in s.replacingOccurrences(of: "Ё", with: "Е").replacingOccurrences(of: "ё", with: "е").unicodeScalars {
        let b = Data(String(ch).utf8)
        if out.count + b.count > max { break }
        out += b
    }
    return out
}

func text(_ s: String, _ max: Int) -> Data {
    let b = clip(s, max)
    return Data([UInt8(b.count)]) + b
}

public func encodeProgram(_ days: [Day]) throws -> Data {
    guard (1...daysMax).contains(days.count) else { throw ProgramError.days(days.count) }
    var out = Data([UInt8(days.count)])
    for d in days {
        guard (1...exercisesMax).contains(d.exercises.count) else {
            throw ProgramError.exercises(day: d.name, d.exercises.count)
        }
        out += text(d.name, nameMax) + Data([UInt8(d.exercises.count)])
        for e in d.exercises {
            guard (1...255).contains(e.sets), 0 <= e.repsMin, e.repsMin <= e.repsMax, e.repsMax <= 255,
                  (0...0xFFFF).contains(e.restS) else { throw ProgramError.range(e.name) }
            out += text(e.name, nameMax) + Data([UInt8(e.sets), UInt8(e.repsMin), UInt8(e.repsMax)])
            out.le(UInt16(e.restS))
            out += text(e.weight, weightMax)
        }
    }
    guard out.count <= programMax else { throw ProgramError.size(out.count) }
    return out
}

public func programVersion(_ bytes: Data) -> UInt32 { crc32(bytes) }

/// The JSON of `wp2apps workout push <dir> --dry-run`: the one-time import of the Markdown program.
public func importDryRun(_ json: Data) throws -> [Day] {
    struct Ex: Decodable { let name: String; let sets: Int; let reps: [Int]; let weight: String; let rest_s: Int }
    struct D: Decodable { let name: String; let exercises: [Ex] }
    struct Top: Decodable { let days: [D] }
    return try JSONDecoder().decode(Top.self, from: json).days.map { d in
        Day(name: d.name, exercises: d.exercises.map {
            Exercise(name: $0.name, sets: $0.sets, repsMin: $0.reps.first ?? 0, repsMax: $0.reps.last ?? 0,
                     weight: $0.weight, restS: $0.rest_s)
        })
    }
}
