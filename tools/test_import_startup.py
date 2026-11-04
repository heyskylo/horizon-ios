#!/usr/bin/env python3
"""Exercise the phone's shared first-run importer through a real desktop game.

Requires the player's prepared resources. A loopback server supplies a private
archive to a scratch game that initially has no resources. The game must reach
its title without restarting. HORIZON_IMPORT_SCREEN opts the desktop into this
flow; this is not a physical iOS or shader-visual check.
"""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time
import zipfile

import lab
from agent_control_e2e import game_env
from integration_env import game_environment


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, required=True,
                        help='directory containing prebuilt horizon-client and horizon-ctl')
    parser.add_argument('--resources', type=Path, default=lab.RESOURCES)
    parser.add_argument('--out', type=Path, default=lab.LOCAL / 'import-startup')
    parser.add_argument('--scratch-parent', type=Path,
                        help='place temporary archive and installed tree on this volume')
    parser.add_argument('--timeout', type=float, default=240)
    args = parser.parse_args()
    resources, bins, out = args.resources.resolve(), args.bin_dir.resolve(), args.out.resolve()
    marker = resources / 'horizon-resources.json'
    if not marker.is_file():
        parser.error(f'no prepared resources in {resources}')
    if args.timeout <= 0:
        parser.error('--timeout must be positive')
    suffix = '.exe' if os.name == 'nt' else ''
    game, ctl = bins / ('horizon-client' + suffix), bins / ('horizon-ctl' + suffix)
    if not game.is_file() or not ctl.is_file():
        parser.error('--bin-dir must contain both prebuilt binaries')
    out.mkdir(parents=True, exist_ok=True)
    result = {'pass': False, 'requests': [], 'forced_stop': False}
    with tempfile.TemporaryDirectory(prefix='horizon-import-startup-', dir=args.scratch_parent) as temporary:
        base = Path(temporary)
        work = base / 'work'
        work.mkdir()
        archive = base / 'game-files.zip'
        with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED, compresslevel=1) as writer:
            for path in resources.rglob('*'):
                if path.is_file():
                    writer.write(path, Path(lab.VERSION) / path.relative_to(resources))
        route = f'/{lab.VERSION}.zip'

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_GET(self):
                result['requests'].append({
                    'path': self.path,
                    'authorization_present': self.headers.get('Authorization') is not None,
                })
                if self.path != route:
                    self.send_error(404)
                    return
                self.send_response(200)
                self.send_header('Content-Length', str(archive.stat().st_size))
                self.end_headers()
                with archive.open('rb') as stream:
                    shutil.copyfileobj(stream, self.wfile)

        server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        serving = threading.Thread(target=server.serve_forever, daemon=True)
        serving.start()
        env = game_environment(game_env('import-startup', home=work / 'home'), work)
        env.update(HORIZON_IMPORT_SCREEN='1',
                   HORIZON_GAME_FILES_URL=f'http://127.0.0.1:{server.server_port}/')
        socket = work / 'control.sock'
        process = None
        started = time.monotonic()
        last = {}
        try:
            with (out / 'game.log').open('w') as log:
                process = subprocess.Popen([
                    str(game), '--resources', str(work / 'initially-missing'),
                    '--offscreen', '--control-socket', str(socket),
                ], cwd=work, env=env, stdout=log, stderr=subprocess.STDOUT)
                while time.monotonic() - started < args.timeout:
                    if process.poll() is not None:
                        raise RuntimeError(f'game exited before title: {process.returncode}')
                    if socket.exists():
                        try:
                            answer = subprocess.run([
                                str(ctl), '--socket', str(socket), 'state',
                            ], cwd=work, env=env, capture_output=True, text=True, timeout=5)
                            lines = answer.stdout.strip().splitlines()
                            if lines:
                                last = json.loads(lines[-1])
                                if last.get('screen') == 'title':
                                    break
                        except (subprocess.TimeoutExpired, json.JSONDecodeError):
                            pass
                    time.sleep(0.5)
                else:
                    raise RuntimeError(f'title not reached: {last}')
                installed = work / 'resources/horizon-resources.json'
                if installed.read_bytes() != marker.read_bytes():
                    raise RuntimeError('installed manifest differs from supplied resources')
                if not result['requests'] or any(r['authorization_present'] for r in result['requests']):
                    raise RuntimeError('missing download request or unexpected Authorization header')
                if (work / 'initially-missing').exists():
                    raise RuntimeError('the initially missing resource base was unexpectedly created')
                result.update({'pass': True, 'screen': last['screen'],
                               'elapsed_seconds': round(time.monotonic() - started, 2),
                               'archive_bytes': archive.stat().st_size,
                               'manifest_sha256': hashlib.sha256(marker.read_bytes()).hexdigest()})
        except Exception as error:
            result['error'] = str(error)
        finally:
            if process is not None and process.poll() is None:
                try:
                    subprocess.run([str(ctl), '--socket', str(socket), 'quit'],
                                   cwd=work, env=env, capture_output=True, timeout=8)
                except subprocess.TimeoutExpired:
                    pass
                try:
                    process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    result['forced_stop'] = True
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=10)
            result['game_exit_code'] = None if process is None else process.returncode
            server.shutdown()
            server.server_close()
            serving.join(timeout=2)
    (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
    return 0 if result['pass'] and result['game_exit_code'] == 0 and not result['forced_stop'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
