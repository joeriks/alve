(() => {
  // Local HTML confirmation also works with native async confirm shims.
  let pending;
  window.AlveConfirm = message => {
    if (pending) return Promise.resolve(false);
    return new Promise(resolve => {
      const dialog = document.createElement('dialog'); dialog.className = 'gm-dialog alve-confirm';
      const text = document.createElement('p'); text.textContent = message;
      const controls = document.createElement('div'); controls.className = 'toolbar';
      const cancel = document.createElement('button'); cancel.type = 'button'; cancel.textContent = 'Cancel';
      const confirm = document.createElement('button'); confirm.type = 'button'; confirm.textContent = 'Confirm'; confirm.className = 'primary';
      const finish = accepted => { pending = null; window.removeEventListener('alve-session-locked',locked); dialog.close(); dialog.remove(); resolve(accepted); };
      const locked = () => finish(false); pending = dialog;
      cancel.onclick = () => finish(false); confirm.onclick = () => finish(true);
      dialog.oncancel = event => { event.preventDefault(); finish(false); };
      window.addEventListener('alve-session-locked',locked,{once:true});
      controls.append(cancel,confirm); dialog.append(text,controls); document.body.append(dialog);
      try { dialog.showModal(); cancel.focus(); } catch { finish(false); }
    });
  };
})();
