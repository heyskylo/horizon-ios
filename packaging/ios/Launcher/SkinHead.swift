import UIKit

/// The face of the skin that belongs to a name: the Minecraft account with that name (Mojang's profile
/// lookup), its `textures` property (the session server's, as the game reads skins), and the skin's 8x8 face
/// with the hat layer over it. Nil for a name no account has, or with no network: the avatar shows the
/// name's letter then.
final class SkinHead: ObservableObject {
    @Published private(set) var face: UIImage?
    private var faces: [String: UIImage] = [:]
    private var revision = UUID()
    typealias Fetch = @MainActor (String) async -> UIImage?
    private let fetch: Fetch

    init(fetch: @escaping Fetch = { await SkinHead.fetch($0) }) {
        self.fetch = fetch
    }

    @MainActor
    func load(_ name: String?) async {
        revision = UUID()
        let requested = revision
        guard let name, !name.isEmpty else {
            face = nil
            return
        }
        let key = name.lowercased()
        if let known = faces[key] {
            face = known
            return
        }
        face = nil
        guard let found = await fetch(name), requested == revision, !Task.isCancelled else {
            return
        }
        faces[key] = found
        face = found
    }

    private static func fetch(_ name: String) async -> UIImage? {
        guard let escaped = name.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed),
              let profile = await json("https://api.mojang.com/users/profiles/minecraft/\(escaped)"),
              let id = profile["id"] as? String,
              let session = await json("https://sessionserver.mojang.com/session/minecraft/profile/\(id)"),
              let properties = session["properties"] as? [[String: Any]],
              let value = properties.first(where: { $0["name"] as? String == "textures" })?["value"] as? String,
              let decoded = Data(base64Encoded: value),
              let payload = (try? JSONSerialization.jsonObject(with: decoded)) as? [String: Any],
              let skin = ((payload["textures"] as? [String: Any])?["SKIN"] as? [String: Any])?["url"] as? String,
              // textures.minecraft.net serves both; App Transport Security wants https.
              let url = URL(string: skin.replacingOccurrences(of: "http://", with: "https://")),
              let downloaded = try? await URLSession.shared.data(from: url),
              let image = UIImage(data: downloaded.0)?.cgImage
        else {
            return nil
        }
        return head(of: image)
    }

    private static func json(_ address: String) async -> [String: Any]? {
        guard let url = URL(string: address),
              let result = try? await URLSession.shared.data(from: url),
              (result.1 as? HTTPURLResponse)?.statusCode == 200
        else {
            return nil
        }
        return (try? JSONSerialization.jsonObject(with: result.0)) as? [String: Any]
    }

    /// The face (8, 8) and the hat over it (40, 8), each 8x8 on a 64-wide skin (scaled for a larger one).
    private static func head(of skin: CGImage) -> UIImage? {
        let unit = CGFloat(skin.width) / 64
        let side = 8 * unit
        guard unit >= 1,
              let face = skin.cropping(to: CGRect(x: 8 * unit, y: 8 * unit, width: side, height: side))
        else {
            return nil
        }
        let hat = skin.cropping(to: CGRect(x: 40 * unit, y: 8 * unit, width: side, height: side))
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        let renderer = UIGraphicsImageRenderer(size: CGSize(width: side, height: side), format: format)
        return renderer.image { context in
            context.cgContext.interpolationQuality = .none
            let bounds = CGRect(x: 0, y: 0, width: side, height: side)
            UIImage(cgImage: face).draw(in: bounds)
            if let hat {
                UIImage(cgImage: hat).draw(in: bounds)
            }
        }
    }
}
