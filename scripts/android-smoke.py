"""Exercise the real Android WebView/native bridge with synthetic data only."""
import glob
import base64
import json
import re
import subprocess
import time
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path
import websocket

APP = 'com.alve.local'
PASSWORD = 'Synthetic Android acceptance vault'
OUT = Path('android-test-results')
OUT.mkdir(exist_ok=True)

def adb(*args):
    return subprocess.check_output(['adb', *args], text=True)

apks = glob.glob('src-tauri/gen/android/app/build/outputs/apk/**/debug/*.apk', recursive=True)
assert apks, 'Emulator APK missing'
adb('install', '-r', apks[0])
adb('shell', 'am', 'start', '-n', f'{APP}/.MainActivity')
def connect():
    deadline = time.monotonic() + 90
    while True:
        sockets = adb('shell', 'cat', '/proc/net/unix')
        matches = re.findall(r'@(webview_devtools_remote[^\s]*)', sockets)
        if matches:
            adb('forward', 'tcp:9223', f'localabstract:{matches[-1]}')
            try:
                pages = json.load(urllib.request.urlopen('http://127.0.0.1:9223/json', timeout=2))
                page = next(p for p in pages if p.get('type') == 'page')
                return websocket.create_connection(page['webSocketDebuggerUrl'], timeout=45, suppress_origin=True)
            except Exception:
                pass
        assert time.monotonic() < deadline, 'Android WebView debugging did not become ready'
        time.sleep(1)

ws = connect()

sequence = 0
def evaluate(expression):
    global sequence
    sequence += 1
    ws.send(json.dumps({'id': sequence, 'method': 'Runtime.evaluate', 'params': {
        'expression': expression, 'awaitPromise': True, 'returnByValue': True}}))
    while True:
        reply = json.loads(ws.recv())
        if reply.get('id') == sequence:
            assert 'error' not in reply, reply
            result = reply['result']
            assert 'exceptionDetails' not in result, result
            return result['result'].get('value')

def wait(expression, seconds=30):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        if evaluate(expression):
            return
        time.sleep(.4)
    raise AssertionError(f'Condition did not become true: {expression}')

def unlock():
    evaluate(f"document.querySelector('#password').value={json.dumps(PASSWORD)};document.querySelector('#unlock-form').requestSubmit();true")
    wait("document.querySelector('#gate').classList.contains('hidden')")

def save_document():
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        adb('shell', 'uiautomator', 'dump', '/sdcard/alve-picker.xml')
        tree = ET.fromstring(adb('shell', 'cat', '/sdcard/alve-picker.xml'))
        for node in tree.iter('node'):
            if node.get('text', '').lower() == 'save' and node.get('enabled') == 'true':
                bounds = list(map(int, re.findall(r'\d+', node.get('bounds', ''))))
                if len(bounds) == 4:
                    adb('shell', 'input', 'tap', str((bounds[0]+bounds[2])//2), str((bounds[1]+bounds[3])//2))
                    return
        time.sleep(.3)
    raise AssertionError('Android document picker Save button missing')

try:
    wait("Boolean(window.__TAURI__?.core?.invoke)")
    info = evaluate("window.__TAURI__.core.invoke('platform_info')")
    assert info['mobile'] and info['platform'] == 'android', info
    unlock()
    evaluate("document.querySelector('#nav [data-view=editor]').click();true")
    wait("Boolean(document.querySelector('#content [name=title]'))")
    evaluate("document.querySelector('#content [name=title]').value='Synthetic Android memory';document.querySelector('#content [name=body]').value='Remember the portable vault.';document.querySelector('#content form').requestSubmit();true")
    wait("Boolean(document.querySelector('.detail-title')?.textContent.includes('Synthetic Android memory'))")
    adb('shell', 'input', 'keyevent', '4')
    wait("document.querySelector('#view-title').textContent==='Memories'")
    adb('shell', 'input', 'keyevent', '3')
    wait("!document.querySelector('#gate').classList.contains('hidden')")
    status = evaluate("window.__TAURI__.core.invoke('alve_request',{method:'GET',path:'/api/status',body:{}})")
    assert not status['unlocked'], 'Vault remained unlocked after backgrounding'
    adb('shell', 'am', 'start', '-n', f'{APP}/.MainActivity')
    unlock()
    wait("document.querySelector('#content').textContent.includes('Synthetic Android memory')")
    assert not evaluate("Array.from(document.querySelectorAll('[data-native-update]')).some(x=>!x.hidden&&!x.classList.contains('hidden'))"), 'Desktop updater shown on Android'
    package = adb('shell', 'dumpsys', 'package', APP)
    assert 'ALLOW_BACKUP' not in package, 'Android automatic backup enabled'
    # Use the actual export button and SAF picker, then compare saved bytes with
    # the encrypted payload passed through the native bridge.
    evaluate("window.exportOutcome=null;window.originalInvoke=window.__TAURI__.core.invoke;window.__TAURI__.core.invoke=(command,args)=>{const result=window.originalInvoke(command,args);if(command==='save_export'){window.exportArgs=args;result.then(value=>window.exportOutcome={value},error=>window.exportOutcome={error:String(error)});}return result;};document.querySelector('#nav [data-view=backup]').click();true")
    evaluate("Array.from(document.querySelectorAll('#content button')).find(x=>x.textContent==='Export encrypted .alve bundle').click();true")
    save_document()
    wait("window.exportOutcome!==null")
    assert evaluate('window.exportOutcome') == {'value': True}, evaluate('window.exportOutcome')
    args = evaluate('window.exportArgs')
    adb('shell', 'mkdir', '-p', '/sdcard/Documents', '/sdcard/Download')
    paths = adb('shell', 'find', '/sdcard/Download', '/sdcard/Documents', '-name', args['suggestedName']).splitlines()
    assert paths, 'Saved Android backup document missing'
    actual = subprocess.check_output(['adb', 'exec-out', 'cat', paths[0]])
    assert actual == base64.b64decode(args['content']), 'Android exported backup bytes changed'
    evaluate("window.exportOutcome=null;Array.from(document.querySelectorAll('#content button')).find(x=>x.textContent==='Export encrypted .alve bundle').click();true")
    time.sleep(1)
    adb('shell', 'input', 'keyevent', '4')
    wait("window.exportOutcome!==null")
    assert evaluate('window.exportOutcome') == {'value': False}, 'Picker cancellation reported success'
    assert evaluate("document.querySelector('#gate').classList.contains('hidden')"), 'Quick picker cancellation locked vault'
    adb('shell', 'am', 'force-stop', APP)
    ws.close()
    adb('shell', 'am', 'start', '-n', f'{APP}/.MainActivity')
    ws = connect()
    wait("!document.querySelector('#gate').classList.contains('hidden')")
    with open(OUT / 'locked-after-restart.png', 'wb') as image:
        subprocess.run(['adb','exec-out','screencap','-p'],stdout=image,check=True)
    unlock()
    wait("document.querySelector('#content').textContent.includes('Synthetic Android memory')")
    (OUT / 'result.txt').write_text('PASS: real Android creation, native save, Back, background lock, reopen, SAF encrypted export and cancel, mobile updater exclusion and backup policy.\n')
    print('Android acceptance passed.')
finally:
    (OUT / 'logcat.txt').write_text(adb('logcat','-d','-s','alve','chromium','AndroidRuntime'))
    ws.close()
