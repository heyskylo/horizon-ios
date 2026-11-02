import XCTest
@testable import LauncherPreview

final class FormattingTests: XCTestCase {
    func testColorsDecorationsAndReset() {
        let r = MinecraftFormatting.runs("§aGreen §lBold§oItalic§nUnder§mStrike§rPlain")
        XCTAssertEqual(r[0].style.color, 0x55FF55)
        XCTAssertTrue(r[1].style.bold)
        XCTAssertTrue(r[2].style.italic)
        XCTAssertTrue(r[3].style.underline)
        XCTAssertTrue(r[4].style.strike)
        XCTAssertEqual(r.last?.style, MinecraftFormatting.Style())
        XCTAssertEqual(MinecraftFormatting.plain("§6§lHYPIXEL§r\n§7SkyBlock"), "HYPIXEL\nSkyBlock")
    }
    func testColorResetsDecorationAndRGBIsOneColor() {
        let r = MinecraftFormatting.runs("§lBold§cRed§x§1§2§3§4§5§6Hex")
        XCTAssertFalse(r[1].style.bold)
        XCTAssertEqual(r[1].style.color, 0xFF5555)
        XCTAssertEqual(r[2].style.color, 0x123456)
    }
    func testUnicodeUnknownSequencesAndTrailingSectionSignArePreserved() {
        XCTAssertEqual(MinecraftFormatting.plain("§b世界 🧱 §z literal §"), "世界 🧱 §z literal §")
        XCTAssertTrue(MinecraftFormatting.runs("§kSecret")[0].style.obfuscated)
        XCTAssertEqual(MinecraftFormatting.plain("§LUPPER"), "UPPER")
    }
    func testWorldCreationJSONCarriesAllFields() throws {
        var settings = WorldCreationSettings()
        settings.name = "A world with spaces"; settings.seed = "hello world"; settings.mode = "hardcore"
        settings.flat = true; settings.structures = false
        let data = try JSONEncoder().encode(settings)
        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        XCTAssertEqual(json["name"] as? String, settings.name)
        XCTAssertEqual(json["seed"] as? String, "hello world")
        XCTAssertEqual(json["mode"] as? String, "hardcore")
        XCTAssertEqual(json["flat"] as? Bool, true)
        XCTAssertEqual(json["structures"] as? Bool, false)
        XCTAssertEqual(json["bonus_chest"] as? Bool, false)
    }
}

@MainActor
final class SettingsModelTests: XCTestCase {
    func testEmptyAndFailedResponsesAreVisibleFailures() async {
        let empty = GameOptions(request: { _ in ["ok": true] })
        await empty.load()
        XCTAssertNotNil(empty.issue)
        XCTAssertTrue(empty.options.isEmpty)
        XCTAssertFalse(empty.loading)
        let failed = GameOptions(request: { _ in ["error": "Disconnected"] })
        await failed.load()
        XCTAssertEqual(failed.issue, "Disconnected")
    }
    func testPartialOptionsRemainUsableAndIdentifyMissingControls() async {
        let model = GameOptions(request: { _ in ["options": ["autoJump": ["type": "bool", "value": false]]] })
        await model.load()
        XCTAssertNil(model.issue)
        XCTAssertNotNil(model.options["autoJump"])
        XCTAssertTrue(model.unavailable.contains("Graphics"))
    }
    func testToggleUpdatesOnlyAfterAcknowledgement() async {
        var value = false
        let model = GameOptions(request: { line in
            if line.hasPrefix("options-set autoJump ") {
                value = line.hasSuffix("true")
                return ["option": ["type": "bool", "value": value]]
            }
            return ["options": ["autoJump": ["type": "bool", "value": value]]]
        })
        await model.load()
        let issue = await model.set("autoJump", "true")
        XCTAssertNil(issue)
        XCTAssertTrue(model.options["autoJump"]?.flag == true)
        await model.load()
        XCTAssertTrue(model.options["autoJump"]?.flag == true)
        let noAck = GameOptions(request: { _ in ["ok": true] })
        let noAckIssue = await noAck.set("autoJump", "true")
        XCTAssertNotNil(noAckIssue)
    }
    func testPreviewFixtureCoversEveryLauncherSetting() throws {
        let url = try XCTUnwrap(Bundle.main.url(forResource: "EngineOptions", withExtension: "json"))
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
        let options = try XCTUnwrap(object["options"] as? [String: Any])
        XCTAssertEqual(Set(options.keys), Set(GameOptions.groups.flatMap { $0.rows.map(\.key) }))
    }
}

