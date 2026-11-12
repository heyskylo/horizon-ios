import XCTest
@testable import LauncherPreview

/// Only a presence marker for the in-memory engine; never actual Minecraft assets.
private final class PreparedGameFilesFixture {
    private var created: URL?
    init() throws {
        let marker = GameFolder.resources.appendingPathComponent("horizon-resources.json")
        if !FileManager.default.fileExists(atPath: marker.path) {
            try FileManager.default.createDirectory(at: marker.deletingLastPathComponent(),
                                                    withIntermediateDirectories: true)
            try Data("{}".utf8).write(to: marker, options: .withoutOverwriting)
            created = marker
        }
    }
    func cleanUp() throws {
        // Preserve a marker/resources already supplied by the preview host.
        if let marker = created {
            try FileManager.default.removeItem(at: marker)
            created = nil
        }
    }
}

private final class MemoryAccountVault: AccountVault {
    var values: [String: AccountSecrets] = [:]
    var failWrites = false
    func read(_ account: String) throws -> AccountSecrets? { values[account] }
    func write(_ secrets: AccountSecrets, for account: String) throws {
        if failWrites { throw AccountError("Fixture storage failure") }
        values[account] = secrets
    }
    func remove(_ account: String) throws { values.removeValue(forKey: account) }
}

private final class AuthURLProtocol: URLProtocol {
    static var failure: Error?
    static var response: (URLRequest) -> (Int, [String: Any]) = { _ in (500, [:]) }
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        if let error = Self.failure { client?.urlProtocol(self, didFailWithError: error); return }
        let (status, body) = Self.response(request)
        let response = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil, headerFields: ["Content-Type": "application/json"])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: try! JSONSerialization.data(withJSONObject: body))
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}

@MainActor
final class AccountTests: XCTestCase {
    private let alice = MicrosoftAccount.Profile(name: "Alice", uuid: String(repeating: "1", count: 32))
    private let bob = MicrosoftAccount.Profile(name: "Bob", uuid: String(repeating: "2", count: 32))
    private let instant = Date(timeIntervalSince1970: 2_000_000_000)
    private var suite: String!
    private var defaults: UserDefaults!
    private var vault: MemoryAccountVault!
    private var session: URLSession!
    private var requests: [String] = []
    private var bridgeCommands: [String] = []
    private let lock = NSLock()
    private var preparedGameFiles: PreparedGameFilesFixture?

