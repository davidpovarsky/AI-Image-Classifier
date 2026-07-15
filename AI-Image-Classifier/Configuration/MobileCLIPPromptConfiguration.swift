import Foundation

nonisolated enum MobileCLIPPromptConfiguration {
    static let prompts: [PersonVisualClass: [String]] = [
        .woman: [
            "a photo of a woman", "a photo of a female person", "a photo of an adult woman",
            "a photo of a girl", "a photo of a female child", "a full body photo of a woman",
            "a cropped photo of a woman", "a person who appears female"
        ],
        .man: [
            "a photo of a man", "a photo of a male person", "a photo of an adult man",
            "a photo of a boy", "a photo of a male child", "a full body photo of a man",
            "a cropped photo of a man", "a person who appears male"
        ],
        .uncertain: [
            "a photo of a person whose gender is unclear", "a photo of an androgynous person",
            "a photo of a partially visible person", "a photo of a person with an obscured face",
            "a photo where the person's gender cannot be determined"
        ],
        .notPerson: [
            "a photo with no person", "an object", "an animal", "a landscape", "a building", "a vehicle"
        ]
    ]

    static var configurationHash: String {
        PersonVisualClass.allCases
            .flatMap { prompts[$0, default: []] }
            .joined(separator: "\n")
    }
}
