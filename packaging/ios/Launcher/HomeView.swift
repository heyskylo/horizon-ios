import SwiftUI
import UIKit

/// Native launcher shared by iPhone and iPad. The game/session bridge is unchanged.
struct HomeView: View {
    var onPlay: () -> Void
    @StateObject private var link = GameLink()
    @StateObject private var download = GameFilesDownload()
    @StateObject private var player = PlayerName()
    @StateObject private var head = SkinHead()
    @StateObject private var account = MicrosoftAccount()
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var tab = Destination.play
    @State private var showingAccount = false
    @State private var editingName = false
    @State private var nameDraft = ""
    @State private var nameRejected = false

    enum Destination: String, CaseIterable, Identifiable {
        case play, packs, settings
        var id: Self { self }
        var title: String { rawValue.capitalized }
        var icon: String {
            switch self {
            case .play: return "play.circle"
            case .packs: return "shippingbox"
            case .settings: return "gearshape"
            }
        }
    }

    init(onPlay: @escaping () -> Void = {}) { self.onPlay = onPlay }

    var body: some View {
        Group {
            if sizeClass == .regular {
                NavigationSplitView {
                    VStack(alignment: .leading, spacing: 28) {
                        HStack(spacing: 12) {
                            Kit.image("horizon-mark").resizable().renderingMode(.template).scaledToFit().frame(width: 32, height: 32)
                            VStack(alignment: .leading, spacing: 3) {
                                Text("Horizon").font(.system(.title3, design: .rounded).bold())
                                Text("JAVA EDITION").font(.caption2.weight(.medium)).tracking(1.5).foregroundStyle(.secondary)
                            }
                        }.padding(.horizontal, 12)
                        VStack(spacing: 8) {
                            ForEach(Destination.allCases) { item in
                                Button { tab = item } label: {
                                    Label(item.title, systemImage: item.icon)
                                        .font(.system(.body, design: .rounded).weight(tab == item ? .semibold : .medium))
                                        .frame(maxWidth: .infinity, minHeight: 52, alignment: .leading)
                                        .padding(.horizontal, 16)
                                        .foregroundStyle(tab == item ? Theme.accent : Theme.textMuted)
                                        .background(tab == item ? Theme.notice : .clear, in: RoundedRectangle(cornerRadius: 18))
                                }.buttonStyle(.plain).accessibilityIdentifier("launcher-nav-" + item.rawValue)
                            }
                        }
                        Spacer()
                    }
                    .padding(20).padding(.top, 12)
                    .background(Theme.background)
                    .navigationSplitViewColumnWidth(min: 210, ideal: 240, max: 280)
                    .accessibilityIdentifier("launcher-navigation-rail")
                } detail: {
                    NavigationStack { destination(tab) }
                }
                .navigationSplitViewStyle(.balanced)
            } else {
                TabView(selection: $tab) {
                    ForEach(Destination.allCases) { item in
                        NavigationStack { destination(item) }
                            .tabItem { Label(item.title, systemImage: item.icon) }
                            .tag(item)
                    }
                }

            }
        }
        .background(Theme.background.ignoresSafeArea())
        .tint(Theme.accent)
        .sheet(isPresented: $showingAccount) { accountSheet }
        .alert("Something went wrong", isPresented: Binding(
            get: { link.problem != nil }, set: { if !$0 { link.problem = nil } }
        )) { Button("OK", role: .cancel) {} } message: { Text(link.problem ?? "") }
        .task(id: shownName) { await head.load(shownName) }
        .task(id: account.profile) { await account.handOver() }
        .onChange(of: account.profile) { profile in
            if profile != nil { editingName = false; nameDraft = "" }
        }
        .task { link.uncover = onPlay; await link.watch() }
        .task(id: link.phase) {
            if link.phase == .waiting { await download.startAutomaticallyIfNeeded() }
        }
    }

    private func pageHeading(_ item: Destination) -> some View {
        HStack(alignment: .center, spacing: 16) {
            VStack(alignment: .leading, spacing: 8) {
                Text("HORIZON\(item == .play ? " / JAVA EDITION" : "")")
                    .font(.caption2.weight(.semibold)).tracking(2).foregroundStyle(Theme.accent)
                Text(item.title).font(.system(.largeTitle, design: .rounded).bold())
                if item == .settings {
                    Text("Display, sound & the way you play").font(.subheadline).foregroundStyle(.secondary)
                }
            }
            Spacer(minLength: 0)
            Button { showingAccount = true } label: {
                Avatar(name: shownName, face: head.face, size: 34)
                    .frame(width: 48, height: 48)
                    .accountCircleGlass()
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Account: \(shownName ?? "Not signed in")")
            .accessibilityIdentifier("launcher-account-button")
        }
    }

    private var shownName: String? { account.profile?.name ?? player.name }

    private func editName() {
        guard account.profile == nil else { return }
        nameRejected = false
        nameDraft = player.name ?? ""
        editingName = true
    }

    private func destination(_ item: Destination) -> some View {
        page {
            VStack(alignment: .leading, spacing: 26) {
                pageHeading(item)
                switch item {
                case .play: home
                case .packs: PacksPage(link: link)
                case .settings:
                    VStack(spacing: 24) {
                        SettingsPage(link: link, download: download)
                        NavigationLink { page { GalleryPage() }.navigationTitle("Screenshots") } label: {
                            Label("Screenshots", systemImage: "photo.on.rectangle")
                                .frame(maxWidth: .infinity, alignment: .leading).padding(16).card()
                        }
                    }
                }
            }
        }
        .navigationTitle("")
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(.hidden, for: .navigationBar)
    }

    private func page<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        ScrollView {
            content()
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(sizeClass == .regular ? 32 : 20)
                .frame(maxWidth: sizeClass == .regular ? 1200 : 700)
                .frame(maxWidth: .infinity)
        }
        .background(Theme.background)
        .refreshable { await link.refresh() }
    }

