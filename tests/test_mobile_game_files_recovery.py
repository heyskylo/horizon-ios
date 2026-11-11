"""Run the production native launch gate and resource-only ZIP import on scratch files."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_game_files_keychain import swift_block

ROOT = Path(__file__).resolve().parents[1]


class GameFilesRecoveryTests(unittest.TestCase):
    def test_missing_files_gate_and_resource_only_import(self):
        if not shutil.which('swiftc'):
            self.skipTest('swiftc required')
        link = (ROOT / 'packaging/ios/Launcher/GameLink.swift').read_text()
        files = (ROOT / 'packaging/ios/Launcher/GameFiles.swift').read_text()
        gate = swift_block(link, link.index('    private func beginLaunch('))
        importer = swift_block(files, files.index('    func importPreparedFile('))
        failure = swift_block(files, files.index('    var automaticFailure:'))
        source = r'''
import Foundation
struct Phase: Equatable {
 let value:Int
 static let waiting=Self(value:0), title=Self(value:1), world=Self(value:2), loading=Self(value:3)
}
'''
        # Production uses an associated-value enum; keep its real matching semantics.
        source = source[:source.index('struct Phase')] + r'''
enum Phase: Equatable {case waiting,title,world,loading,needsFiles(String?)}
enum GameFolder {static var hasGameFiles=false}
final class LaunchAttempt {}
final class Gate {
 var phase=Phase.waiting;var requestingGameFiles=false
 var launching:String?;var currentLaunch:LaunchAttempt?;var phaseEpoch=UUID()
 GATE
 func launch()->Bool {beginLaunch("Play") != nil}
}
extension Dictionary where Key == String, Value == Any {var error:String? {self["error"] as? String}}
enum Haptics {static func failed(){};static func done(){}}
@MainActor final class Importer {
 nonisolated static let maximumArchiveBytes:Int64=1024
 enum State:Equatable {case idle,installing,done(String),failed(String)}
 var busy=false;var resourcesOnly=false;var attempt=UUID();var state=State.idle;var calls=[String]();var reject=false
 static var destination:URL {URL(fileURLWithPath:CommandLine.arguments[1]).appendingPathComponent("copy.zip")}
 func importArchive(_ line:String) async->[String:Any] {
  precondition(state == .installing);precondition(FileManager.default.fileExists(atPath:Self.destination.path))
  calls.append(line);return reject ? ["error":"Not prepared resources"] : ["kind":"resources"]
 }
 static func describe(_ answer:[String:Any])->String {"Installed"}
 FAILURE
 IMPORTER
}
@main struct Probe {
 static func main() async throws {
  for phase in [Phase.waiting,.needsFiles(nil),.needsFiles("Missing manifest")] {
   let g=Gate();g.phase=phase;GameFolder.hasGameFiles=false
   precondition(!g.launch());precondition(g.requestingGameFiles);precondition(g.launching == nil)
  }
  for phase in [Phase.waiting,.title,.world,.loading] {
   let g=Gate();g.phase=phase;GameFolder.hasGameFiles=true
   precondition(g.launch());precondition(!g.requestingGameFiles)
  }
  let active=Gate();active.launching="Starting";precondition(!active.launch());precondition(!active.requestingGameFiles)
  let url=URL(fileURLWithPath:CommandLine.arguments[1]).appendingPathComponent("source.zip")
  try Data("fixture".utf8).write(to:url)
  let i=Importer();await i.importPreparedFile(url)
  precondition(i.state == .done("Installed"));precondition(i.calls.count == 1)
  precondition(i.calls[0].hasPrefix("import-game-files "));precondition(!FileManager.default.fileExists(atPath:Importer.destination.path))
  precondition(FileManager.default.fileExists(atPath:url.path))
  i.resourcesOnly=true;i.state = .failed("Connection refused")
  precondition(i.automaticFailure == "Connection refused")
  i.state = .idle;precondition(i.automaticFailure == nil)
  i.state = .installing;precondition(i.automaticFailure == nil)
  i.state = .failed("Custom-link failure");i.resourcesOnly=false
  precondition(i.automaticFailure == nil)
  i.resourcesOnly=true
  i.reject=true;await i.importPreparedFile(url);precondition(i.state == .failed("Not prepared resources"));precondition(i.automaticFailure == nil)
  precondition(!FileManager.default.fileExists(atPath:Importer.destination.path))
  // The file limit is enforced before copying/importing a ZIP.
  try Data(repeating: 1, count: 1025).write(to:url)
  await i.importPreparedFile(url)
  if case .failed = i.state {} else {preconditionFailure("Oversized files must fail")}
  precondition(i.calls.count == 2)
  precondition(!FileManager.default.fileExists(atPath:Importer.destination.path))
  i.busy=true;await i.importPreparedFile(url);precondition(i.calls.count == 2)
  i.busy=false;await i.importPreparedFile(url.appendingPathComponent("missing"))
  if case .failed = i.state {} else {preconditionFailure("File errors must be visible")}
 }
}
'''
        source = source.replace('GATE', gate).replace('IMPORTER', importer).replace('FAILURE', failure)
        with tempfile.TemporaryDirectory(prefix='horizon-recovery-') as directory:
            probe = Path(directory) / 'probe.swift'
            binary = Path(directory) / 'probe'
            probe.write_text(source)
            build = subprocess.run(['swiftc', '-parse-as-library', str(probe), '-o', str(binary)], capture_output=True, text=True)
            self.assertEqual(build.returncode, 0, build.stdout + build.stderr)
            run = subprocess.run([str(binary), directory], capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
