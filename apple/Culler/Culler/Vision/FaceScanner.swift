import CoreGraphics
import Foundation
import ImageIO
import Vision

/// Apple Vision face facts (PRD §5.3): face count and an eyes-open ratio
/// derived from eye landmark geometry. Runs on cached thumbnails.
enum FaceScanner {
    struct Facts: Sendable {
        let faceCount: Int
        let eyesOpenRatio: Double
    }

    static func analyze(thumbPath: String) -> Facts? {
        let url = URL(fileURLWithPath: thumbPath)
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
            let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
        else { return nil }

        let request = VNDetectFaceLandmarksRequest()
        let handler = VNImageRequestHandler(cgImage: image, options: [:])
        guard (try? handler.perform([request])) != nil else { return nil }
        let faces = request.results ?? []
        guard !faces.isEmpty else {
            return Facts(faceCount: 0, eyesOpenRatio: 0)
        }

        var measured = 0
        var open = 0
        for face in faces {
            guard let landmarks = face.landmarks else { continue }
            for eye in [landmarks.leftEye, landmarks.rightEye].compactMap({ $0 }) {
                if let isOpen = eyeLooksOpen(eye) {
                    measured += 1
                    open += isOpen ? 1 : 0
                }
            }
        }
        // No measurable eyes (tiny faces): neutral rather than penalizing.
        let ratio = measured > 0 ? Double(open) / Double(measured) : 0.5
        return Facts(faceCount: faces.count, eyesOpenRatio: ratio)
    }

    /// Eye aspect ratio heuristic: an open eye's landmark bounding box is
    /// meaningfully taller relative to its width than a closed one's.
    private static func eyeLooksOpen(_ eye: VNFaceLandmarkRegion2D) -> Bool? {
        let points = eye.normalizedPoints
        guard points.count >= 4 else { return nil }
        let xs = points.map(\.x)
        let ys = points.map(\.y)
        let width = (xs.max() ?? 0) - (xs.min() ?? 0)
        let height = (ys.max() ?? 0) - (ys.min() ?? 0)
        guard width > 0 else { return nil }
        return (height / width) > 0.18
    }
}
