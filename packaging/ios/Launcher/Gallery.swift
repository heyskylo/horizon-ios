import SwiftUI
import UIKit

/// A screenshot the game took (F2, or the touch screenshot button) into `Documents/screenshots`.
struct Shot: Identifiable, Hashable {
    let url: URL
    let date: Date
    var id: URL { url }

    /// Newest first.
    static func all() -> [Shot] {
        let keys: [URLResourceKey] = [.contentModificationDateKey]
        let files = (try? FileManager.default.contentsOfDirectory(
            at: GameFolder.screenshots, includingPropertiesForKeys: keys, options: [.skipsHiddenFiles]
        )) ?? []
        return files
            .filter { $0.pathExtension.lowercased() == "png" }
            .map { Shot(url: $0, date: (try? $0.resourceValues(forKeys: Set(keys)))?.contentModificationDate ?? .distantPast) }
            .sorted { $0.date > $1.date }
    }
}

/// Small pictures of the screenshots, made off the main thread and kept while the launcher is up.
@MainActor
final class Thumbnails: ObservableObject {
    @Published private(set) var images: [URL: UIImage] = [:]
    private var asked: Set<URL> = []

    func load(_ url: URL, side: CGFloat) {
        guard !asked.contains(url) else { return }
        asked.insert(url)
        // Three pixels a point: the sharpest screen an iPhone has.
        let size = CGSize(width: side * 16 / 9 * 3, height: side * 3)
        Task.detached(priority: .utility) {
            let image = await UIImage(contentsOfFile: url.path)?.byPreparingThumbnail(ofSize: size)
            await MainActor.run {
                self.images[url] = image
                // A file may still be arriving or temporarily unreadable.
                // Coalesce only in-flight work and successfully cached images.
                if image == nil { self.asked.remove(url) }
            }
        }
    }
}

struct GalleryPage: View {
    @State private var shots: [Shot] = []
    @State private var opened: Shot?
    @StateObject private var thumbnails = Thumbnails()
    private let columns = [GridItem(.adaptive(minimum: 150), spacing: 8)]

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            PageTitle(title: "Gallery", subtitle: shots.isEmpty ? nil : "\(shots.count) screenshot\(shots.count == 1 ? "" : "s")")
            if shots.isEmpty {
                EmptyNote(icon: "camera", text: "Screenshots you take in the game show up here, ready to share.")
            } else {
                LazyVGrid(columns: columns, spacing: 8) {
                    ForEach(shots) { shot in
                        Button {
                            Haptics.tap()
                            opened = shot
                        } label: {
                            thumbnail(shot)
                        }
                        .buttonStyle(PressStyle())
                        .contextMenu {
                            ShareLink(item: shot.url) { Label("Share", systemImage: "square.and.arrow.up") }
                            Button(role: .destructive) {
                                delete(shot)
                            } label: { Label("Delete", systemImage: "trash") }
                        }
                    }
                }
            }
        }
        .onAppear { shots = Shot.all() }
        .fullScreenCover(item: $opened) { shot in
            ShotViewer(shots: shots, start: shot) { deleted in
                delete(deleted)
            }
        }
    }

    private func thumbnail(_ shot: Shot) -> some View {
        Color.clear
            .aspectRatio(16 / 9, contentMode: .fit)
            .overlay {
                if let image = thumbnails.images[shot.url] {
                    Image(uiImage: image).resizable().scaledToFill()
                } else {
                    Theme.surfaceSunken
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Theme.border))
            .onAppear { thumbnails.load(shot.url, side: 110) }
    }

    private func delete(_ shot: Shot) {
        try? FileManager.default.removeItem(at: shot.url)
        shots.removeAll { $0 == shot }
    }
}

/// One screenshot at a time, swiped through, with Share and Delete.
private struct ShotViewer: View {
    let shots: [Shot]
    let delete: (Shot) -> Void
    @State private var current: Shot
    @State private var confirming = false
    @Environment(\.dismiss) private var dismiss

    init(shots: [Shot], start: Shot, delete: @escaping (Shot) -> Void) {
        self.shots = shots
        self.delete = delete
        _current = State(initialValue: start)
    }

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            TabView(selection: $current) {
                ForEach(shots) { shot in
                    FullImage(url: shot.url).tag(shot)
                }
            }
            .tabViewStyle(.page(indexDisplayMode: .never))
            .ignoresSafeArea()
        }
        .overlay(alignment: .top) {
            HStack {
                Button { dismiss() } label: {
                    Image(systemName: "xmark").font(.system(size: 16, weight: .bold)).frame(width: 44, height: 44)
                }
                Spacer()
                Text(current.date.formatted(date: .abbreviated, time: .shortened))
                    .font(Theme.mono(11))
                Spacer()
                ShareLink(item: current.url) {
                    Image(systemName: "square.and.arrow.up").font(.system(size: 16, weight: .semibold))
                        .frame(width: 44, height: 44)
                }
                Button { confirming = true } label: {
                    Image(systemName: "trash").font(.system(size: 16, weight: .semibold)).frame(width: 44, height: 44)
                }
            }
            .foregroundStyle(Theme.text)
            .padding(.horizontal, 8)
            .background(.black.opacity(0.45))
        }
        .confirmationDialog("Delete this screenshot?", isPresented: $confirming, titleVisibility: .visible) {
            Button("Delete", role: .destructive) {
                delete(current)
                dismiss()
            }
        }

    }
}

private struct FullImage: View {
    let url: URL
    @State private var image: UIImage?

    var body: some View {
        Group {
            if let image {
                Image(uiImage: image).resizable().scaledToFit()
            } else {
                ProgressView().tint(.white)
            }
        }
        .task(id: url) {
            let url = url
            image = await Task.detached(priority: .userInitiated) { UIImage(contentsOfFile: url.path) }.value
        }
    }
}
