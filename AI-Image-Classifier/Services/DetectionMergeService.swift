import Foundation

nonisolated struct DetectionMergeService: Sendable {
    static let intersectionOverUnionThreshold = 0.50

    func intersectionOverUnion(_ lhs: NormalizedBoundingBox, _ rhs: NormalizedBoundingBox) -> Double {
        let left = max(lhs.x, rhs.x)
        let top = max(lhs.y, rhs.y)
        let right = min(lhs.x + lhs.width, rhs.x + rhs.width)
        let bottom = min(lhs.y + lhs.height, rhs.y + rhs.height)
        let intersection = max(0, right - left) * max(0, bottom - top)
        let union = lhs.width * lhs.height + rhs.width * rhs.height - intersection
        return union > 0 ? intersection / union : 0
    }

    func merge(_ detections: [RawNudityDetection]) -> [MergedNudityDetection] {
        var groups: [[RawNudityDetection]] = []
        for detection in detections.sorted(by: { $0.confidence > $1.confidence }) {
            if let index = groups.firstIndex(where: { group in
                group.contains { candidate in
                    candidate.rawLabel == detection.rawLabel
                        && intersectionOverUnion(candidate.boundingBox, detection.boundingBox) >= Self.intersectionOverUnionThreshold
                }
            }) {
                groups[index].append(detection)
            } else {
                groups.append([detection])
            }
        }

        return groups.enumerated().map { index, group in
            let winner = group.max(by: { $0.confidence < $1.confidence })!
            let maximumIoU = group.indices.flatMap { first in
                group.indices.filter { $0 > first }.map { intersectionOverUnion(group[first].boundingBox, group[$0].boundingBox) }
            }.max() ?? 1
            return MergedNudityDetection(
                mergedDetectionId: "merged-nudity-\(index + 1)",
                rawLabel: winner.rawLabel,
                confidence: winner.confidence,
                boundingBox: winner.boundingBox,
                sources: Array(Set(group.map(\.source))).sorted { $0.rawValue < $1.rawValue },
                sourceDetectionIds: group.map(\.detectionId),
                sourceConfidences: group.map(\.confidence),
                personIds: Array(Set(group.compactMap(\.personId))).sorted(),
                merge: MergeEvidence(
                    algorithm: "sameLabelIoU",
                    threshold: Self.intersectionOverUnionThreshold,
                    maximumIoU: maximumIoU
                )
            )
        }
    }

    func assignedPersonIDs(
        for detection: MergedNudityDetection,
        people: [PersonDetectionEvidence]
    ) -> [String] {
        let centerX = detection.boundingBox.x + detection.boundingBox.width / 2
        let centerY = detection.boundingBox.y + detection.boundingBox.height / 2
        return people.compactMap { person in
            let box = DetectionMappingService().topLeft(fromVisionBottomLeft: person.boundingBox)
            let centerInside = centerX >= box.x && centerX <= box.x + box.width
                && centerY >= box.y && centerY <= box.y + box.height
            let overlap = intersectionArea(detection.boundingBox, box)
            let detectionArea = detection.boundingBox.width * detection.boundingBox.height
            return centerInside || (detectionArea > 0 && overlap / detectionArea >= 0.50) ? person.personId : nil
        }
    }

    private func intersectionArea(_ lhs: NormalizedBoundingBox, _ rhs: NormalizedBoundingBox) -> Double {
        max(0, min(lhs.x + lhs.width, rhs.x + rhs.width) - max(lhs.x, rhs.x))
            * max(0, min(lhs.y + lhs.height, rhs.y + rhs.height) - max(lhs.y, rhs.y))
    }
}
