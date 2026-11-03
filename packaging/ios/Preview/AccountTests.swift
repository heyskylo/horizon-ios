import XCTest
@testable import LauncherPreview

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
        let connectStarted = expectation(description: "connect started")
        var connection: CheckedContinuation<[String: Any], Never>?
        let game = GameLink { command in
            if command == "launcher-state" { return ["phase": "title"] }
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
