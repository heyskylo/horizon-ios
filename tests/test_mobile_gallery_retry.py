"""Production thumbnail coalescing and retry with deterministic fake image IO.

Only UIKit's file/image decoder and Published are doubled; the actual cache and
async completion code run unchanged, without reading player screenshots.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


def program(source=None):
    source = source or (ROOT / "packaging/ios/Launcher/Gallery.swift").read_text()
    start = source.index("@MainActor\nfinal class Thumbnails")
    end = source.index("\nstruct GalleryPage", start)
    return PREAMBLE + source[start:end] + CASES


PREAMBLE = r'''
import Foundation
protocol ObservableObject {}
enum PublishedFixture {
 static let lock=NSLock()
 static var count=0
 static func changed(){lock.lock();defer{lock.unlock()};count+=1}
 static func total()->Int{lock.lock();defer{lock.unlock()};return count}
}
@propertyWrapper struct Published<T>{var wrappedValue:T{didSet{PublishedFixture.changed()}}}
enum ImageFixture {
 static let lock=NSLock()
 static var available=false
 static var reads=0
 static func set(_ value:Bool){lock.lock();defer{lock.unlock()};available=value;reads=0}
 static func open()->Bool{lock.lock();defer{lock.unlock()};reads+=1;return available}
 static func total()->Int{lock.lock();defer{lock.unlock()};return reads}
}
actor ThumbnailGate {
 static let shared=ThumbnailGate()
 var blocked=false
 var waiters:[CheckedContinuation<Void,Never>]=[]
 func block(){blocked=true}
 func release(){blocked=false;let pending=waiters;waiters=[];pending.forEach{$0.resume()}}
 func wait() async {if blocked{await withCheckedContinuation{waiters.append($0)}}}
}
final class UIImage:@unchecked Sendable {
 init?(contentsOfFile:String){if !ImageFixture.open(){return nil}}
 func byPreparingThumbnail(ofSize:CGSize) async -> UIImage? {await ThumbnailGate.shared.wait();return self}
}
'''
CASES = r'''
@main struct Probe {
 @MainActor static var failures=0
 @MainActor static func expect(_ value:Bool,_ message:String){if !value{failures+=1;print("FAIL: \(message)")}}
 @MainActor static func until(_ predicate:()->Bool) async -> Bool {
  let deadline=Date().addingTimeInterval(3)
  while !predicate(),Date()<deadline{await Task.yield()}
  return predicate()
 }
 @MainActor static func main() async {
  let url=URL(fileURLWithPath:"/fixture/thumbnail.png")
  do {
   let model=Thumbnails();ImageFixture.set(false);let previous=PublishedFixture.total()
   model.load(url,side:110)
   let completed=await until{PublishedFixture.total()>previous}
   expect(completed,"first unavailable image decode completed")
   expect(model.images[url] == nil,"unavailable image stays blank")
   ImageFixture.set(true);model.load(url,side:110)
   let loaded=await until{model.images[url] != nil}
   expect(loaded,"retry must load a now-available image")
   expect(ImageFixture.total()==1,"retry actually reads the now-valid file")
  }
  do {
   let model=Thumbnails();ImageFixture.set(true);await ThumbnailGate.shared.block()
   model.load(url,side:110);model.load(url,side:110)
   let started=await until{ImageFixture.total()>0};expect(started,"first image IO starts")
   expect(ImageFixture.total()==1,"simultaneous loads share one in-flight decode")
   expect(model.images[url] == nil,"blocked decoder has not published")
   await ThumbnailGate.shared.release()
   let loaded=await until{model.images[url] != nil};expect(loaded,"coalesced decode publishes")
  }
  do {
   let model=Thumbnails();ImageFixture.set(true);model.load(url,side:110)
   let loaded=await until{model.images[url] != nil};expect(loaded,"successful initial decode publishes")
   model.load(url,side:110);model.load(url,side:110)
   expect(ImageFixture.total()==1,"successful cache avoids further file reads")
  }
  if failures>0{exit(1)}
  print("3 thumbnail retry/cache cases passed")
 }
}
'''


class GalleryThumbnailTests(unittest.TestCase):
    def test_failed_thumbnail_can_retry_without_duplicating_successful_work(self):
        compiler = shutil.which("swiftc")
        if not compiler:
            self.skipTest("swiftc is required for production thumbnail checks")
        with tempfile.TemporaryDirectory(prefix="horizon-gallery-thumbnail-") as folder:
            source = Path(folder) / "probe.swift"
            binary = Path(folder) / "probe"
            source.write_text(program())
            built = subprocess.run([compiler, "-parse-as-library", str(source), "-o", str(binary)],
                                   capture_output=True, text=True, timeout=60)
            self.assertEqual(built.returncode, 0, built.stdout + built.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True, timeout=15)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
            self.assertIn("3 thumbnail retry/cache cases passed", run.stdout)
