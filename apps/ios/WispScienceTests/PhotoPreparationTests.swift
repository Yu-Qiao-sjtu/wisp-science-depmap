import XCTest
import UIKit
@testable import WispScience

@MainActor final class PhotoPreparationTests: XCTestCase {
    func testCameraSizedImageFitsEncryptedUploadBudget() throws {
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        let image = UIGraphicsImageRenderer(size: CGSize(width: 4032, height: 3024), format: format).image { context in
            UIColor.systemTeal.setFill()
            context.fill(CGRect(x: 0, y: 0, width: 4032, height: 3024))
        }
        let data = try PhotoPreparation.jpeg(XCTUnwrap(image.pngData()))
        XCTAssertLessThanOrEqual(data.count, 560 * 1024)
        let decoded = try XCTUnwrap(UIImage(data: data))
        XCTAssertLessThanOrEqual(max(decoded.size.width, decoded.size.height), 1600)
        XCTAssertEqual(decoded.size.width / decoded.size.height, 4 / 3, accuracy: 0.01)
    }
    func testInvalidImageIsRejected() {
        XCTAssertThrowsError(try PhotoPreparation.jpeg(Data("not an image".utf8)))
    }
}