    private var home: some View {
        VStack(alignment: .leading, spacing: 28) {
            launchPanel
            if needsFiles { GameFilesCard(download: download, missing: true) }
        }
    }

    private var launchPanel: some View {
        VStack(alignment: .leading, spacing: 22) {
            HStack(spacing: 8) {
                Text("JAVA EDITION · \(link.version)").font(.caption.weight(.semibold)).tracking(1.4)
            }
            .foregroundStyle(.white.opacity(0.9))
            Spacer(minLength: 22)
            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 14) {
                    Kit.image("horizon-mark").resizable().renderingMode(.template)
                        .scaledToFit().frame(width: 54, height: 54).accessibilityHidden(true)
                    Text("Horizon").font(.system(.largeTitle, design: .rounded).bold())
                }
                .foregroundStyle(.white)
                .padding(.top, 8)
                HStack(spacing: 8) {
                    Avatar(name: shownName, face: head.face, size: 26)
                    Text(shownName ?? "Offline player").font(.subheadline.weight(.medium)).lineLimit(1)
                    if account.profile != nil {
                        Image(systemName: "checkmark.seal.fill").font(.caption)
                            .accessibilityLabel("Microsoft account")
                    }
                }.foregroundStyle(.white.opacity(0.9))
                    .accessibilityIdentifier("launcher-playing-as")
            }
            HStack {
                Button { Task { await link.play() } } label: {
                    Label(playLabel, systemImage: "play.fill")
                        .font(.headline).frame(minWidth: 124, minHeight: 36)
                }
                .controlSize(.large)
                .launcherPrimaryButton()
                .tint(Theme.accent)
                .disabled(needsFiles || link.launching != nil)
                .accessibilityIdentifier("launcher-play-button")
                if link.launching != nil {
                    Button("Cancel") { link.cancelLaunch() }.foregroundStyle(.white).frame(minHeight: 44)
                }
                Spacer(minLength: 0)
            }
            if case let .loading(progress) = link.phase {
                ProgressView(value: progress).tint(.white).accessibilityLabel("Loading game")
            }
        }
        .padding(sizeClass == .regular ? 32 : 24)
        .frame(maxWidth: .infinity, minHeight: sizeClass == .regular ? 350 : 300, alignment: .leading)
        .background {
            LauncherLandscape(index: 0)
                .overlay(LinearGradient(colors: [.black.opacity(0.05), .black.opacity(0.72)],
                                        startPoint: .topTrailing, endPoint: .bottomLeading))
        }
        .clipShape(RoundedRectangle(cornerRadius: 28))
    }

    private var playLabel: String { link.launching ?? (needsFiles ? "Game files needed" : "Play") }
    private var needsFiles: Bool {
        if case .needsFiles = link.phase { return true }
        return link.phase == .waiting && !GameFolder.hasGameFiles
    }

    private var accountSheet: some View {
        NavigationStack {
            Form {
                Section {
                    HStack(spacing: 16) {
                        Avatar(name: shownName, face: head.face, size: 52)
                        VStack(alignment: .leading, spacing: 4) {
                            Text(shownName ?? "Offline player").font(.headline)
                            Text(account.profile == nil ? "Offline profile" : "Microsoft account")
                                .font(.subheadline).foregroundStyle(.secondary)
                        }
                    }.padding(.vertical, 8)
                    if account.profile == nil { Button("Change Offline Name", action: editName) }
                }
                Section { AccountCard(account: account) }
            }
            .navigationDestination(isPresented: $editingName) { offlineNameEditor }
            .navigationTitle("Account")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { showingAccount = false } } }
        }
        .presentationDetents([.medium, .large])
    }

    private var offlineNameEditor: some View {
        Form {
            Section {
                TextField("Name", text: $nameDraft)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .accessibilityIdentifier("offline-player-name")
            } footer: {
                if nameRejected {
                    Text("That name can’t be used. Use 1 to 16 characters with no spaces.")
                        .foregroundStyle(.red)
                        .accessibilityIdentifier("offline-player-name-error")
                } else {
                    Text("Up to 16 characters, with no spaces.")
                }
            }
        }
        .navigationTitle("Offline player name")
        .navigationBarTitleDisplayMode(.inline)
        .navigationBarBackButtonHidden()
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button("Cancel") { editingName = false }
            }
            ToolbarItem(placement: .confirmationAction) {
                Button("Save") {
                    guard account.profile == nil else { return }
                    if player.save(nameDraft.trimmingCharacters(in: .whitespaces)) {
                        editingName = false
                    } else {
                        nameRejected = true
                    }
                }
            }
        }
    }

}

struct Avatar: View {
    let name: String?
    let face: UIImage?
    let size: CGFloat
    var body: some View {
        Group {
            if let face {
                Image(uiImage: face).resizable().interpolation(.none).scaledToFit().padding(size * 0.10)
            } else {
                Image(systemName: "person.fill").resizable().scaledToFit().padding(size * 0.24)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(width: size, height: size)
        .background(Theme.surfaceDisabled, in: Circle())
        .clipShape(Circle())
        .accessibilityHidden(true)
    }
}
