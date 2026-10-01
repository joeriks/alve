(async () => {
  'use strict';
  const native = () => window.__TAURI__?.core?.invoke;
  if (!native()) return;
  try { if ((await native()('platform_info')).mobile) return; } catch { /* Older desktop shells. */ }

  const dialog = document.createElement('dialog');
  dialog.setAttribute('aria-labelledby', 'update-title');
  const panel = document.createElement('div');
  const title = document.createElement('h2'); title.id = 'update-title';
  const message = document.createElement('p');
  const details = document.createElement('p');
  const progress = document.createElement('progress'); progress.hidden = true;
  const actions = document.createElement('div');
  const check = document.createElement('button'); check.type = 'button'; check.textContent = 'Check for updates';
  const install = document.createElement('button'); install.type = 'button'; install.textContent = 'Install update and restart'; install.hidden = true;
  const close = document.createElement('button'); close.type = 'button'; close.textContent = 'Close';
  actions.append(check, install, close); panel.append(title, message, details, progress, actions); dialog.append(panel); document.body.append(dialog);
  let checkedVersion = null;
  let busy = false;

  const setBusy = value => { busy = value; check.disabled = value; install.disabled = value; close.disabled = value; progress.hidden = !value; };
  const show = () => { if (!dialog.open) dialog.showModal(); };
  const state = (heading, copy, extra = '') => { title.textContent = heading; message.textContent = copy; details.textContent = extra; };
  const updateButtons = () => document.querySelectorAll('[data-native-update]').forEach(button => { button.hidden = false; button.classList.remove('hidden'); button.addEventListener('click', openCheck); });
  const openCheck = async () => {
    if (busy) { show(); return; }
    show(); checkedVersion = null; install.hidden = true; setBusy(true); state('Checking for updates', 'Contacting the desktop update service.');
    try {
      const result = await native()('check_update');
      if (result.available && result.version) {
        checkedVersion = result.version; install.hidden = false;
        state(`Update ${result.version} is available`, 'Review the update notes, then choose whether to install and restart.', result.notes || 'No release notes were provided.');
      } else state('Alve is up to date', `You are running ${result.currentVersion}.`);
    } catch (error) { state('Could not check for updates', typeof error === 'string' ? error : 'The desktop update service did not respond.'); }
    finally { setBusy(false); }
  };
  check.addEventListener('click', openCheck);
  close.addEventListener('click', () => { if (!busy) dialog.close(); });
  dialog.addEventListener('cancel', event => { if (busy) event.preventDefault(); });
  install.addEventListener('click', async () => {
    if (!checkedVersion) return;
    const before = new CustomEvent('alve-before-update', { cancelable: true, detail: { version: checkedVersion } });
    if (!window.dispatchEvent(before)) { state('Update paused', 'Finish or discard the open edit before installing the update.'); return; }
    setBusy(true); state('Preparing update', 'The update will download, install, and restart Alve.');
    try { await native()('install_update', { version: checkedVersion }); }
    catch (error) { checkedVersion = null; install.hidden = true; setBusy(false); state('Could not install update', typeof error === 'string' ? error : 'The desktop update service did not respond.'); }
  });
  window.__TAURI__.event.listen('alve-check-for-updates', openCheck);
  window.__TAURI__.event.listen('alve-update-state', event => {
    const payload = event.payload || {};
    if (payload.phase === 'locked') { window.dispatchEvent(new Event('alve-update-lock')); return; }
    if (payload.phase === 'downloading') {
      setBusy(true); state('Downloading update', payload.total ? `${payload.downloaded || 0} of ${payload.total} bytes downloaded.` : 'Downloading update.');
      if (payload.total) { progress.max = payload.total; progress.value = payload.downloaded || 0; }
    } else if (payload.phase === 'installing') { setBusy(true); state('Installing update', 'Alve will restart when installation completes.'); }
  });
  updateButtons();
})();
