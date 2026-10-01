/* Explicit, foreground exchange of encrypted bundles. No browser network access. */
(() => {
  'use strict';
  window.mountAlveSync = ({container, invoke, getToken, refresh,onReceived,lastResult}) => {
    const node = (tag, text, attrs={}) => {const n=document.createElement(tag);if(text)n.textContent=text;for(const [k,v] of Object.entries(attrs))n.setAttribute(k,v);return n;};
    const panel=node('section','',{class:'card sync-panel'});
    panel.append(node('h3','Sync now over local Wi-Fi'),node('p','Keep both devices open and unlocked on the same Wi-Fi or phone hotspot. This exchange uses a five-minute, single-use transfer link.'));
    const notice=node('p','Use a network you trust. Vault contents are encrypted, but this first transfer mode does not verify the other device’s identity or encrypt the transfer link in transit.',{class:'notice'});
    panel.append(notice);
    const status=node('p','Checking transfer status…',{role:'status','aria-live':'polite'}),error=node('p','',{class:'form-error',role:'alert'});
    const offer=node('details'),offerSummary=node('summary','1. Offer this device’s changes');offer.append(offerSummary);
    const address=node('input','',{type:'text',placeholder:'192.168.1.42','aria-label':'This device’s Wi-Fi IPv4 address',inputmode:'decimal',autocomplete:'off'});
    const link=node('textarea','',{readonly:'',rows:'3','aria-label':'Single-use transfer link',spellcheck:'false'});
    const start=node('button','Create transfer link',{type:'button',class:'secondary'}),stop=node('button','Stop offering',{type:'button',class:'secondary'});
    const copy=node('button','Copy link',{type:'button',class:'secondary'});copy.disabled=true;
    offer.append(node('p','Enter this device’s local IPv4 address from its Wi-Fi settings. Share the link with your other device using a method you control. Paste it into Alve; do not open it in a browser.'),address,start,link,copy,stop);
    const receive=node('details');receive.append(node('summary','2. Receive and merge changes'));
    const incoming=node('textarea','',{rows:'3',placeholder:'Paste the other device’s transfer link','aria-label':'Incoming transfer link',spellcheck:'false',autocomplete:'off'});
    const password=node('input','',{type:'password',placeholder:'Password of the offering vault','aria-label':'Offering vault password',autocomplete:'off'});
    const pull=node('button','Receive & merge',{type:'button',class:'primary'});
    receive.append(node('p','Both devices must already contain this same vault. For a fresh phone, first restore an encrypted backup. Import retains revision history and conflicts; it does not overwrite the vault.'),incoming,password,pull);
    if(lastResult?.())panel.append(node('p',lastResult(),{class:'notice',role:'status'}));
    panel.append(status,error,offer,receive,node('p','To exchange edits in both directions, repeat with the devices reversed after the first import. The offer is a snapshot taken when you create its link. A receipt only means the other device reported a successful import.',{class:'meta'}));
    container.append(panel);
    let busy=false, timer;
    const cleanup=()=>{clearTimeout(timer);password.value='';link.value='';incoming.value='';window.removeEventListener('alve-session-locked',cleanup);};
    const current=()=>panel.isConnected&&Boolean(getToken());
    const setBusy=value=>{busy=value;start.disabled=value;pull.disabled=value;stop.disabled=value;};
    const request=(command,args={})=>invoke(command,{...args,token:getToken()});
    const show=out=>{
      link.value=out.link||'';copy.disabled=!out.link;
      status.textContent=out.peerReportedMerged?'The receiving device reported a successful merge. Offer your return changes to finish both directions.':out.phase==='offering'?`Offering this snapshot (${out.expiresInSeconds||0}s remaining).`:out.phase==='downloaded'?'Bundle downloaded; waiting for the peer’s import receipt.':out.phase==='expired'?'Transfer expired. Create a new link to retry.':'No transfer is being offered.';
    };
    const run=async task=>{if(busy)return;error.textContent='';setBusy(true);try{await task();}catch(e){if(current())error.textContent=typeof e==='string'?e:e.message||'Transfer failed.';}finally{password.value='';if(current())setBusy(false);}};
    start.addEventListener('click',()=>run(async()=>{const out=await request('sync_start',{address:address.value.trim()});if(current())show(out);}));
    stop.addEventListener('click',()=>run(async()=>{await request('sync_stop');if(current())show({phase:'idle'});}));
    copy.addEventListener('click',async()=>{link.focus();link.select();try{await navigator.clipboard.writeText(link.value);status.textContent='Link copied. Keep it private and paste it into Alve on the other device.';}catch{status.textContent='Link selected. Copy it using your device’s text menu.';}});
    pull.addEventListener('click',()=>run(async()=>{
      if(!incoming.value.trim()||!password.value){throw new Error('Enter the transfer link and the offering vault password.');}
      const token=getToken(),out=await request('sync_pull',{link:incoming.value.trim(),password:password.value});
      if(!current()||getToken()!==token)return;
      incoming.value='';status.textContent=out.receiptSent?'Changes saved locally; receipt sent. Reverse the devices to send your changes back.':'Changes saved locally, but the receipt could not be sent. The other device may show an unconfirmed transfer.';
      onReceived?.(out);
      await refresh();
    }));
    const poll=async()=>{if(!current()){cleanup();return;}try{if(!busy)show(await request('sync_status'));}catch(e){if(current())error.textContent=typeof e==='string'?e:'Transfer status unavailable.';}finally{if(current())timer=setTimeout(poll,1500);else cleanup();}};
    poll();
    window.addEventListener('alve-session-locked',cleanup);
  };
})();