@MainActor
final class LauncherAsyncTests: XCTestCase {
    private func hits(_ id: String) -> [String: Any] { ["hits": [["project_id": id, "title": id]]] }

    func testOldPackSearchCannotOverwriteNewResults() async {
        var pending: [String: CheckedContinuation<Any?, Never>] = [:]
        let store = PackStore(fetch: { url in
            let query = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "query" }?.value ?? ""
            return await withCheckedContinuation { pending[query] = $0 }
        })
        let old = Task { await store.search(.resource, query: "old") }
        while pending["old"] == nil { await Task.yield() }
        let new = Task { await store.search(.shader, query: "new") }
        while pending["new"] == nil { await Task.yield() }
        pending.removeValue(forKey: "new")?.resume(returning: hits("new"))
        await new.value
        pending.removeValue(forKey: "old")?.resume(returning: hits("old"))
        await old.value
        XCTAssertEqual(store.results.map(\.id), ["new"])
        XCTAssertFalse(store.searching)
        XCTAssertNil(store.failed)
    }

    func testOldPackFailureCannotEndNewSearchOrSetError() async {
        var pending: [String: CheckedContinuation<Any?, Never>] = [:]
        let store = PackStore(fetch: { url in
            let query = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "query" }?.value ?? ""
            return await withCheckedContinuation { pending[query] = $0 }
        })
        let old = Task { await store.search(.resource, query: "old") }
        while pending["old"] == nil { await Task.yield() }
        let new = Task { await store.search(.shader, query: "new") }
        while pending["new"] == nil { await Task.yield() }
        pending.removeValue(forKey: "old")?.resume(returning: nil)
        await old.value
        XCTAssertTrue(store.searching)
        XCTAssertNil(store.failed)
        pending.removeValue(forKey: "new")?.resume(returning: hits("new"))
        await new.value
        XCTAssertEqual(store.results.map(\.id), ["new"])
    }

    func testDuplicatePackInstallDoesNotStartAnotherRequest() async {
        var resume: CheckedContinuation<Any?, Never>?
        let store = PackStore(fetch: { _ in await withCheckedContinuation { resume = $0 } })
        let project = ModrinthProject(id: "test", title: "Test", summary: "", author: "", downloads: 0, icon: nil, gameVersions: [])
        let first = Task { await store.install(project, kind: .resource, version: "26.3") }
        while resume == nil { await Task.yield() }
        if case .success = await store.install(project, kind: .resource, version: "26.3") { XCTFail("Duplicate install succeeded") }
        XCTAssertNotNil(store.downloading[project.id])
        resume?.resume(returning: [])
        _ = await first.value
        XCTAssertNil(store.downloading[project.id])
    }

    func testStaleGameFileCompletionDoesNotImportOrReplaceSharedArchive() async throws {
        let shared = GameFolder.documents.appendingPathComponent("horizon-download.zip")
        let previous = try? Data(contentsOf: shared)
        let sentinel = Data("keep-current-download".utf8)
        try sentinel.write(to: shared)
        defer { if let previous { try? previous.write(to: shared) } else { try? FileManager.default.removeItem(at: shared) } }
        let source = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try Data("stale-download".utf8).write(to: source)
        let imported = expectation(description: "Stale completion must not import")
        imported.isInverted = true
        let model = GameFilesDownload(importArchive: { _ in imported.fulfill(); return ["ok": true] })
        let session = URLSession(configuration: .ephemeral)
        defer { session.invalidateAndCancel() }
        let task = session.downloadTask(with: URL(string: "https://example.invalid/test.zip")!)
        model.urlSession(session, downloadTask: task, didFinishDownloadingTo: source)
        await fulfillment(of: [imported], timeout: 0.15)
        XCTAssertEqual(model.state, .idle)
        XCTAssertEqual(try Data(contentsOf: shared), sentinel)
    }
}

