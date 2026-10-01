// Optional UI regression suite: Playwright plus an installed Edge browser.
const {test}=require('node:test');
const assert=require('node:assert/strict');
const {chromium}=require('playwright');
const path=require('node:path');

test('project graph: readable navigation, safe confirmation and protected drafts',async()=>{
  const browser=await chromium.launch({channel:'msedge',headless:true});
  try {
    const page=await browser.newPage({viewport:{width:1100,height:760}});
    const errors=[];page.on('pageerror',e=>errors.push(e.message));
    await page.setContent('<main id="graph"></main>');
    for(const file of ['style.css','graph.css'])await page.addStyleTag({path:path.resolve('app/static',file)});
    for(const file of ['confirm.js','graph.js'])await page.addScriptTag({path:path.resolve('app/static',file)});
    await page.evaluate(()=>{
      const roots=Array.from({length:5},(_,i)=>({id:'p'+i,title:'Project '+i,type:'project',kind:'record',status:'active',body:'A concise project summary.',tags:[],facts:[],references:[],revisionId:'r'+i}));
      const notes=Array.from({length:95},(_,i)=>({id:'n'+i,title:i===0?'W'.repeat(60):'Task '+i,type:'memory',kind:'commitment',status:'active',body:'Concise task detail.',tags:[i%2?'home':'work'],facts:i===1?[{key:'cost',value:{type:'money',amount:'0',currency:'SEK'}}]:[],references:i===0?[{title:'Source reference',url:'https://example.com'}]:[],revisionId:'n'+i}));
      window.fixture={nodes:[...roots,...notes],relations:notes.map((n,i)=>({id:'e'+i,fromId:n.id,toId:'p'+Math.floor(i/19),type:'belongs_to'})),conflicts:[]};
      window.graphView=AlveGraph.initial();window.mutations=[];window.cleared=0;window.auditErrors=[];
      window.renderGraph=(query='')=>{document.querySelector('#graph').replaceChildren();AlveGraph.mount(document.querySelector('#graph'),fixture,graphView,{clearSearch:()=>cleared++,reference:()=>{},error:e=>auditErrors.push(e),open:()=>{},refresh:async()=>renderGraph(),unlink:async r=>{mutations.push(r.id);fixture.relations=fixture.relations.filter(e=>e.id!==r.id);renderGraph();},save:async()=>{throw Error('Synthetic save failure');}},query);};
      renderGraph();
    });
    assert.equal(await page.locator('.gm-node').count(),5);if(process.env.ALVE_GRAPH_SCREENSHOT)await page.screenshot({path:process.env.ALVE_GRAPH_SCREENSHOT});
    const select=async id=>{await page.locator(`[data-node="${id}"]`).focus();await page.keyboard.press('Enter');};
    await select('p0');assert.equal(await page.evaluate(()=>document.activeElement.dataset.node),'p0');
    await page.getByRole('button',{name:'Focus here',exact:true}).click();assert.equal(await page.locator('.gm-node').count(),20);
    assert.ok(await page.locator('.gm-node text').first().evaluate(e=>e.getBoundingClientRect().height>=12));
    assert.ok(await page.locator('[data-node="n0"] text').first().evaluate(e=>e.getBBox().width<=196));
    assert.equal(await page.locator('[data-node="n0"] title').textContent(),'W'.repeat(60));
    await page.evaluate(()=>renderGraph('Task 1'));await select('n1');assert.match(await page.locator('.gm-panel').innerText(),/0 SEK/);
    await page.getByRole('button',{name:/belongs to · Project 0/}).click();await page.locator('.gm-panel').getByRole('button',{name:'Project 0',exact:true}).click();
    assert.equal(await page.locator('[data-node="p0"]').count(),1);assert.equal(await page.evaluate(()=>cleared),2);
    await select('n0');assert.match(await page.locator('.gm-panel').innerText(),/Source reference/);
    await page.locator('.gm-panel .gm-menu').evaluate(e=>e.open=true);await page.getByRole('button',{name:'Edit memory',exact:true}).click();
    await page.locator('.gm-dialog textarea').fill('Keep this unsaved draft.');
    const guard=()=>page.evaluate(()=>{const event=new Event('alve-before-update',{cancelable:true});window.dispatchEvent(event);return event.defaultPrevented;});
    assert.equal(await guard(),true);await page.keyboard.press('Escape');await page.locator('.alve-confirm').getByRole('button',{name:'Cancel',exact:true}).click();assert.equal(await page.locator('.gm-dialog textarea').inputValue(),'Keep this unsaved draft.');
    await page.getByRole('button',{name:'Save',exact:true}).click();assert.match(await page.locator('.gm-dialog .form-error').innerText(),/Synthetic save failure/);assert.equal(await page.locator('.gm-dialog textarea').inputValue(),'Keep this unsaved draft.');
    await page.locator('.gm-dialog').getByRole('button',{name:'Cancel',exact:true}).click();await page.locator('.alve-confirm').getByRole('button',{name:'Confirm',exact:true}).click();assert.equal(await guard(),false);
    await page.getByRole('button',{name:/belongs to · Project 0/}).click();await page.getByRole('button',{name:'Remove relation',exact:true}).click();await page.locator('.alve-confirm').getByRole('button',{name:'Cancel',exact:true}).click();assert.deepEqual(await page.evaluate(()=>mutations),[]);
    await page.getByRole('button',{name:'Remove relation',exact:true}).click();await page.locator('.alve-confirm').getByRole('button',{name:'Confirm',exact:true}).click();assert.deepEqual(await page.evaluate(()=>mutations),['e0']);
    await page.evaluate(()=>{graphView.focus=null;graphView.overview=false;graphView.multiMode=true;graphView.multi=new Set(['n1','n2']);renderGraph('Task 1');});
    assert.match(await page.locator('.gm-panel').innerText(),/Remove Task 2 \(outside view\)/);
    await page.getByRole('button',{name:'Create group',exact:true}).click();assert.deepEqual(await page.locator('.gm-dialog li').allTextContents(),['Task 1','Task 2']);await page.locator('.gm-dialog').getByRole('button',{name:'Cancel',exact:true}).click();
    await page.getByRole('button',{name:'Done selecting',exact:true}).click();await page.evaluate(()=>{graphView.focus='p0';renderGraph();});await select('p0');
    await page.setViewportSize({width:390,height:844});assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);assert.ok(await page.locator('.gm-panel').evaluate(e=>e.getBoundingClientRect().bottom<=innerHeight));
    await page.locator('.gm-panel .gm-menu').evaluate(e=>e.open=true);await page.getByRole('button',{name:'Edit memory',exact:true}).click();await page.locator('.gm-dialog textarea').fill('Lock protection test');await page.locator('.gm-dialog').getByRole('button',{name:'Cancel',exact:true}).click();await page.locator('.alve-confirm').waitFor();await page.evaluate(()=>window.dispatchEvent(new Event('alve-session-locked')));assert.equal(await page.locator('.gm-dialog').count(),0);assert.equal(await guard(),false);
    assert.deepEqual(errors,[]);assert.deepEqual(await page.evaluate(()=>auditErrors),[]);
  } finally {await browser.close();}
});