    override func setUp() async throws {
        suite = "horizon.auth.fixture." + UUID().uuidString
        defaults = UserDefaults(suiteName: suite)!
        vault = MemoryAccountVault()
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [AuthURLProtocol.self]
        session = URLSession(configuration: configuration)
        requests = []; bridgeCommands = []; AuthURLProtocol.failure = nil
    }
    override func tearDown() async throws {
        try preparedGameFiles?.cleanUp()
        preparedGameFiles = nil
        session.invalidateAndCancel()
        defaults.removePersistentDomain(forName: suite)
        AuthURLProtocol.response = { _ in (500, [:]) }; AuthURLProtocol.failure = nil
    }
    private func seed(expired: Bool = true) throws {
        defaults.set(try JSONEncoder().encode(MicrosoftAccount.Catalog(profiles: [alice, bob], selected: alice.uuid)), forKey: "microsoftAccounts.v2")
        vault.values[alice.uuid] = AccountSecrets(refresh: "fixture-old-refresh", token: "fixture-old-access", expires: instant.addingTimeInterval(expired ? -86400 : 7200))
        vault.values[bob.uuid] = AccountSecrets(refresh: "fixture-bob-refresh", token: "fixture-bob-access", expires: instant.addingTimeInterval(7200))
    }
    private func model(bridgeOK: Bool = true) -> MicrosoftAccount {
        MicrosoftAccount(defaults: defaults, vault: vault, session: session, clock: { self.instant }, sleep: { _ in await Task.yield() }, client: "fixture-client") { command in
            self.bridgeCommands.append(command)
            return bridgeOK ? ["ok": true] : ["error": "fixture failure"]
        }
    }
    private func network(tokenError: Bool = false, downstreamError: Bool = false, expiry: Bool = true) {
        let uuid = alice.uuid
        AuthURLProtocol.response = { request in
            let path = request.url!.path
            self.lock.lock(); self.requests.append(path); self.lock.unlock()
            if path.hasSuffix("/token") {
                return tokenError ? (400, ["error": "invalid_grant"]) : (200, ["access_token": "fixture-ms-access", "refresh_token": "fixture-rotated"])
            }
            if path == "/user/authenticate" { return (200, ["Token": "fixture-xbl", "DisplayClaims": ["xui": [["uhs": "fixture"]]]]) }
            if path == "/xsts/authorize" { return downstreamError ? (503, ["error": "unavailable"]) : (200, ["Token": "fixture-xsts"]) }
            if path == "/authentication/login_with_xbox" {
                var answer: [String: Any] = ["access_token": "fixture-new-access"]
                if expiry { answer["expires_in"] = 3600 }
                return (200, answer)
            }
            if path == "/entitlements/mcstore" { return (200, ["items": [["name": "game_minecraft"]]]) }
            return (200, ["id": uuid, "name": "AliceRenewed"])
        }
    }
    func testExpiredRelaunchAndConcurrentPlayShareRefresh() async throws {
        try seed(); network(); let account = model()
        async let first: Void = account.handOver()
        async let second: Void = account.handOver()
        try await first; try await second
        XCTAssertEqual(requests.filter { $0.hasSuffix("/token") }.count, 1)
        XCTAssertEqual(vault.values[alice.uuid]?.refresh, "fixture-rotated")
        XCTAssertEqual(account.profile?.name, "AliceRenewed")
        XCTAssertTrue(bridgeCommands.allSatisfy { $0.contains("fixture-new-access") })
        let metadata = String(decoding: defaults.data(forKey: "microsoftAccounts.v2")!, as: UTF8.self)
        XCTAssertFalse(metadata.contains("fixture-"))
    }
    func testRotationPersistsBeforeDownstreamFailureWithoutSigningOut() async throws {
        try seed(); network(downstreamError: true); let account = model()
        do { try await account.handOver(); XCTFail("must block Play") } catch {
            XCTAssertFalse((error as? AccountError)?.reauthenticate ?? true)
        }
        XCTAssertEqual(vault.values[alice.uuid]?.refresh, "fixture-rotated")
        XCTAssertEqual(account.profile, alice); XCTAssertEqual(account.accounts.count, 2)
        XCTAssertTrue(bridgeCommands.isEmpty)
    }
    func testInvalidGrantPreservesAccountAndAsksForReauthentication() async throws {
        try seed(); network(tokenError: true); let account = model()
        do { try await account.handOver(); XCTFail("must block Play") } catch {
            XCTAssertTrue((error as? AccountError)?.reauthenticate ?? false)
        }
        XCTAssertEqual(account.profile, alice)
        XCTAssertEqual(vault.values[alice.uuid]?.refresh, "fixture-old-refresh")
        XCTAssertTrue(bridgeCommands.isEmpty)
    }
    func testUnknownExpiryIsNotAcceptedAsOneDay() async throws {
        try seed(); network(expiry: false); let account = model()
        do { try await account.handOver(); XCTFail("missing expiry must fail") } catch {}
        XCTAssertEqual(vault.values[alice.uuid]?.refresh, "fixture-rotated")
        XCTAssertEqual(vault.values[alice.uuid]?.token, "fixture-old-access")
        XCTAssertTrue(bridgeCommands.isEmpty)
    }
    func testBridgeFailureDoesNotUncoverOrStartGame() async throws {
        try seed(expired: false); let account = model(bridgeOK: false)
        let game = GameLink(); var uncovered = false
        game.prepareSession = { try await account.handOver() }; game.uncover = { uncovered = true }
        await game.play()
        XCTAssertNotNil(game.problem); XCTAssertFalse(uncovered)
        XCTAssertEqual(game.phase, .unknown); XCTAssertNil(game.launching)
    }
    func testLegacyMigrationRetriesWithoutDeletingOnWriteFailure() async throws {
        defaults.set(try JSONEncoder().encode(alice), forKey: "microsoftProfile")
        let secrets = AccountSecrets(refresh: "fixture-old", token: "fixture-valid", expires: instant.addingTimeInterval(7200))
        vault.values["account"] = secrets; vault.failWrites = true
        let account = model()
        XCTAssertEqual(vault.values["account"], secrets)
        XCTAssertNil(defaults.data(forKey: "microsoftAccounts.v2"))
        vault.failWrites = false; try await account.handOver()
        XCTAssertEqual(vault.values[alice.uuid], secrets); XCTAssertNil(vault.values["account"])
        XCTAssertNil(defaults.data(forKey: "microsoftProfile"))
        XCTAssertEqual(model().profile, alice)
    }
    func testSwitchDuringRefreshCannotTransferOldSelection() async throws {
        try seed(); network(); let account = model()
        let requested = expectation(description: "refresh started")
        let gate = DispatchSemaphore(value: 0)
        let response = AuthURLProtocol.response
        AuthURLProtocol.response = { request in
            if request.url!.path.hasSuffix("/token") {
                requested.fulfill(); _ = gate.wait(timeout: .now() + 5)
            }
            return response(request)
        }
        let task = Task { try await account.handOver() }
        await fulfillment(of: [requested], timeout: 3)
        account.select(bob)
        gate.signal()
        do { try await task.value } catch is CancellationError {} catch { XCTFail("\(error)") }
        try await account.handOver()
        XCTAssertEqual(account.profile, bob)
        XCTAssertTrue(bridgeCommands.allSatisfy { $0.contains("fixture-bob-access") })
    }
    func testRemoveSelectedAccountIsExplicitOfflineAndKeepsOthers() async throws {
        try seed(expired: false); let account = model()
        await account.remove(alice)
        XCTAssertNil(account.profile); XCTAssertNil(vault.values[alice.uuid])
        XCTAssertNotNil(vault.values[bob.uuid]); XCTAssertEqual(account.accounts, [bob])
        XCTAssertEqual(bridgeCommands, ["launcher-session off"])
    }
    func testFailedAddKeepsExistingSelection() async throws {
        try seed(expired: false); network(downstreamError: true)
        let response = AuthURLProtocol.response
        AuthURLProtocol.response = { request in
            if request.url!.path.hasSuffix("/devicecode") {
                return (200, ["device_code": "fixture-device", "user_code": "FIXTURE", "verification_uri": "https://microsoft.com/link", "expires_in": 900, "interval": 1])
            }
            return response(request)
        }
        let account = model(); account.signIn()
        for _ in 0..<100 {
            if case .failed = account.state { break }
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        guard case .failed = account.state else { return XCTFail("fixture add did not fail") }
        XCTAssertEqual(account.profile, alice); XCTAssertEqual(account.accounts, [alice, bob])
        XCTAssertEqual(vault.values[alice.uuid]?.refresh, "fixture-old-refresh")
    }
    func testCancelAddCannotReplaceSelection() async throws {
        try seed(expired: false); network()
        let codeRequested = expectation(description: "device code requested")
        AuthURLProtocol.response = { _ in
            codeRequested.fulfill()
            return (200, ["device_code": "fixture-device", "user_code": "FIXTURE", "verification_uri": "https://microsoft.com/link", "expires_in": 900, "interval": 1])
        }
        let account = MicrosoftAccount(defaults: defaults, vault: vault, session: session, clock: { self.instant }, sleep: { _ in try await Task.sleep(nanoseconds: 5_000_000_000) }, client: "fixture")
        account.signIn(); await fulfillment(of: [codeRequested], timeout: 3)
        account.cancel(); await Task.yield()
        XCTAssertEqual(account.profile, alice); XCTAssertEqual(account.state, .idle)
        XCTAssertEqual(account.accounts, [alice, bob])
    }
    func testCancelLaunchWhilePreparingDoesNotUncover() async throws {
        let game = GameLink(); var uncovered = false
        let started = expectation(description: "preparing")
        var continuation: CheckedContinuation<Void, Never>?
        game.prepareSession = {
            started.fulfill()
            await withCheckedContinuation { continuation = $0 }
        }
        game.uncover = { uncovered = true }
        let play = Task { await game.play() }
        await fulfillment(of: [started], timeout: 3)
        game.cancelLaunch(); continuation?.resume(); await play.value
        XCTAssertFalse(uncovered); XCTAssertEqual(game.phase, .unknown)
    }

    func testSuccessfulAddPersistsSeparateCredentialAndCanSwitchBack() async throws {
        try seed(expired: false); network()
        let response = AuthURLProtocol.response
        let third = String(repeating: "3", count: 32)
        AuthURLProtocol.response = { request in
            if request.url!.path.hasSuffix("/devicecode") {
                return (200, ["device_code": "fixture-device", "user_code": "FIXTURE", "verification_uri": "https://microsoft.com/link", "expires_in": 900, "interval": 1])
            }
            if request.url!.path == "/minecraft/profile" { return (200, ["id": third, "name": "Third"]) }
            return response(request)
        }
        let account = model(); account.signIn()
        for _ in 0..<100 {
            if account.profile?.uuid == third { break }
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTAssertEqual(account.profile?.uuid, third); XCTAssertEqual(account.accounts.count, 3)
        XCTAssertNotNil(vault.values[third]); XCTAssertNotNil(vault.values[alice.uuid])
        account.select(alice); try await account.handOver()
        XCTAssertTrue(bridgeCommands.last?.contains("fixture-old-access") == true)
        XCTAssertEqual(model().profile, alice)
    }

    func testNetworkFailurePreservesCredentialsAndDoesNotRequestReauthentication() async throws {
        try seed(); AuthURLProtocol.failure = URLError(.notConnectedToInternet)
        let account = model()
        do { try await account.handOver(); XCTFail("must fail") } catch {
            XCTAssertFalse((error as? AccountError)?.reauthenticate ?? true)
        }
        XCTAssertEqual(vault.values[alice.uuid]?.refresh, "fixture-old-refresh")
        XCTAssertEqual(account.profile, alice); XCTAssertTrue(bridgeCommands.isEmpty)
    }
    func testFailedRotationStorageStopsBeforeXbox() async throws {
        try seed(); network(); vault.failWrites = true
        let account = model()
        do { try await account.handOver(); XCTFail("must fail") } catch {}
        XCTAssertEqual(vault.values[alice.uuid]?.refresh, "fixture-old-refresh")
        XCTAssertEqual(requests.count, 1); XCTAssertTrue(bridgeCommands.isEmpty)
    }

    func testRemovingLegacyAccountDuringDeferredMigrationCannotRestoreIt() async throws {
        defaults.set(try JSONEncoder().encode(alice), forKey: "microsoftProfile")
        vault.values["account"] = AccountSecrets(refresh: "fixture-legacy", token: "fixture-valid", expires: instant.addingTimeInterval(7200))
        vault.failWrites = true
        let account = model()
        await account.remove(alice)
        vault.failWrites = false
        try await account.handOver()
        XCTAssertTrue(vault.values.isEmpty); XCTAssertTrue(account.accounts.isEmpty)
        XCTAssertNil(defaults.data(forKey: "microsoftProfile")); XCTAssertNil(account.profile)
    }

    func testSuccessfulSignInRepairsMissingSelectedAccount() async throws {
        try seed(expired: false)
        defaults.set(try JSONEncoder().encode(MicrosoftAccount.Catalog(profiles: [alice], selected: "missing")), forKey: "microsoftAccounts.v2")
        network(); let response = AuthURLProtocol.response
        AuthURLProtocol.response = { request in
            if request.url!.path.hasSuffix("/devicecode") {
                return (200, ["device_code": "fixture-device", "user_code": "FIXTURE", "verification_uri": "https://microsoft.com/link", "expires_in": 900, "interval": 1])
            }
            return response(request)
        }
        let account = model(); XCTAssertTrue(account.needsAccountSelection)
        account.signIn()
        for _ in 0..<100 {
            if account.profile != nil { break }
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTAssertFalse(account.needsAccountSelection)
        try await account.handOver()
        XCTAssertTrue(bridgeCommands.last?.contains("fixture-new-access") == true)
    }
    func testCancelledJoinReplyCannotUncoverOrClearNewLaunch() async throws {
        preparedGameFiles = try PreparedGameFilesFixture()
        let connectStarted = expectation(description: "connect started")
        var connection: CheckedContinuation<[String: Any], Never>?
        let game = GameLink { command in
            if command == "launcher-state" { return ["phase": "title"] }
            if command.hasPrefix("launcher-start ") { return ["ok": true, "phase": "title"] }
            if command.hasPrefix("launcher-commit-start ") { return ["ok": true, "status": "committed"] }
            if command.hasPrefix("launcher-cancel-start ") { return ["ok": true, "status": "stopped"] }
            if command.hasPrefix("connect ") {
                connectStarted.fulfill()
                return await withCheckedContinuation { connection = $0 }
            }
            return ["ok": true]
        }
        await game.refresh(); game.prepareSession = {}
        var uncovered = false; game.uncover = { uncovered = true }
        let join = Task { await game.join("fixture.invalid") }
        await fulfillment(of: [connectStarted], timeout: 3)
        game.cancelLaunch()
        let deadline = Date().addingTimeInterval(3)
        while game.launching != nil, Date() < deadline { await Task.yield() }
        XCTAssertNil(game.launching, "The canceled start must drain before a replacement begins")
        let preparing = expectation(description: "new launch preparing")
        var preparation: CheckedContinuation<Void, Never>?
        game.prepareSession = {
            preparing.fulfill()
            await withCheckedContinuation { preparation = $0 }
        }
        let newLaunch = Task { await game.play() }
        await fulfillment(of: [preparing], timeout: 3)
        connection?.resume(returning: ["ok": true]); await join.value
        XCTAssertFalse(uncovered)
        XCTAssertEqual(game.launching, "Preparing account…")
        game.cancelLaunch(); preparation?.resume(); await newLaunch.value
        XCTAssertFalse(uncovered); XCTAssertNil(game.launching)
    }

}

@MainActor
final class LaunchCancellationTests: XCTestCase {
    private var preparedGameFiles: PreparedGameFilesFixture?

    override func setUp() async throws {
        // These tests start/cancel a prepared game. GameLink deliberately refuses
        // a waiting engine when its resource-ready marker is absent.
        preparedGameFiles = try PreparedGameFilesFixture()
        XCTAssertTrue(GameFolder.hasGameFiles, "The launch lifecycle fixture requires prepared resources")
    }

    override func tearDown() async throws {
        try preparedGameFiles?.cleanUp()
        preparedGameFiles = nil
    }

    @MainActor
    private final class Engine {
        var phase = "waiting"
        var owner: String?
        var committed = false
        var pendingCancel: String?
        var commands: [String] = []
        var starts: [String] = []
        var commits: [String] = []
        var cancels: [String] = []
        var cancellationTaskFlags: [Bool] = []
        var onStart: ((String) -> Void)?
        var onCommit: ((String) -> Void)?
        var onCancel: ((String) -> Void)?
        var holdStart = true
        var holdCommit = false
        var holdCancel = false
        var holdCommittedCancelReply = false
        var cancelFailures = 0
        var startReplies: [String: (String, CheckedContinuation<[String: Any], Never>)] = [:]
        var commitReplies: [String: CheckedContinuation<[String: Any], Never>] = [:]
        var cancelReplies: [String: CheckedContinuation<Void, Never>] = [:]
        var running: Bool { phase != "waiting" }

        func send(_ command: String) async -> [String: Any] {
            commands.append(command)
            if command == "launcher-state" { return ["phase": phase] }
            if command.hasPrefix("launcher-start ") {
                let id = String(command.dropFirst("launcher-start ".count))
                guard pendingCancel == nil else { return ["error": "Cancellation is still draining"] }
                owner = id
                committed = committed || phase == "world"
                if phase == "waiting" { phase = "loading" }
                let replyPhase = phase
                starts.append(id)
                onStart?(id)
                if holdStart {
                    return await withCheckedContinuation { startReplies[id] = (replyPhase, $0) }
                }
                return ["ok": true, "phase": replyPhase]
            }
            if command.hasPrefix("launcher-commit-start ") {
                let id = String(command.dropFirst("launcher-commit-start ".count))
                guard owner == id, pendingCancel == nil, running else {
                    return ["ok": false, "error": "Start no longer active"]
                }
                // The backend commits before replying, independently of when
                // the caller's actor continuation receives the acknowledgment.
                committed = true
                commits.append(id)
                onCommit?(id)
                if holdCommit {
                    return await withCheckedContinuation { commitReplies[id] = $0 }
                }
                return ["ok": true, "status": "committed"]
            }
            if command.hasPrefix("launcher-cancel-start ") {
                let id = String(command.dropFirst("launcher-cancel-start ".count))
                cancels.append(id)
                cancellationTaskFlags.append(Task.isCancelled)
                onCancel?(id)
                guard owner == id else { return ["ok": true, "status": "stale"] }
                guard !committed else {
                    if holdCommittedCancelReply {
                        await withCheckedContinuation { cancelReplies[id] = $0 }
                    }
                    return ["ok": true, "status": "committed"]
                }
                pendingCancel = id
                if cancelFailures > 0 {
                    cancelFailures -= 1
                    return ["error": "Fixture transport failure"]
                }
                if holdCancel {
                    await withCheckedContinuation { cancelReplies[id] = $0 }
                }
                guard owner == id, pendingCancel == id else { return ["ok": true, "status": "stale"] }
                reset()
                return ["ok": true, "status": "stopped"]
            }
            return ["error": "Unexpected fixture command"]
        }

        func reset() {
            phase = "waiting"
            owner = nil
            pendingCancel = nil
            committed = false
        }

        func releaseStart(_ id: String) {
            guard let (phase, reply) = startReplies.removeValue(forKey: id) else { return }
            reply.resume(returning: ["ok": true, "phase": phase])
        }
        func releaseCommit(_ id: String) {
            commitReplies.removeValue(forKey: id)?.resume(returning: ["ok": true, "status": "committed"])
        }
        func releaseCancel(_ id: String) { cancelReplies.removeValue(forKey: id)?.resume() }
    }
    @MainActor
    private final class Presentation { var count = 0 }

    private func game(_ engine: Engine, _ shown: Presentation) async -> GameLink {
        let game = GameLink { await engine.send($0) }
        game.prepareSession = {}
        game.uncover = { shown.count += 1 }
        await game.refresh()
        return game
    }
    private func heldStart(_ engine: Engine, _ game: GameLink) async -> Task<Void, Never> {
        let accepted = expectation(description: "Backend accepted Play")
        engine.onStart = { _ in accepted.fulfill(); engine.onStart = nil }
        let play = Task { await game.play() }
        await fulfillment(of: [accepted], timeout: 3)
        return play
    }
    private func until(_ condition: @MainActor () -> Bool, _ message: String = "State did not settle") async {
        let deadline = Date().addingTimeInterval(3)
        while !condition(), Date() < deadline { await Task.yield() }
        XCTAssertTrue(condition(), message)
    }

    func testCancelWhileStartAcknowledgmentIsPendingStopsHiddenEngine() async {
        let engine = Engine(), shown = Presentation()
        let game = await game(engine, shown)
        let play = await heldStart(engine, game)
        let id = engine.starts[0]
        XCTAssertNotNil(game.launching, "The launcher still exposes Cancel during this await")
        XCTAssertTrue(engine.running)
        game.cancelLaunch()
        engine.releaseStart(id)
        await play.value
        await until({ game.launching == nil })
        XCTAssertEqual(shown.count, 0)
        XCTAssertFalse(engine.running)
        XCTAssertTrue(engine.commits.isEmpty, "Cancellation won before presentation commit")
    }

    func testTaskCancellationAfterAcceptedStartCannotUncoverOrLeaveHiddenEngine() async {
        let engine = Engine(), shown = Presentation()
        let game = await game(engine, shown)
        let play = await heldStart(engine, game)
        let id = engine.starts[0]
        play.cancel()
        engine.releaseStart(id)
        await play.value
        await until({ game.launching == nil })
        XCTAssertEqual(shown.count, 0)
        XCTAssertFalse(engine.running)
    }

    func testTaskCancellationRunsCleanupWithoutWaitingForStartAcknowledgment() async {
        let engine = Engine(), shown = Presentation()
        let game = await game(engine, shown)
        let play = await heldStart(engine, game)
        let id = engine.starts[0]
        play.cancel()
        await until({ game.launching == nil }, "Cleanup was inherited by the canceled caller")
        XCTAssertFalse(engine.running)
        XCTAssertEqual(engine.cancellationTaskFlags, [false], "Cleanup must run in a non-canceled task")
        XCTAssertNotNil(engine.startReplies[id], "The original start continuation must still be suspended")
        engine.releaseStart(id)
        await play.value
        XCTAssertEqual(shown.count, 0)
    }

    private func checkCancellationBeforeStart(cancelTask: Bool) async {
        let engine = Engine(), shown = Presentation()
        let game = await game(engine, shown)
        let preparing = expectation(description: "Preparing account")
        var preparation: CheckedContinuation<Void, Never>?
        game.prepareSession = {
            preparing.fulfill()
            await withCheckedContinuation { preparation = $0 }
        }
        let play = Task { await game.play() }
        await fulfillment(of: [preparing], timeout: 3)
        if cancelTask { play.cancel() } else { game.cancelLaunch() }
        preparation?.resume()
        await play.value
        XCTAssertFalse(engine.running)
        XCTAssertTrue(engine.starts.isEmpty)
        XCTAssertTrue(engine.cancels.isEmpty)
        XCTAssertEqual(shown.count, 0)
        XCTAssertNil(game.launching)
    }
    func testCancelBeforeBackendStartLeavesEngineOff() async { await checkCancellationBeforeStart(cancelTask: false) }
    func testTaskCancellationBeforeBackendStartLeavesEngineOff() async { await checkCancellationBeforeStart(cancelTask: true) }

    func testOrdinaryPlayStartsAndUncoversEngine() async {
        let engine = Engine(), shown = Presentation()
        engine.holdStart = false
        let game = await game(engine, shown)
        await game.play()
        XCTAssertTrue(engine.running)
        XCTAssertEqual(shown.count, 1)
        XCTAssertEqual(game.phase, .loading(0))
        XCTAssertEqual(engine.starts, engine.commits)
        XCTAssertNil(game.launching)
        XCTAssertNil(game.problem)
    }

    func testCachedTitleCannotSkipStartingAResetBackend() async {
        let engine = Engine(), shown = Presentation()
        engine.phase = "title"
        engine.holdStart = false
        let game = await game(engine, shown)
        XCTAssertEqual(game.phase, .title)
        engine.phase = "waiting"
        await game.play()
        XCTAssertTrue(engine.running)
        XCTAssertEqual(engine.starts.count, 1)
        XCTAssertEqual(game.phase, .loading(0))
        XCTAssertEqual(shown.count, 1)
    }

    func testNewPlayWaitsForCancellationDrainAndCleanupIsCoalesced() async {
        let engine = Engine(), shown = Presentation()
        engine.holdCancel = true
        let game = await game(engine, shown)
        let play = await heldStart(engine, game)
        let id = engine.starts[0]
        let cancelling = expectation(description: "Cancellation accepted")
        engine.onCancel = { _ in cancelling.fulfill(); engine.onCancel = nil }
        game.cancelLaunch()
        await fulfillment(of: [cancelling], timeout: 3)
        game.cancelLaunch()
        await game.play()
        XCTAssertEqual(engine.starts.count, 1)
        XCTAssertEqual(engine.cancels, [id])
        XCTAssertNotNil(game.launching, "New Play must stay disabled until physical drain acknowledgment")
        XCTAssertTrue(engine.running)
        engine.releaseCancel(id)
        await until({ game.launching == nil })
        engine.releaseStart(id)
        await play.value
        XCTAssertFalse(engine.running)
        XCTAssertEqual(shown.count, 0)
        engine.holdStart = false
        await game.play()
        XCTAssertEqual(engine.starts.count, 2)
        XCTAssertEqual(shown.count, 1)
    }

    func testLateOldStartAcknowledgmentCannotPresentOrClearANewerLaunch() async {
        let engine = Engine(), shown = Presentation()
        engine.phase = "title"
        let game = await game(engine, shown)
        let old = await heldStart(engine, game)
        let oldID = engine.starts[0]
        game.cancelLaunch()
        await until({ game.launching == nil })
        let preparing = expectation(description: "New launch preparing")
        var preparation: CheckedContinuation<Void, Never>?
        game.prepareSession = {
            preparing.fulfill()
            await withCheckedContinuation { preparation = $0 }
        }
        let next = Task { await game.play() }
        await fulfillment(of: [preparing], timeout: 3)
        engine.releaseStart(oldID)
        await old.value
        XCTAssertEqual(shown.count, 0)
        XCTAssertEqual(game.phase, .waiting)
        XCTAssertNotNil(game.launching)
        engine.holdStart = false
        preparation?.resume()
        await next.value
        XCTAssertEqual(shown.count, 1)
        XCTAssertEqual(engine.cancels, [oldID])
        XCTAssertEqual(engine.owner, engine.starts.last)
    }

    private func checkCancellationAfterCommit(cancelTask: Bool) async {
        let engine = Engine(), shown = Presentation()
        engine.holdStart = false
        engine.holdCommit = true
        let game = await game(engine, shown)
        let committed = expectation(description: "Presentation committed before reply")
        engine.onCommit = { _ in committed.fulfill(); engine.onCommit = nil }
        let play = Task { await game.play() }
        await fulfillment(of: [committed], timeout: 3)
        let id = engine.starts[0]
        if cancelTask { play.cancel() } else { game.cancelLaunch() }
        await until({ shown.count == 1 })
        XCTAssertTrue(engine.running)
        XCTAssertNotEqual(game.phase, .waiting)
        engine.releaseCommit(id)
        await play.value
        XCTAssertEqual(shown.count, 1, "Late commit/cancel replies must not uncover twice")
        XCTAssertNil(game.launching)
    }
    func testCancelAfterPresentationCommitMustUncoverOnce() async { await checkCancellationAfterCommit(cancelTask: false) }
    func testTaskCancellationAfterPresentationCommitMustUncoverOnce() async { await checkCancellationAfterCommit(cancelTask: true) }

    func testCommittedExistingWorldIsNotStoppedByCancel() async {
        let engine = Engine(), shown = Presentation()
        engine.phase = "world"
        let game = await game(engine, shown)
        let play = await heldStart(engine, game)
        let id = engine.starts[0]
        game.cancelLaunch()
        await until({ shown.count == 1 })
        XCTAssertEqual(engine.phase, "world")
        engine.releaseStart(id)
        await play.value
        XCTAssertEqual(shown.count, 1)
    }

    func testUnprovenCleanupDoesNotEnablePlayAndCanRetry() async {
        let engine = Engine(), shown = Presentation()
        engine.cancelFailures = 1
        let game = await game(engine, shown)
        let play = await heldStart(engine, game)
        let id = engine.starts[0]
        game.cancelLaunch()
        await until({ game.problem != nil })
        XCTAssertNotNil(game.launching)
        XCTAssertTrue(engine.running)
        await game.play()
        XCTAssertEqual(engine.starts.count, 1)
        game.cancelLaunch()
        await until({ game.launching == nil })
        engine.releaseStart(id)
        await play.value
        XCTAssertFalse(engine.running)
        XCTAssertEqual(shown.count, 0)
        XCTAssertEqual(engine.cancels, [id, id])
    }
    func testLateCommittedStartReplyCannotPresentOrClearANewerLaunch() async {
        let engine = Engine(), shown = Presentation()
        engine.holdStart = false
        engine.holdCommit = true
        let game = await game(engine, shown)
        let committed = expectation(description: "First presentation committed")
        engine.onCommit = { _ in committed.fulfill(); engine.onCommit = nil }
        let old = Task { await game.play() }
        await fulfillment(of: [committed], timeout: 3)
        let oldID = engine.starts[0]
        game.cancelLaunch()
        await until({ shown.count == 1 })
        let preparing = expectation(description: "Replacement preparing")
        var preparation: CheckedContinuation<Void, Never>?
        game.prepareSession = {
            preparing.fulfill()
            await withCheckedContinuation { preparation = $0 }
        }
        let next = Task { await game.play() }
        await fulfillment(of: [preparing], timeout: 3)
        engine.releaseCommit(oldID)
        await old.value
        XCTAssertEqual(shown.count, 1)
        XCTAssertNotNil(game.launching)
        engine.holdCommit = false
        preparation?.resume()
        await next.value
        XCTAssertEqual(shown.count, 2)
        XCTAssertEqual(engine.cancels, [oldID])
        XCTAssertEqual(engine.owner, engine.starts.last)
    }

    func testLateCommittedCancellationReplyCannotPresentANewerLaunch() async {
        let engine = Engine(), shown = Presentation()
        engine.holdStart = false
        engine.holdCommit = true
        engine.holdCommittedCancelReply = true
        let game = await game(engine, shown)
        let committed = expectation(description: "Presentation committed")
        engine.onCommit = { _ in committed.fulfill(); engine.onCommit = nil }
        let old = Task { await game.play() }
        await fulfillment(of: [committed], timeout: 3)
        let oldID = engine.starts[0]
        game.cancelLaunch()
        await until({ engine.cancelReplies[oldID] != nil })
        engine.releaseCommit(oldID)
        await old.value
        XCTAssertEqual(shown.count, 1)
        let preparing = expectation(description: "New launch preparing")
        var preparation: CheckedContinuation<Void, Never>?
        game.prepareSession = {
            preparing.fulfill()
            await withCheckedContinuation { preparation = $0 }
        }
        let next = Task { await game.play() }
        await fulfillment(of: [preparing], timeout: 3)
        engine.releaseCancel(oldID)
        await Task.yield()
        XCTAssertEqual(shown.count, 1)
        XCTAssertNotNil(game.launching)
        engine.holdCommit = false
        preparation?.resume()
        await next.value
        XCTAssertEqual(shown.count, 2)
        XCTAssertEqual(engine.cancels, [oldID])
    }

    func testNewOwnedStartPreservesCommittedTitleAndCancelUncoversOnce() async {
        let engine = Engine(), shown = Presentation()
        engine.phase = "title"
        engine.committed = true
        let game = await game(engine, shown)
        let play = await heldStart(engine, game)
        let id = engine.starts[0]
        XCTAssertTrue(engine.committed, "A new owner must not undo an existing presentation commit")
        game.cancelLaunch()
        await until({ shown.count == 1 })
        XCTAssertTrue(engine.running)
        XCTAssertEqual(engine.phase, "title")
        engine.releaseStart(id)
        await play.value
        XCTAssertEqual(shown.count, 1)
        XCTAssertEqual(game.phase, .title)
    }

    func testCachedCommittedTitleAfterBackendResetStartsCancellableFreshEngine() async {
        let engine = Engine(), shown = Presentation()
        engine.phase = "title"
        engine.committed = true
        let game = await game(engine, shown)
        engine.reset()
        XCTAssertEqual(game.phase, .title, "The client deliberately still has its old cached phase")
        let play = await heldStart(engine, game)
        let id = engine.starts[0]
        XCTAssertFalse(engine.committed, "An explicit reset starts a fresh uncommitted generation")
        game.cancelLaunch()
        engine.releaseStart(id)
        await play.value
        await until({ game.launching == nil })
        XCTAssertFalse(engine.running)
        XCTAssertEqual(shown.count, 0)
        XCTAssertTrue(engine.commits.isEmpty)
        XCTAssertEqual(game.phase, .waiting)
    }

}
