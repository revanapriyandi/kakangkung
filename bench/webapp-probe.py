"""Probe a real local manifest and standalone launch in disposable profiles (requires websockets)."""
import asyncio
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
import zlib
from websockets.asyncio.client import connect


def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def png():
    def chunk(kind, value):
        return struct.pack('!I', len(value)) + kind + value + struct.pack('!I', zlib.crc32(kind + value))
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('!2I5B', 192, 192, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress((b'\0' + b'\x7c\x3a\xed' * 192) * 192)) + chunk(b'IEND', b'')


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        if self.path == '/manifest.json':
            body = json.dumps(dict(name='Local Fixture App', start_url='/app/', scope='/app/', id='/app/', display='standalone', icons=[dict(src='/icon.png', sizes='192x192', type='image/png')])).encode()
            kind = 'application/manifest+json'
        elif self.path == '/icon.png':
            body, kind = png(), 'image/png'
        else:
            body = b'<!doctype html><title>Local PWA fixture</title><link rel="manifest" href="/manifest.json"><h1>Local PWA fixture</h1><p>Real manifest discovery; disposable profile.</p>'
            kind = 'text/html'
        self.send_response(200)
        self.send_header('Content-Type', kind)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


async def evaluate(ws_url, expression):
    async with connect(ws_url) as ws:
        await ws.send(json.dumps(dict(id=1, method='Runtime.evaluate', params=dict(expression=expression, returnByValue=True, awaitPromise=True))))
        while True:
            response = json.loads(await asyncio.wait_for(ws.recv(), 10))
            if response.get('id') == 1:
                return response['result']['result'].get('value')


def run(exe, root, name, url, app_id=None):
    profile = root / 'profile'
    debug_port = free_port()
    trace = root / (name + '.jsonl')
    env = dict(os.environ, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=f'--remote-debugging-port={debug_port}', KAKANGKUNG_MEMORY_TRACE=str(trace))
    args = [str(exe), '--user-data-dir=' + str(profile), '--no-first-run', '--app-id=' + app_id if app_id else url]
    startup = subprocess.STARTUPINFO()
    startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    startup.wShowWindow = 0
    with (root / (name + '-stderr.log')).open('wb') as log:
        process = subprocess.Popen(args, env=env, startupinfo=startup, stderr=log)
        try:
            deadline = time.monotonic() + 35
            target = None
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError(f'{name} exited: {process.returncode}')
                try:
                    with urllib.request.urlopen(f'http://127.0.0.1:{debug_port}/json/list', timeout=2) as response:
                        target = next((page for page in json.load(response) if page['type'] == 'page' and page['url'].startswith(url)), None)
                    if target and trace.exists():
                        samples = [json.loads(line) for line in trace.read_text().splitlines()]
                        if any(tab.get('installable') for tab in samples[-1]['tabs']):
                            break
                except (OSError, json.JSONDecodeError):
                    pass
                time.sleep(.5)
            assert target, 'No page at the expected local URL'
            assert any(tab.get('installable') for tab in samples[-1]['tabs']), 'Manifest did not offer installation'
            value = asyncio.run(evaluate(target['webSocketDebuggerUrl'], '({secure:isSecureContext,title:document.title,manifest:document.querySelector("link[rel~=manifest]").href})'))
            assert value['secure'] is True
            return dict(pid=process.pid, page=value, trace=samples[-1])
        finally:
            subprocess.run(['taskkill', '/PID', str(process.pid), '/T', '/F'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


if __name__ == '__main__':
    exe = Path(sys.argv[1]).resolve()
    root = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else Path(tempfile.mkdtemp(prefix='kk-pwa-probe-'))
    root.mkdir(parents=True, exist_ok=True)
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f'http://127.0.0.1:{server.server_port}/app/'
    try:
        browser = run(exe, root, 'browser', url)
        app_id = hashlib.sha256(url.encode()).hexdigest()
        record = root / 'profile' / 'Web Apps' / app_id / 'app.json'
        record.parent.mkdir(parents=True, exist_ok=True)
        record.write_text(json.dumps(dict(name='Local Fixture App', start_url=url, scope=url, manifest_id=url, manifest_url=f'http://127.0.0.1:{server.server_port}/manifest.json', icon_url=f'http://127.0.0.1:{server.server_port}/icon.png')))
        standalone = run(exe, root, 'standalone', url, app_id)
        assert standalone['trace']['app_window'] is True
        assert standalone['trace']['chrome_height'] == 0
        assert len(standalone['trace']['tabs']) == 1
        result = dict(browser=browser, standalone=standalone, evidence=str(root))
        (root / 'result.json').write_text(json.dumps(result, indent=2))
        print(json.dumps(dict(passed=True, secure_context=browser['page']['secure'], manifest_detected=True, standalone_window=True, chrome_height=standalone['trace']['chrome_height'], evidence=str(root))))
    finally:
        server.shutdown()
