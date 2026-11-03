import Foundation
import Security

struct AccountSecrets: Codable, Equatable {
    let refresh: String
    let token: String
    let expires: Date
}

/// Injectable for tests: fixtures never access the user's Keychain.
protocol AccountVault {
    func read(_ account: String) throws -> AccountSecrets?
    func write(_ secrets: AccountSecrets, for account: String) throws
    func remove(_ account: String) throws
}

struct KeychainAccountVault: AccountVault {
    private func query(_ account: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: "dev.horizon.launcher.microsoft",
         kSecAttrAccount as String: account]
    }
    func read(_ account: String) throws -> AccountSecrets? {
        var search = query(account)
        search[kSecReturnData as String] = true
        search[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        let status = SecItemCopyMatching(search as CFDictionary, &item)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = item as? Data else { throw storageError() }
        guard let secrets = try? JSONDecoder().decode(AccountSecrets.self, from: data) else {
            throw AccountError("Saved sign-in data could not be read. Sign in again.", reauthenticate: true)
        }
        return secrets
    }
    func write(_ secrets: AccountSecrets, for account: String) throws {
        let data = try JSONEncoder().encode(secrets)
        // Update atomically. Never delete a working credential before its replacement is saved.
        let values: [String: Any] = [kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly]
        var status = SecItemUpdate(query(account) as CFDictionary, values as CFDictionary)
        if status == errSecItemNotFound {
            var item = query(account)
            values.forEach { item[$0.key] = $0.value }
            status = SecItemAdd(item as CFDictionary, nil)
        }
        guard status == errSecSuccess else { throw storageError() }
    }
    func remove(_ account: String) throws {
        let status = SecItemDelete(query(account) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw storageError() }
    }
    private func storageError() -> AccountError {
        AccountError("Sign-in could not be saved securely. Unlock this device and try again.")
    }
}

struct AccountError: LocalizedError {
    let errorDescription: String?
    let reauthenticate: Bool
    init(_ message: String, reauthenticate: Bool = false) {
        errorDescription = message; self.reauthenticate = reauthenticate
    }
}

/// HTTPS authentication requests. Responses never enter logs or error strings.
struct AccountNet {
    let session: URLSession
    func form(_ url: String, _ fields: [String: String], allowErrors: Bool = false) async throws -> [String: Any] {
        var request = URLRequest(url: URL(string: url)!)
        request.httpMethod = "POST"
        request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        var allowed = CharacterSet.alphanumerics; allowed.insert(charactersIn: "-._~")
        request.httpBody = fields.map { "\($0.key)=\($0.value.addingPercentEncoding(withAllowedCharacters: allowed) ?? "")" }
            .joined(separator: "&").data(using: .utf8)
        return try await send(request, allowErrors: allowErrors)
    }
    func json(_ url: String, _ body: [String: Any], allowErrors: Bool = false) async throws -> [String: Any] {
        var request = URLRequest(url: URL(string: url)!)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        return try await send(request, allowErrors: allowErrors)
    }
    func get(_ url: String, token: String, allowErrors: Bool = false) async throws -> [String: Any] {
        var request = URLRequest(url: URL(string: url)!)
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        return try await send(request, allowErrors: allowErrors)
    }
    private func send(_ input: URLRequest, allowErrors: Bool) async throws -> [String: Any] {
        try Task.checkCancellation()
        var request = input; request.timeoutInterval = 30
        let result: (Data, URLResponse)
        do { result = try await session.data(for: request) }
        catch {
            try Task.checkCancellation()
            throw AccountError("Could not reach the sign-in service. Check your connection and try again.")
        }
        try Task.checkCancellation()
        let status = (result.1 as? HTTPURLResponse)?.statusCode ?? 0
        guard var object = (try? JSONSerialization.jsonObject(with: result.0)) as? [String: Any] else {
            throw AccountError("The sign-in service returned an incomplete response. Try again.")
        }
        if !(200..<300).contains(status) {
            guard allowErrors else {
                throw AccountError("The sign-in service is unavailable (\(status)). Try again.")
            }
            object["status"] = status
        }
        return object
    }
}