@MainActor
final class PackMutationTests: XCTestCase {
    func testSimultaneousResourceTogglesMergeAfterFirstAcknowledgement() async {
        var packs: [String] = []
        var writes: [[String]] = []
        var firstAck: CheckedContinuation<Void, Never>?
        let choices = PackChoices(request: { line in
            if line == "options-get resourcePacks" { return ["options": ["resourcePacks": ["value": packs]]] }
            if line.hasPrefix("options-set resourcePacks ") {
                let json = String(line.dropFirst("options-set resourcePacks ".count))
                let next = (try? JSONSerialization.jsonObject(with: Data(json.utf8))) as? [String] ?? []
                writes.append(next)
                if writes.count == 1 { await withCheckedContinuation { firstAck = $0 } }
                packs = next
                return ["ok": true]
            }
            return ["selected": NSNull()]
        })
        let first = Task { await choices.set("a.zip", kind: .resource, on: true) }
        while firstAck == nil { await Task.yield() }
        let second = Task { await choices.set("b.zip", kind: .resource, on: true) }
        for _ in 0..<10 { await Task.yield() }
        XCTAssertEqual(writes.count, 1)
        firstAck?.resume()
        let firstError = await first.value; let secondError = await second.value
        XCTAssertNil(firstError); XCTAssertNil(secondError)
        XCTAssertEqual(writes, [["file/a.zip"], ["file/a.zip", "file/b.zip"]])
        XCTAssertEqual(choices.resourcePacks, ["file/a.zip", "file/b.zip"])
    }
    func testActivePackDisableFailureLeavesFileUntouched() async {
        var deleted = false
        let choices = PackChoices(request: { line in
            if line.hasPrefix("options-set") { return ["error": "Reload failed"] }
            return ["options": ["resourcePacks": ["value": ["file/a.zip"]]]]
        })
        let error = await choices.remove("a.zip", kind: .resource, delete: { deleted = true; return nil })
        XCTAssertEqual(error, "Reload failed")
        XCTAssertFalse(deleted)
        XCTAssertTrue(choices.isOn("a.zip", kind: .resource))
    }
    func testUnconfirmedDisableDoesNotDeleteFile() async {
        var deleted = false
        let choices = PackChoices(request: { line in
            if line.hasPrefix("options-set") { return ["ok": true] }
            return ["options": ["resourcePacks": ["value": ["file/a.zip"]]]]
        })
        let error = await choices.remove("a.zip", kind: .resource, delete: { deleted = true; return nil })
        XCTAssertNotNil(error)
        XCTAssertFalse(deleted)
    }
    func testDeletionRunsOnlyAfterConfirmedDisable() async {
        var packs = ["file/a.zip"]
        var events: [String] = []
        let choices = PackChoices(request: { line in
            if line.hasPrefix("options-set") { events.append("disable"); packs = []; return ["ok": true] }
            return ["options": ["resourcePacks": ["value": packs]]]
        })
        let error = await choices.remove("a.zip", kind: .resource, delete: { events.append("delete"); XCTAssertTrue(packs.isEmpty); return nil })
        XCTAssertNil(error)
        XCTAssertEqual(events, ["disable", "delete"])
    }
}

@MainActor
final class AutomaticGameFilesTests: XCTestCase {
    private func temporarilyRemoveResourceMarker() -> () -> Void {
        let marker = GameFolder.resources.appendingPathComponent("horizon-resources.json")
        let previous = try? Data(contentsOf: marker)
        try? FileManager.default.removeItem(at: marker)
        return {
            if let previous { try? previous.write(to: marker) }
            else { try? FileManager.default.removeItem(at: marker) }
        }
    }

    func testAutomaticSourceFailureNeedsExplicitRetry() async {
        let restore = temporarilyRemoveResourceMarker()
        defer { restore() }
        var calls = 0
        let model = GameFilesDownload(automaticSource: {
            calls += 1
            return ["error": "Fixture source is unavailable"]
        })
        model.link = "https://example.invalid/private.zip"
        model.token = "private-test-token"
        await model.startAutomatic()
        XCTAssertEqual(model.state, .failed("Fixture source is unavailable"))
        await model.startAutomaticallyIfNeeded()
        XCTAssertEqual(calls, 1)
        await model.startAutomatic()
        XCTAssertEqual(calls, 2)
        XCTAssertFalse(model.busy)
        XCTAssertEqual(model.link, "https://example.invalid/private.zip")
        XCTAssertEqual(model.token, "private-test-token")
    }

    func testCancelWhileResolvingSourceCannotStartLateDownload() async {
        var resume: CheckedContinuation<[String: Any], Never>?
        let model = GameFilesDownload(automaticSource: {
            await withCheckedContinuation { resume = $0 }
        })
        let start = Task { await model.startAutomatic() }
        while resume == nil { await Task.yield() }
        XCTAssertTrue(model.busy)
        model.cancel()
        resume?.resume(returning: ["url": "https://example.invalid/must-not-fetch.zip"])
        await start.value
        XCTAssertEqual(model.state, .idle)
        XCTAssertFalse(model.busy)
    }

    func testCancelledSetupTaskCannotStartLateDownload() async {
        var resume: CheckedContinuation<[String: Any], Never>?
        let model = GameFilesDownload(automaticSource: {
            await withCheckedContinuation { resume = $0 }
        })
        let start = Task { await model.startAutomatic() }
        while resume == nil { await Task.yield() }
        start.cancel()
        resume?.resume(returning: ["url": "http://127.0.0.1:1/must-not-fetch.zip"])
        await start.value
        XCTAssertEqual(model.state, .idle)
        XCTAssertFalse(model.busy)
    }

