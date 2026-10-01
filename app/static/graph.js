(() => {
  'use strict';
  const types = ['belongs_to','based_on','related_to','supersedes','contradicts','fulfills'];
  const label = s => s.replaceAll('_',' ');
  const h = (tag, attrs={}, ...children) => {
    const e=document.createElement(tag);
    for(const [k,v] of Object.entries(attrs)) {
      if(k==='text') e.textContent=v;
      else if(k.startsWith('on')) e.addEventListener(k.slice(2).toLowerCase(),v);
      else e.setAttribute(k,v);
    }
    e.append(...children.flat());return e;
  };
  const svg = (tag,attrs={},...children) => {
    const e=document.createElementNS('http://www.w3.org/2000/svg',tag);
    for(const [k,v] of Object.entries(attrs))e.setAttribute(k,v);
    e.append(...children);return e;
  };
  const button=(text,fn)=>h('button',{type:'button',class:'secondary',onClick:fn,text});
  const field=(name,input)=>h('label',{class:'gm-field'},h('span',{text:name}),input);
  const select=(values,value)=>{const e=h('select');for(const [v,t] of values)e.append(h('option',{value:v,text:t}));e.value=value;return e;};
  const initial=()=>({hidden:new Set(),selected:null,edge:null,focus:null,depth:1,positions:{},zoom:1,x:0,y:0,archived:false,tag:'',relation:'',limit:80,multi:new Set(),multiMode:false,overview:true,focusPositions:{},panelOpen:true});
  window.AlveGraph={initial,mount};
  function mount(root, data, view, actions, query='') {
    const nodes=data.nodes||[],edges=data.relations||[],byId=new Map(nodes.map(n=>[n.id,n]));
    const conflict=new Set((data.conflicts||[]).map(c=>c.nodeId));
    const shell=h('section',{class:'gm'}),bar=h('div',{class:'gm-toolbar'}),layout=h('div',{class:'gm-layout'}),map=h('div',{class:'gm-map'}),panel=h('aside',{class:'gm-panel','aria-label':'Graph selection'});
    root.append(shell);shell.append(bar,layout);layout.append(map,panel);
    let canvas,world,count,visible=[],drag=null;
    const run=async(fn)=>{try{await fn();}catch(e){actions.error(e.message||String(e));}};
    const positions=()=>view.focus?view.focusPositions:view.positions;
    const navigate=id=>{query='';actions.clearSearch();view.tag='';view.relation='';tag.value='';rel.value='';view.hidden.delete(id);view.archived=byId.get(id)?.status==='archived';archived.checked=view.archived;view.focus=id;view.depth=1;view.focusPositions={};view.selected=id;view.edge=null;view.panelOpen=true;layout.classList.remove('gm-panel-closed');draw();fit();};
    const choose=id=>{view.panelOpen=true;layout.classList.remove('gm-panel-closed');if(view.multiMode){view.multi.has(id)?view.multi.delete(id):view.multi.add(id);}else{view.selected=id;view.edge=null;}draw();};
    const menu=h('details',{class:'gm-menu'},h('summary',{text:'Graph menu'}));
    menu.append(button('Add memory',()=>memoryDialog()),button('Add relation',()=>relationDialog()),button('Select several memories',()=>{view.multiMode=true;view.selected=null;view.edge=null;menu.open=false;draw();}),button('Show all hidden',()=>{view.hidden.clear();draw();}),button('Project overview',()=>{view.overview=true;view.focus=null;draw();fit();}),button('Show every memory',()=>{view.overview=false;view.focus=null;draw();fit();}),button('Reset view',()=>{query='';actions.clearSearch();Object.assign(view,initial());tag.value='';rel.value='';archived.checked=false;draw();fit();}));
    const filters=h('details',{class:'gm-menu'},h('summary',{text:'Filters'}));
    const tag=select([['','All tags'],...[...new Set(nodes.flatMap(n=>n.tags||[]))].sort().map(t=>[t,`#${t}`])],view.tag);
    const rel=select([['','All relations'],...types.map(t=>[t,label(t)])],view.relation);
    const archived=h('input',{type:'checkbox'});archived.checked=view.archived;
    tag.onchange=()=>{view.tag=tag.value;draw();};rel.onchange=()=>{view.relation=rel.value;draw();};archived.onchange=()=>{view.archived=archived.checked;draw();};
    filters.append(field('Tag',tag),field('Relation',rel),h('label',{},archived,' Show archived memories'));
    count=h('span',{class:'gm-count','aria-live':'polite'});
    bar.append(menu,filters,button('Details',()=>{view.panelOpen=!view.panelOpen;layout.classList.toggle('gm-panel-closed',!view.panelOpen);}),button('−',()=>zoom(.8)),button('+',()=>zoom(1.25)),button('Fit',()=>fit()),count,button('Clear filters',()=>{query='';actions.clearSearch();view.tag='';view.relation='';view.archived=false;tag.value='';rel.value='';archived.checked=false;draw();fit();}));
    const hint=h('p',{class:'hint',text:'Select a memory or relation. Double-click to explore a neighborhood. Drag memories to arrange; drag the background to pan when the map extends beyond the window. Ctrl/⌘-click selects several memories.'});shell.append(hint);
    const accessible=h('details',{class:'gm-list'},h('summary',{text:'Memories in this view'}));shell.append(accessible);
    layout.classList.toggle('gm-panel-closed',!view.panelOpen);
    function candidates(){
      let allowed=new Set(nodes.filter(n=>(view.archived||n.status!=='archived')&&!view.hidden.has(n.id)&&(!view.tag||(n.tags||[]).includes(view.tag))&&(!query||`${n.title} ${n.body} ${(n.tags||[]).join(' ')}`.toLowerCase().includes(query.toLowerCase()))).map(n=>n.id));
      if(view.focus){
        const seen=new Set([view.focus]);let frontier=new Set(seen);
        for(let i=0;i<view.depth;i++){const next=new Set();for(const r of edges.filter(r=>!view.relation||r.type===view.relation)){if(frontier.has(r.fromId)&&!seen.has(r.toId))next.add(r.toId);if(frontier.has(r.toId)&&!seen.has(r.fromId))next.add(r.fromId);}for(const id of next)seen.add(id);frontier=next;}
        allowed=new Set([...allowed].filter(id=>seen.has(id)));
      }
      if(!view.focus&&view.overview&&!query&&!view.tag&&nodes.some(n=>n.type==='project')){const grouped=new Set(edges.filter(e=>e.type==='belongs_to'&&allowed.has(e.toId)&&byId.get(e.toId)?.type==='project').map(e=>e.fromId));allowed=new Set([...allowed].filter(id=>byId.get(id)?.type==='project'||!grouped.has(id)));}
      return nodes.filter(n=>allowed.has(n.id));
    }
    function transform(){world.setAttribute('transform',`translate(${view.x} ${view.y}) scale(${view.zoom})`);}
    function zoom(f){const z=Math.min(3,Math.max(.15,view.zoom*f)),cx=map.clientWidth/2,cy=map.clientHeight/2;view.x=cx-(cx-view.x)*z/view.zoom;view.y=cy-(cy-view.y)*z/view.zoom;view.zoom=z;transform();}
    function fit(){if(!visible.length)return;const ps=visible.map(n=>positions()[n.id]),left=Math.min(...ps.map(p=>p.x))-120,top=Math.min(...ps.map(p=>p.y))-45,right=Math.max(...ps.map(p=>p.x))+120,bottom=Math.max(...ps.map(p=>p.y))+45;view.zoom=Math.min(1.3,Math.max(.75,Math.min((map.clientWidth-40)/(right-left),(map.clientHeight-40)/(bottom-top))));view.x=map.clientWidth/2-(left+right)/2*view.zoom;view.y=map.clientHeight/2-(top+bottom)/2*view.zoom;if(view.focus&&positions()[view.focus]){view.x=map.clientWidth*.25-positions()[view.focus].x*view.zoom;view.y=map.clientHeight/2-positions()[view.focus].y*view.zoom;}transform();}
    function draw(){
      const focusedNode=document.activeElement?.closest('[data-node]')?.dataset.node;const focusedEdge=document.activeElement?.closest('[data-edge]')?.dataset.edge;
      const all=candidates();visible=all.slice(0,view.limit);
      count.textContent=`${visible.length} of ${all.length} matching · ${nodes.length} total · ${view.hidden.size} hidden${view.focus?' · neighborhood':view.overview?' · project overview':''}${query?' · search active':''}`;
      map.replaceChildren();panel.replaceChildren();accessible.querySelectorAll('button').forEach(e=>e.remove());
      if(view.focus&&!positions()[view.focus]){const peers=visible.filter(n=>n.id!==view.focus);const rows=Math.ceil(peers.length/3);view.focusPositions[view.focus]={x:150,y:90+Math.max(0,rows-1)*55};peers.forEach((n,i)=>{view.focusPositions[n.id]={x:450+(i%3)*270,y:90+Math.floor(i/3)*110};});}
      const columns=Math.max(2,Math.ceil(Math.sqrt(Math.min(all.length,view.focus?9:80))));
      visible.forEach(n=>{if(!positions()[n.id]){let i=0,p;do{p={x:150+(i%columns)*300,y:90+Math.floor(i/columns)*150};i++;}while(Object.values(positions()).some(other=>Math.abs(other.x-p.x)<250&&Math.abs(other.y-p.y)<100));positions()[n.id]=p;}accessible.append(button(n.title,()=>choose(n.id)));});
      canvas=svg('svg',{'aria-label':'Interactive memory map',role:'group',class:'gm-canvas'});const arrow=svg('marker',{id:'gm-arrow',viewBox:'0 0 10 10',refX:9,refY:5,markerWidth:7,markerHeight:7,orient:'auto'},svg('path',{d:'M 0 0 L 10 5 L 0 10 z',fill:'#a2bba9'}));canvas.append(svg('defs',{},arrow));world=svg('g');canvas.append(world);map.append(canvas);transform();
      const shown=new Set(visible.map(n=>n.id));
      for(const r of edges.filter(r=>shown.has(r.fromId)&&shown.has(r.toId)&&(!view.relation||r.type===view.relation))){
        const a=positions()[r.fromId],b=positions()[r.toId],dx=b.x-a.x,dy=b.y-a.y,len=Math.hypot(dx,dy)||1,offset=(len>420?130:28)+types.indexOf(r.type)*12;
        const mx=(a.x+b.x)/2-dy/len*offset,my=(a.y+b.y)/2+dx/len*offset;
        const boundary=(p,q)=>{const x=q.x-p.x,y=q.y-p.y,t=Math.min(116/Math.max(Math.abs(x),.01),38/Math.max(Math.abs(y),.01));return{x:p.x+x*t,y:p.y+y*t};};
        const start=boundary(a,{x:mx,y:my}),end=boundary(b,{x:mx,y:my});
        const path=`M ${start.x} ${start.y} Q ${mx} ${my} ${end.x} ${end.y}`;
        const g=svg('g',{'data-edge':r.id,class:`gm-edge${view.edge===r.id?' selected':''}`,tabindex:0,role:'button','aria-label':`${byId.get(r.fromId).title} ${label(r.type)} ${byId.get(r.toId).title}`});
        g.append(svg('path',{d:path,class:'gm-edge-hit'}),svg('path',{d:path,class:'gm-edge-line','marker-end':'url(#gm-arrow)'}));
        const text=svg('text',{x:(a.x+2*mx+b.x)/4,y:(a.y+2*my+b.y)/4-7,class:'gm-edge-label'});text.textContent=label(r.type);g.append(text);
        const pick=()=>{view.edge=r.id;view.selected=null;draw();};g.onclick=pick;g.onkeydown=e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();pick();}};world.append(g);
      }
      for(const n of visible){const p=positions()[n.id],g=svg('g',{transform:`translate(${p.x} ${p.y})`,class:`gm-node${view.selected===n.id||view.multi.has(n.id)?' selected':''}${n.status==='archived'?' archived':''}`,tabindex:0,role:'button','aria-label':n.title,'data-node':n.id});
        const tooltip=svg('title');tooltip.textContent=n.title;g.append(tooltip);g.append(svg('rect',{x:-112,y:-34,width:224,height:68,rx:10}));const title=svg('text',{x:-98,y:-7});title.textContent=n.title;world.append(g);g.append(title);while(title.getComputedTextLength()>196&&title.textContent.length>2)title.textContent=title.textContent.slice(0,-2)+'…';const subtitle=svg('text',{x:-98,y:16,class:'gm-node-meta'});subtitle.textContent=`${n.type}${n.type==='project'?' · '+edges.filter(e=>e.type==='belongs_to'&&e.toId===n.id).length+' members':''} ${(n.tags||[]).slice(0,2).map(t=>'#'+t).join(' ')}${conflict.has(n.id)?' · conflict':''}`;g.append(title,subtitle);while(subtitle.getComputedTextLength()>196&&subtitle.textContent.length>2)subtitle.textContent=subtitle.textContent.slice(0,-2)+'…';g.ondblclick=()=>navigate(n.id);g.onkeydown=e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();choose(n.id);}else if(e.key.startsWith('Arrow')){e.preventDefault();const neighbors=edges.filter(r=>r.fromId===n.id||r.toId===n.id).map(r=>r.fromId===n.id?r.toId:r.fromId).filter(id=>shown.has(id));const id=neighbors[0];if(id){choose(id);canvas.querySelector(`[data-node="${id}"]`)?.focus({preventScroll:true});}}};world.append(g);}
      if(!visible.length)map.append(h('p',{class:'gm-empty',text:'No memories match this view. Clear filters or show hidden memories.'}));
      if(all.length>view.limit)panel.append(button(`Show more (${all.length-view.limit})`,()=>{view.limit+=80;draw();fit();}));
      if(view.focus)panel.append(h('p',{text:`Focused on ${byId.get(view.focus)?.title||'memory'} · ${view.depth} hop(s)`}),button('Expand neighborhood',()=>{view.depth++;view.focusPositions={};draw();fit();}),button('Collapse neighborhood',()=>{view.depth=Math.max(0,view.depth-1);view.focusPositions={};draw();fit();}),button('Show whole map',()=>{view.focus=null;draw();fit();}));
      if(view.hidden.size){const hidden=h('details',{},h('summary',{text:`Hidden memories (${view.hidden.size})`}));for(const id of view.hidden)if(byId.has(id))hidden.append(button(`Show ${byId.get(id).title}`,()=>{view.hidden.delete(id);draw();}));panel.append(hidden);}
      if(view.multiMode||view.multi.size){panel.append(h('h3',{text:`${view.multi.size} selected`}),h('p',{class:'hint',text:'Select memories on the map or in the list below it.'}),button('Create group',()=>groupDialog()),button('Hide selected',()=>{for(const id of view.multi)view.hidden.add(id);view.multi.clear();draw();}),button('Done selecting',()=>{view.multiMode=false;view.multi.clear();draw();}));}
      if(view.multi.size){const members=h('div',{class:'gm-members'});for(const id of view.multi){const n=byId.get(id);if(n)members.append(button(`Remove ${n.title}${shown.has(id)?'':' (outside view)'}`,()=>{view.multi.delete(id);draw();}));}panel.append(members);}
      const n=byId.get(view.selected),r=edges.find(e=>e.id===view.edge);
      if(n){panel.append(h('h3',{text:n.title}),h('p',{class:'meta',text:`${n.type} · ${n.kind} · ${n.status}`}),h('p',{class:'gm-body',text:n.body||'No description'}),h('p',{text:(n.tags||[]).map(t=>'#'+t).join(' ')||'No tags'}));
        for(const fact of n.facts||[])panel.append(h('p',{class:'gm-fact',text:`${fact.label||fact.key}: ${formatFact(fact.value)}${fact.precision&&fact.precision!=='exact'?' ('+fact.precision+')':''}`}));
        if(n.references?.length){panel.append(h('h4',{text:'Sources'}));for(const ref of n.references){if(/^https?:\/\//i.test(ref.url||''))panel.append(button(ref.title||ref.url,()=>actions.reference(ref.url)));else panel.append(h('p',{text:ref.title||'Source reference'}));}}
        if(conflict.has(n.id))panel.append(h('p',{class:'notice',text:'Conflicting versions: open the full memory to resolve before editing.'}));
        panel.append(button('Open full memory',()=>actions.open(n.id)),button('Focus here',()=>{navigate(n.id);}));
        const nodeActions=h('details',{class:'gm-menu'},h('summary',{text:'Actions'}));panel.append(nodeActions);nodeActions.append(button('Hide from map',()=>{view.hidden.add(n.id);view.selected=null;draw();}));if(!conflict.has(n.id))nodeActions.append(button('Edit memory',()=>memoryDialog(n)),button(n.status==='archived'?'Restore memory':'Archive memory',()=>run(async()=>{if(!await window.AlveConfirm(n.status==='archived'?'Restore this memory?':'Archive this memory? It stays in the vault and can be restored.'))return;await actions.save(`/api/nodes/${n.id}`,'PATCH',{status:n.status==='archived'?'active':'archived',expectedRevision:n.revisionId});await actions.refresh();})),button('Link to another memory',()=>relationDialog(null,n.id)));
        const related=edges.filter(e=>e.fromId===n.id||e.toId===n.id);panel.append(h('h4',{text:`Relations (${related.length})`}));for(const e of related){const other=byId.get(e.fromId===n.id?e.toId:e.fromId);panel.append(button(`${e.fromId===n.id?'→':'←'} ${label(e.type)} · ${other?.title||'Unavailable'}`,()=>{view.edge=e.id;view.selected=null;draw();}));}
      }else if(r){panel.append(h('h3',{text:'Relation'}),button(byId.get(r.fromId)?.title||'Source',()=>{navigate(r.fromId);}),h('p',{text:`→ ${label(r.type)} →`}),button(byId.get(r.toId)?.title||'Target',()=>{navigate(r.toId);}),button('Edit relation',()=>relationDialog(r)),button('Remove relation',()=>run(async()=>{if(!await window.AlveConfirm('Remove this relation? The memories will remain.'))return;await actions.unlink(r);view.edge=null;})));}
      else if(!view.multi.size)panel.append(h('p',{class:'hint',text:'Select a memory or relation to explore and edit it.'}));
      if(focusedNode)canvas.querySelector(`[data-node="${focusedNode}"]`)?.focus({preventScroll:true});else if(focusedEdge)canvas.querySelector(`[data-edge="${focusedEdge}"]`)?.focus({preventScroll:true});
      canvas.onpointerdown=e=>{if(e.button!==0||e.target.closest('.gm-edge'))return;const target=e.target.closest('[data-node]');drag={id:target?.dataset.node,startX:e.clientX,startY:e.clientY,x:view.x,y:view.y,moved:false,ctrl:e.ctrlKey||e.metaKey};if(drag.id)drag.position={...positions()[drag.id]};canvas.setPointerCapture(e.pointerId);};
      canvas.onpointermove=e=>{if(!drag)return;const dx=e.clientX-drag.startX,dy=e.clientY-drag.startY;drag.moved ||=Math.hypot(dx,dy)>5;if(drag.id){const p=positions()[drag.id];p.x=drag.position.x+dx/view.zoom;p.y=drag.position.y+dy/view.zoom;canvas.querySelector(`[data-node="${drag.id}"]`).setAttribute('transform',`translate(${p.x} ${p.y})`);}else{view.x=drag.x+dx;view.y=drag.y+dy;transform();}};
      canvas.onpointerup=()=>{if(!drag)return;const d=drag;drag=null;if(d.id&&!d.moved){view.panelOpen=true;layout.classList.remove('gm-panel-closed');if(d.ctrl||view.multiMode){view.multi.has(d.id)?view.multi.delete(d.id):view.multi.add(d.id);}else{view.selected=d.id;view.edge=null;}}draw();};canvas.onpointercancel=()=>{drag=null;draw();};
      canvas.onwheel=e=>{if(e.ctrlKey||e.metaKey){e.preventDefault();zoom(e.deltaY>0?.9:1.1);}};
    }
    function formatFact(value){if(value==null)return '—';if(typeof value!=='object')return String(value);return [value.value??value.amount,value.currency??value.unit].filter(v=>v!==undefined&&v!=='').join(' ')||JSON.stringify(value);}
    function dialog(title,build,save){
      const d=h('dialog',{class:'gm-dialog'}),form=h('form'),error=h('p',{class:'form-error',role:'alert'});let busy=false;
      let baseline='',asking=false;const signature=()=>JSON.stringify([...form.querySelectorAll('input,textarea,select')].map(e=>[e.value,e.checked]));const dirty=()=>signature()!==baseline;
      const guard=event=>{if(busy||dirty())event.preventDefault();};const unload=event=>{if(busy||dirty()){event.preventDefault();event.returnValue='';}};
      const cleanup=()=>{window.removeEventListener('alve-before-update',guard);window.removeEventListener('beforeunload',unload);window.removeEventListener('alve-session-locked',lock);};
      const close=async()=>{if(busy||asking)return;asking=true;try{if(dirty()&&!await window.AlveConfirm('Discard unsaved graph changes?'))return;d.close();d.remove();cleanup();}finally{asking=false;}};
      d.append(form);form.append(h('h2',{text:title}));const values=build(form);baseline=signature();window.addEventListener('alve-before-update',guard);window.addEventListener('beforeunload',unload);const submit=h('button',{type:'submit',class:'primary',text:'Save'}),cancel=button('Cancel',close);form.append(error,h('div',{class:'toolbar'},submit,cancel));
      form.onsubmit=async e=>{e.preventDefault();if(busy)return;busy=true;submit.disabled=true;cancel.disabled=true;try{await save(values);await actions.refresh();d.close();d.remove();cleanup();}catch(e){error.textContent=e.message||String(e);}finally{busy=false;submit.disabled=false;cancel.disabled=false;}};
      d.oncancel=e=>{e.preventDefault();close();};document.body.append(d);const lock=()=>{d.remove();cleanup();};window.addEventListener('alve-session-locked',lock,{once:true});d.addEventListener('close',cleanup,{once:true});d.showModal();
    }
    function memoryDialog(n){dialog(n?'Edit memory':'Add memory',form=>{const title=h('input',{required:'',maxlength:200,value:n?.title||''}),body=h('textarea',{rows:6,maxlength:6000,text:n?.body||''}),tags=h('input',{value:(n?.tags||[]).join(', ')}),type=select(['memory','project','person','event','document'].map(t=>[t,t]),n?.type||'memory'),kind=select(['decision','preference','insight','commitment','record'].map(t=>[t,t]),n?.kind||'record');form.append(field('Summary heading',title),field('Concise details',body),field('Tags (comma separated)',tags),field('Type',type),field('Kind',kind),h('p',{class:'hint',text:n?'Existing facts and references are preserved. Use the full memory editor to change them.':'Keep this concise and understandable to a person.'}));return{title,body,tags,type,kind};},async v=>{const data={title:v.title.value,body:v.body.value,tags:v.tags.value.split(',').map(t=>t.trim()).filter(Boolean),type:v.type.value,kind:v.kind.value};if(n)data.expectedRevision=n.revisionId;const saved=await actions.save(n?`/api/nodes/${n.id}`:'/api/nodes',n?'PATCH':'POST',data);view.selected=saved.id;if(!n){query='';actions.clearSearch();view.tag='';view.focus=saved.id;view.focusPositions={};view.fitted=false;}else if(!candidates().some(candidate=>candidate.id===saved.id)){query='';actions.clearSearch();view.tag='';} });}
    function relationDialog(r,source){dialog(r?'Edit relation':'Add relation',form=>{const options=nodes.filter(n=>n.status!=='archived'&&!conflict.has(n.id)).map(n=>[n.id,n.title]),from=select(options,r?.fromId||source||view.selected||options[0]?.[0]),to=select(options,r?.toId||options.find(o=>o[0]!==from.value)?.[0]),type=select(types.map(t=>[t,label(t)]),r?.type||'related_to');const picker=(name,input)=>{const search=h('input',{type:'search',placeholder:'Search memories','aria-label':`Search ${name.toLowerCase()} memories`});search.oninput=()=>{const selected=input.value;input.replaceChildren();for(const [id,title] of options)if(id===selected||title.toLowerCase().includes(search.value.toLowerCase()))input.append(h('option',{value:id,text:title}));input.value=selected;};return field(name,h('div',{},search,input));};form.append(picker('From',from),field('Relation',type),picker('To',to),h('p',{class:'hint',text:'Direction matters: From → relation → To.'}));return{from,to,type};},async v=>{if(!v.from.value||v.from.value===v.to.value)throw new Error('Choose two different memories.');const data={fromId:v.from.value,toId:v.to.value,type:v.type.value};if(r)data.expectedRelation=r;const saved=await actions.save(r?`/api/relations/${r.id}/replace`:'/api/relations','POST',data);view.edge=saved.id;view.selected=null;});}
    function groupDialog(){const members=[...view.multi];dialog('Create a group',form=>{const title=h('input',{required:'',maxlength:200});form.append(field('Group heading',title),h('p',{text:'Creates a project memory and belongs to relations for the selected memories.'}));const list=h('ul');for(const id of members)list.append(h('li',{text:byId.get(id)?.title||id}));form.append(list);return title;},async title=>{if(members.length<2)throw new Error('Select at least two memories.');const result=await actions.save('/api/nodes/group','POST',{nodeIds:members,groupTitle:title.value});view.multi.clear();view.multiMode=false;view.selected=result.group.id;view.focus=result.group.id;view.focusPositions={};view.fitted=false;query='';actions.clearSearch();view.tag='';});}
    draw();if(!view.fitted){fit();view.fitted=true;}
  }
})();
