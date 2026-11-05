"""Game-file credential replacement and download gating with fake Security APIs.

The production Keychain enum and start method are compiled unchanged. No real
Keychain, URLSession, UserDefaults, or player files are touched.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def swift_block(source, start):
    opening = source.index("{", start)
    end, depth = opening + 1, 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


def program(source=None):
    source = source or (ROOT / "packaging/ios/Launcher/GameFiles.swift").read_text()
    vault = swift_block(source, source.index("private enum Keychain"))
    start = swift_block(source, source.index("    func start()"))
    return PREAMBLE + vault + "\nfinal class StartProbe {\n" + START_STUBS + start + "\n}\n" + CASES


PREAMBLE = r'''
import Foundation
// Module-local names deliberately shadow system APIs. These tests cannot use
// the device Keychain even when run on macOS.
typealias CFDictionary = NSDictionary
typealias CFTypeRef = AnyObject
typealias OSStatus = Int32
let errSecSuccess:OSStatus = 0
let errSecItemNotFound:OSStatus = -25300
let errSecInteractionNotAllowed:OSStatus = -25308
let kSecClass = "class", kSecClassGenericPassword = "password"
let kSecAttrService = "service", kSecAttrAccount = "account"
let kSecReturnData = "return", kSecValueData = "data"
let kSecAttrAccessible = "access", kSecAttrAccessibleAfterFirstUnlock = "unlocked"
enum SecurityFixture {
 static var data:Data? = Data("old-working-token".utf8)
 static var update:OSStatus = errSecSuccess
 static var add:OSStatus = errSecSuccess
 static var remove:OSStatus = errSecSuccess
 static var calls:[String] = []
 static func reset(){data=Data("old-working-token".utf8);update=errSecSuccess;add=errSecSuccess;remove=errSecSuccess;calls=[];UserDefaults.standard.values=[:]}
}
func SecItemCopyMatching(_ query:CFDictionary,_ result:UnsafeMutablePointer<CFTypeRef?>?) -> OSStatus {
 guard let data=SecurityFixture.data else{return errSecItemNotFound}
 result?.pointee=data as NSData;return errSecSuccess
}
func SecItemUpdate(_ query:CFDictionary,_ values:CFDictionary) -> OSStatus {
 SecurityFixture.calls.append("update")
 guard SecurityFixture.update == errSecSuccess else{return SecurityFixture.update}
 guard SecurityFixture.data != nil else{return errSecItemNotFound}
 SecurityFixture.data=values[kSecValueData] as? Data;return errSecSuccess
}
func SecItemAdd(_ values:CFDictionary,_ result:UnsafeMutablePointer<CFTypeRef?>?) -> OSStatus {
 SecurityFixture.calls.append("add")
 guard SecurityFixture.add == errSecSuccess else{return SecurityFixture.add}
 SecurityFixture.data=values[kSecValueData] as? Data;return errSecSuccess
}
func SecItemDelete(_ query:CFDictionary) -> OSStatus {
 SecurityFixture.calls.append("delete")
 guard SecurityFixture.remove == errSecSuccess else{return SecurityFixture.remove}
 guard SecurityFixture.data != nil else{return errSecItemNotFound}
 SecurityFixture.data=nil;return errSecSuccess
}
struct DownloadError:LocalizedError {
 let errorDescription:String?
 init(_ message:String){errorDescription=message}
}
final class UserDefaults {
 static let standard=UserDefaults()
 var values:[String:String]=[:]
 func set(_ value:String,forKey key:String){values[key]=value}
}
'''
START_STUBS = r'''
 enum State {case idle,failed(String)}
 var busy=false,resourcesOnly=true
 var attempt=UUID()
 var link=" https://fixture.invalid/archive.zip "
 var token=" replacement-token "
 var state=State.idle
 var requests:[(URL,String?)]=[]
 func begin(url:URL,token:String?){requests.append((url,token))}
'''
CASES = r'''
var failures=0
func expect(_ value:Bool,_ message:String){if !value{failures+=1;print("FAIL: \(message)")}}
func expectFailure(_ action:() throws -> Void,_ message:String){do{try action();expect(false,message)}catch{}}
func check(_ name:String,_ action:() throws -> Void){SecurityFixture.reset();do{try action()}catch{expect(false,"\(name): unexpected error \(error)")}}
check("failed replacement"){
 SecurityFixture.update=errSecInteractionNotAllowed
 expectFailure({try Keychain.write("replacement")},"failed replacement must throw")
 expect(SecurityFixture.data == Data("old-working-token".utf8),"failed replacement must preserve prior credential")
 expect(SecurityFixture.calls == ["update"],"permission failure must not delete or add")
}
check("successful replacement"){
 try Keychain.write("replacement")
 expect(SecurityFixture.data == Data("replacement".utf8),"replacement persisted")
 expect(SecurityFixture.calls == ["update"],"existing item updates atomically")
}
check("new credential"){
 SecurityFixture.data=nil;try Keychain.write("new")
 expect(SecurityFixture.data == Data("new".utf8),"new credential added")
 expect(SecurityFixture.calls == ["update","add"],"add only after missing item")
}
check("failed add"){
 SecurityFixture.data=nil;SecurityFixture.add=errSecInteractionNotAllowed
 expectFailure({try Keychain.write("new")},"add failure must throw")
 expect(SecurityFixture.data == nil,"failed add leaves no credential")
}
check("failed clear"){
 SecurityFixture.remove=errSecInteractionNotAllowed
 expectFailure({try Keychain.write(nil)},"clear failure must throw")
 expect(SecurityFixture.data == Data("old-working-token".utf8),"clear failure preserves credential")
}
check("clear and already absent"){
 try Keychain.write(nil);expect(SecurityFixture.data == nil,"explicit clear removes credential")
 try Keychain.write(nil);expect(SecurityFixture.calls == ["delete","delete"],"already absent is successful clear")
}
check("failed save gates download"){
 SecurityFixture.update=errSecInteractionNotAllowed
 let model=StartProbe();model.start()
 expect(model.requests.isEmpty,"failed save must not start network download")
 if case let .failed(reason)=model.state{expect(!reason.isEmpty,"visible storage error")}else{expect(false,"failed save must be visible")}
 expect(UserDefaults.standard.values.isEmpty,"failed save must not replace saved link")
}
check("successful save starts download"){
 let model=StartProbe();model.start()
 expect(model.requests.count == 1,"saved credential starts one download")
 expect(model.requests.first?.1 == "replacement-token","download uses trimmed token")
 expect(SecurityFixture.data == Data("replacement-token".utf8),"saved token matches request")
 expect(UserDefaults.standard.values["gameFilesLink"] == "https://fixture.invalid/archive.zip","saved link matches request")
}
if failures > 0 {exit(1)}
print("8 credential/download cases passed")
'''


class GameFilesKeychainTests(unittest.TestCase):
    def test_atomic_credentials_and_actual_start_gate(self):
        compiler = shutil.which("swiftc")
        if not compiler:
            self.skipTest("swiftc is required for production Swift credential checks")
        with tempfile.TemporaryDirectory(prefix="horizon-download-credentials-") as folder:
            source = Path(folder) / "probe.swift"
            binary = Path(folder) / "probe"
            source.write_text(program())
            built = subprocess.run([compiler, str(source), "-o", str(binary)],
                                   capture_output=True, text=True, timeout=60)
            self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
            self.assertIn("8 credential/download cases passed", run.stdout)
