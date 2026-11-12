"""Exercise the real iOS bounded log writer without building the renderer."""
import pathlib, subprocess, tempfile, unittest
ROOT = pathlib.Path(__file__).resolve().parents[1]
class MobileDiagnosticsTests(unittest.TestCase):
    def test_rotation_and_redaction(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            source = '#[path="' + str(ROOT / "crates/horizon-client-render/src/mobile_log.rs") + '"] mod mobile_log;\n' + SOURCE
            (root / "test.rs").write_text(source)
            subprocess.run(["rustc", "--edition=2024", "--test", str(root / "test.rs"), "-o", str(root / "test")], check=True)
            subprocess.run([str(root / "test")], check=True)
SOURCE = r'''
#[test] fn privacy_and_rotation() {
 use std::{fs,io::{Read,Seek,SeekFrom}};
 let d=std::env::temp_dir().join(format!("horizon-log-test-{}",std::process::id()));fs::create_dir_all(&d).unwrap();
 let mut f=fs::OpenOptions::new().create(true).truncate(true).read(true).write(true).open(d.join("latest.log")).unwrap();
 for secret in ["Authorization: Bearer abc", "{\"refresh_token\":\n\"short-secret\"}","XBL3.0 x=secret"] {assert!(!mobile_log::redact(secret).contains("secret"));}
 assert!(mobile_log::redact("shader compilation: unexpected token near main()").contains("unexpected token"));
 for _ in 0..90 {mobile_log::append(&mut f,&d,&"shader failure details ".repeat(1000)).unwrap();}
 assert!(f.metadata().unwrap().len() <= mobile_log::LIMIT as u64);
 assert!(fs::metadata(d.join("previous.log")).unwrap().len() <= mobile_log::LIMIT as u64);
 mobile_log::append(&mut f,&d,"access_token=never-export-this").unwrap();f.seek(SeekFrom::Start(0)).unwrap();let mut s=String::new();f.read_to_string(&mut s).unwrap();assert!(!s.contains("never-export-this"));fs::remove_dir_all(d).unwrap();
}
'''
