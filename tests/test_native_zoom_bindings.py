"""Native zoom conflict labels include independent Horizon key bindings."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from test_mobile_command_keyboard import method
from test_mobile_multiplayer_keyboard import block
ROOT = Path(__file__).resolve().parents[1]

class NativeZoomBindingsTests(unittest.TestCase):
    def test_actual_native_conflict_lookup_tracks_emote_and_vanilla_rebinding(self):
        compiler = shutil.which('rustc')
        if not compiler:
            self.skipTest('rustc required')
        keys = (ROOT / 'crates/horizon-client-input/src/keybinds.rs').read_text()
        native = (ROOT / 'crates/horizon-client-render/src/extras/native_ios.rs').read_text()
        mappings = keys[keys.index('pub const MAPPINGS:'):keys.index('/// Mappings Options builds')]
        default = block(keys, keys.index('{', keys.index('impl Default for KeyBinds')))[1:-1]
        methods = '\n'.join('pub '+method(keys,n) for n in ['get','set','saved','horizon_saved'])
        source = '''use std::collections::BTreeMap;
mod keybinds { use super::*;
''' + mappings + '''
pub struct KeyBinds {keys:BTreeMap<&'static str,String>}
impl Default for KeyBinds {''' + default + '''}
impl KeyBinds {''' + methods + '''}
}
struct Extras { zoom_key:String }
''' + method(native,'zoom_key_conflicts') + '''
fn main(){
 let mut keys=keybinds::KeyBinds::default();let mut extras=Extras{zoom_key:"key.keyboard.b".into()};
 assert!(zoom_key_conflicts(&extras,&keys).contains(&"key.emote"));
 keys.set("key.emote","key.keyboard.g");assert!(!zoom_key_conflicts(&extras,&keys).contains(&"key.emote"));
 extras.zoom_key="key.keyboard.g".into();keys.set("key.jump","key.keyboard.g");
 let conflicts=zoom_key_conflicts(&extras,&keys);assert!(conflicts.contains(&"key.jump"));assert!(conflicts.contains(&"key.emote"));
 assert!(!keys.saved().iter().any(|(n,_)|*n=="key.emote"));
 assert_eq!(keys.horizon_saved(),vec![("key.emote","key.keyboard.g".into())]);
 extras.zoom_key="key.keyboard.unknown".into();keys.set("key.emote","key.keyboard.unknown");
 assert!(zoom_key_conflicts(&extras,&keys).is_empty());
}
'''
        with tempfile.TemporaryDirectory(prefix='horizon-zoom-bindings-') as folder:
            path=Path(folder)/'probe.rs';binary=Path(folder)/'probe';path.write_text(source)
            built=subprocess.run([compiler,'--edition=2024',str(path),'-o',str(binary)],capture_output=True,text=True,timeout=60)
            self.assertEqual(built.returncode,0,built.stdout+built.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True,timeout=10)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
