"""Compile production terrain inputs through Naga and Metal, without a GPU/resources.

Optionally set HORIZON_TEST_SHADER_PACK to a locally owned Complementary 5.9.3 zip.
The fixture reuses the generator method and constants; only atlas dimensions are stubbed.
"""
import json, os, pathlib, re, subprocess, tempfile, unittest
from test_mobile_command_keyboard import method
ROOT = pathlib.Path(__file__).resolve().parents[1]
class ShaderInputTests(unittest.TestCase):
    def test_production_terrain_and_shadow_inputs_compile(self):
        source = (ROOT/'crates/horizon-client-render/src/gfx/pack_atlas.rs').read_text()
        constants = source[source.index('const CORNER_FN:'):source.index('/// GLSL: whether a terrain vertex')]
        state_words = re.search(r'TERRAIN_STATE_WORDS: usize = (\d+)', (ROOT/'crates/horizon-client-render/src/gfx.rs').read_text())[1]
        quad_words = re.search(r'WORDS: usize = (\d+)', (ROOT/'crates/horizon-client-render/src/gfx/terrain_metadata.rs').read_text())[1]
        body = method(source, 'terrain_inputs').replace('super::TERRAIN_STATE_WORDS',state_words).replace('super::terrain_metadata::WORDS',quad_words)
        lines = method((ROOT/'crates/horizon-client-render/src/gfx/pack_lines.rs').read_text(),'inputs').replace('fn inputs(', 'fn line_inputs(')
        with tempfile.TemporaryDirectory(prefix='horizon-shader-inputs-') as directory:
            work = pathlib.Path(directory);(work/'src').mkdir()
            manifest = '[package]\nname="mobile-shader-input-regression"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\nhorizon-shaders={path='+json.dumps(str(ROOT/'crates/horizon-shaders'))+'}\nnaga={version="=30.0.1",features=["msl-out"]}\n[patch.crates-io]\nnaga={path='+json.dumps(str(ROOT/'third_party/naga'))+'}\n'
            (work/'Cargo.toml').write_text(manifest)
            (work/'src/main.rs').write_text('use horizon_shaders::*;\nstruct PackAtlas{columns:u32,sprite:u32,separate_ao:bool}\nimpl PackAtlas{fn size(&self)->[u32;2]{[256,256]}\n'+body+'}\n'+constants+'\n'+lines+'\n'+(ROOT/'tests/fixtures/mobile_shader_inputs.rs').read_text())
            env = os.environ.copy();env.setdefault('CARGO_TARGET_DIR',str(ROOT/'target'));env['CARGO_INCREMENTAL']='0';env['CARGO_PROFILE_DEV_DEBUG']='0'
            args = ['cargo','run','--quiet','--manifest-path',str(work/'Cargo.toml'),'--']
            if pack := os.environ.get('HORIZON_TEST_SHADER_PACK'): args.append(pack)
            result = subprocess.run(args,env=env,capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stdout+result.stderr)
            self.assertIn('production terrain/shadow input variants: pass',result.stdout)