    func testFilesArrivingDuringSourceLookupDoNotGetReplaced() async throws {
        let marker = GameFolder.resources.appendingPathComponent("horizon-resources.json")
        let previous = try? Data(contentsOf: marker)
        try? FileManager.default.removeItem(at: marker)
        defer {
            if let previous { try? previous.write(to: marker) }
            else { try? FileManager.default.removeItem(at: marker) }
        }
        var resume: CheckedContinuation<[String: Any], Never>?
        let model = GameFilesDownload(automaticSource: {
            await withCheckedContinuation { resume = $0 }
        })
        let start = Task { await model.startAutomaticallyIfNeeded() }
        while resume == nil { await Task.yield() }
        try FileManager.default.createDirectory(at: GameFolder.resources, withIntermediateDirectories: true)
        try Data("manual-import-fixture".utf8).write(to: marker)
        resume?.resume(returning: ["url": "http://127.0.0.1:1/must-not-fetch.zip"])
        await start.value
        XCTAssertEqual(model.state, .done("Game files are already installed."))
        XCTAssertEqual(try Data(contentsOf: marker), Data("manual-import-fixture".utf8))
        XCTAssertFalse(model.busy)
    }

    func testAlreadyInstalledFilesDoNotDownload() async throws {
        let marker = GameFolder.resources.appendingPathComponent("horizon-resources.json")
        let previous = try? Data(contentsOf: marker)
        try FileManager.default.createDirectory(at: GameFolder.resources, withIntermediateDirectories: true)
        try Data("{}".utf8).write(to: marker)
        defer {
            if let previous { try? previous.write(to: marker) }
            else { try? FileManager.default.removeItem(at: marker) }
        }
        var calls = 0
        let model = GameFilesDownload(automaticSource: { calls += 1; return [:] })
        await model.startAutomaticallyIfNeeded()
        XCTAssertEqual(calls, 0)
        XCTAssertEqual(model.state, .idle)
    }

    func testAutomaticDownloadUsesResourceImporterAndKeepsCustomCredentials() async throws {
        let restore = temporarilyRemoveResourceMarker()
        defer { restore() }
        guard let url = ProcessInfo.processInfo.environment["HORIZON_DOWNLOAD_FIXTURE_URL"] else {
            throw XCTSkip("Requires the loopback download fixture")
        }
        let imported = expectation(description: "Downloaded fixture reaches importer")
        let model = GameFilesDownload(importArchive: { line in
            XCTAssertTrue(line.hasPrefix("import-game-files "))
            let file = URL(fileURLWithPath: String(line.dropFirst("import-game-files ".count)))
            XCTAssertEqual(try? Data(contentsOf: file), Data("horizon-download-fixture".utf8))
            imported.fulfill()
            return ["ok": true, "kind": "resources"]
        }, automaticSource: { ["url": url] })
        model.link = "https://example.invalid/private.zip"
        model.token = "private-test-token"
        await model.startAutomatic()
        await fulfillment(of: [imported], timeout: 10)
        if case .done = model.state {} else { XCTFail("Download did not finish: \(model.state)") }
        XCTAssertEqual(model.link, "https://example.invalid/private.zip")
        XCTAssertEqual(model.token, "private-test-token")
    }

    func testFilesArrivingDuringTransferDoNotGetReplaced() async throws {
        let restore = temporarilyRemoveResourceMarker()
        defer { restore() }
        guard let url = ProcessInfo.processInfo.environment["HORIZON_DOWNLOAD_FIXTURE_URL"] else {
            throw XCTSkip("Requires the loopback download fixture")
        }
        var imports = 0
        let model = GameFilesDownload(importArchive: { _ in
            imports += 1
            return ["ok": true, "kind": "resources"]
        }, automaticSource: { ["url": url + "?delay=1"] })
        await model.startAutomatic()
        let marker = GameFolder.resources.appendingPathComponent("horizon-resources.json")
        try FileManager.default.createDirectory(at: GameFolder.resources, withIntermediateDirectories: true)
        try Data("manual-transfer-fixture".utf8).write(to: marker)
        for _ in 0..<100 where model.busy {
            try await Task.sleep(for: .milliseconds(50))
        }
        XCTAssertEqual(imports, 0)
        XCTAssertEqual(model.state, .done("Game files are already installed."))
        XCTAssertEqual(try Data(contentsOf: marker), Data("manual-transfer-fixture".utf8))
    }
}
