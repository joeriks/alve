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
  const initial=()=>({hidden:new Set(),selected:null,edge:null,focus:null,depth:1,positions:{},zoom:1,x:0,y:0,archived:false,tag:'',relation:'',limit:80,multi:new Set(),multiMode:false});
  window.AlveGraph={initial,mount};
  function mount(root, data, view, actions, query='') {
    const nodes=data.nodes||[],edges=data.relations||[],byId=new Map(nodes.map(n=>[n.id,n]));
    const conflict=new Set((data.conflicts||[]).map(c=>c.nodeId));
    const shell=h('section',{class:'gm'}),bar=h('div',{class:'gm-toolbar'}),layout=h('div',{class:'gm-layout'}),map=h('div',{class:'gm-map'}),panel=h('aside',{class:'gm-panel','aria-label':'Graph selection'});
    root.append(shell);shell.append(bar,layout);layout.append(map,panel);
    let canvas,world,count,visible=[],drag=null;
    const run=async(fn)=>{try{await fn();}catch(e){actions.error(e.message||String(e));}};
    const redraw=()=>draw();
    const choose=id=>{if(view.multiMode){view.multi.has(id)?view.multi.delete(id):view.multi.add(id);}else{view.selected=id;view.edge=null;}draw();};
    const menu=h('details',{class:'gm-menu'},h('summary',{text:'Graph menu'}));
    menu.append(button('Add memory',()=>memoryDialog()),button('Add relation',()=>relationDialog()),button('Select several memories',()=>{view.multiMode=true;view.selected=null;view.edge=null;menu.open=false;draw();}),button('Show all hidden',()=>{view.hidden.clear();draw();}),button('Reset view',()=>{Object.assign(view,initial());tag.value='';rel.value='';archived.checked=false;draw();fit();}));
    const filters=h('details',{class:'gm-menu'},h('summary',{text:'Filters'}));
    const tag=select([['','All tags'],...[...new Set(nodes.flatMap(n=>n.tags||[]))].sort().map(t=>[t,`#${t}`])],view.tag);
    const rel=select([['','All relations'],...types.map(t=>[t,label(t)])],view.relation);
    const archived=h('input',{type:'checkbox'});archived.checked=view.archived;
    tag.onchange=()=>{view.tag=tag.value;draw();};rel.onchange=()=>{view.relation=rel.value;draw();};archived.onchange=()=>{view.archived=archived.checked;draw();};
    filters.append(field('Tag',tag),field('Relation',rel),h('label',{},archived,' Show archived memories'));
    count=h('span',{class:'gm-count','aria-live':'polite'});
    bar.append(menu,filters,button('−',()=>zoom(.8)),button('+',()=>zoom(1.25)),button('Fit',()=>fit()),count);
    const hint=h('p',{class:'hint',text:'Select a memory or relation. Drag memories to arrange; drag the background to pan. Ctrl/⌘-click selects several memories.'});shell.append(hint);
    const accessible=h('details',{class:'gm-list'},h('summary',{text:'Memories in this view'}));shell.append(accessible);
    function candidates(){
      let allowed=new Set(nodes.filter(n=>(view.archived||n.status!=='archived')&&!view.hidden.has(n.id)&&(!view.tag||(n.tags||[]).includes(view.tag))&&(!query||`${n.title} ${n.body} ${(n.tags||[]).join(' ')}`.toLowerCase().includes(query.toLowerCase()))).map(n=>n.id));
      if(view.focus){
        const seen=new Set([view.focus]);let frontier=new Set(seen);
        for(let i=0;i<view.depth;i++){const next=new Set();for(const r of edges.filter(r=>!view.relation||r.type===view.relation)){if(frontier.has(r.fromId)&&!seen.has(r.toId))next.add(r.toId);if(frontier.has(r.toId)&&!seen.has(r.fromId))next.add(r.fromId);}for(const id of next)seen.add(id);frontier=next;}
        allowed=new Set([...allowed].filter(id=>seen.has(id)));
      }
      return nodes.filter(n=>allowed.has(n.id));
    }
    function transform(){world.setAttribute('transform',`translate(${view.x} ${view.y}) scale(${view.zoom})`);}
    function zoom(f){const z=Math.min(3,Math.max(.15,view.zoom*f)),cx=map.clientWidth/2,cy=map.clientHeight/2;view.x=cx-(cx-view.x)*z/view.zoom;view.y=cy-(cy-view.y)*z/view.zoom;view.zoom=z;transform();}
    function fit(){if(!visible.length)return;const ps=visible.map(n=>view.positions[n.id]),left=Math.min(...ps.map(p=>p.x))-120,top=Math.min(...ps.map(p=>p.y))-45,right=Math.max(...ps.map(p=>p.x))+120,bottom=Math.max(...ps.map(p=>p.y))+45;view.zoom=Math.min(1.3,Math.max(.05,Math.min((map.clientWidth-40)/(right-left),(map.clientHeight-40)/(bottom-top))));view.x=map.clientWidth/2-(left+right)/2*view.zoom;view.y=map.clientHeight/2-(top+bottom)/2*view.zoom;transform();}
    function draw(){
      const all=candidates();visible=all.slice(0,view.limit);
      count.textContent=`${visible.length} of ${all.length} matching · ${view.hidden.size} hidden`;
      map.replaceChildren();panel.replaceChildren();accessible.querySelectorAll('button').forEach(e=>e.remove());
      const columns=Math.max(2,Math.ceil(Math.sqrt(Math.min(all.length,80))));
      visible.forEach(n=>{if(!view.positions[n.id]){let i=0,p;do{p={x:150+(i%columns)*300,y:90+Math.floor(i/columns)*150};i++;}while(Object.values(view.positions).some(other=>Math.abs(other.x-p.x)<250&&Math.abs(other.y-p.y)<100));view.positions[n.id]=p;}accessible.append(button(n.title,()=>choose(n.id)));});
      canvas=svg('svg',{'aria-label':'Interactive memory map',role:'group',class:'gm-canvas'});const arrow=svg('marker',{id:'gm-arrow',viewBox:'0 0 10 10',refX:9,refY:5,markerWidth:7,markerHeight:7,orient:'auto'},svg('path',{d:'M 0 0 L 10 5 L 0 10 z',fill:'#a2bba9'}));canvas.append(svg('defs',{},arrow));world=svg('g');canvas.append(world);map.append(canvas);transform();
      const shown=new Set(visible.map(n=>n.id));
      for(const r of edges.filter(r=>shown.has(r.fromId)&&shown.has(r.toId)&&(!view.relation||r.type===view.relation))){
        const a=view.positions[r.fromId],b=view.positions[r.toId],dx=b.x-a.x,dy=b.y-a.y,len=Math.hypot(dx,dy)||1,offset=(len>420?130:28)+types.indexOf(r.type)*12;
        const mx=(a.x+b.x)/2-dy/len*offset,my=(a.y+b.y)/2+dx/len*offset;
        const boundary=(p,q)=>{const x=q.x-p.x,y=q.y-p.y,t=Math.min(116/Math.max(Math.abs(x),.01),38/Math.max(Math.abs(y),.01));return{x:p.x+x*t,y:p.y+y*t};};
        const start=boundary(a,{x:mx,y:my}),end=boundary(b,{x:mx,y:my});
        const path=`M ${start.x} ${start.y} Q ${mx} ${my} ${end.x} ${end.y}`;
        const g=svg('g',{class:`gm-edge${view.edge===r.id?' selected':''}`,tabindex:0,role:'button','aria-label':`${byId.get(r.fromId).title} ${label(r.type)} ${byId.get(r.toId).title}`});
        g.append(svg('path',{d:path,class:'gm-edge-hit'}),svg('path',{d:path,class:'gm-edge-line','marker-end':'url(#gm-arrow)'}));
        const text=svg('text',{x:(a.x+2*mx+b.x)/4,y:(a.y+2*my+b.y)/4-7,class:'gm-edge-label'});text.textContent=label(r.type);g.append(text);
        const pick=()=>{view.edge=r.id;view.selected=null;draw();};g.onclick=pick;g.onkeydown=e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();pick();}};world.append(g);
      }
      for(const n of visible){const p=view.positions[n.id],g=svg('g',{transform:`translate(${p.x} ${p.y})`,class:`gm-node${view.selected===n.id||view.multi.has(n.id)?' selected':''}${n.status==='archived'?' archived':''}`,tabindex:0,role:'button','aria-label':n.title,'data-node':n.id});
        g.append(svg('rect',{x:-112,y:-34,width:224,height:68,rx:10}));const title=svg('text',{x:-98,y:-7});title.textContent=n.title.length>27?n.title.slice(0,26)+'…':n.title;const subtitle=svg('text',{x:-98,y:16,class:'gm-node-meta'});subtitle.textContent=`${n.type} ${(n.tags||[]).slice(0,2).map(t=>'#'+t).join(' ')}${conflict.has(n.id)?' · conflict':''}`;g.append(title,subtitle);g.onkeydown=e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();choose(n.id);}};world.append(g);}
      if(!visible.length)map.append(h('p',{class:'gm-empty',text:'No memories match this view. Clear filters or show hidden memories.'}));
      if(all.length>view.limit)panel.append(button(`Show more (${all.length-view.limit})`,()=>{view.limit+=80;draw();fit();}));
      if(view.focus)panel.append(h('p',{text:`Focused on ${byId.get(view.focus)?.title||'memory'} · ${view.depth} hop(s)`}),button('Expand neighborhood',()=>{view.depth++;draw();fit();}),button('Collapse neighborhood',()=>{view.depth=Math.max(0,view.depth-1);draw();fit();}),button('Show whole map',()=>{view.focus=null;draw();fit();}));
      if(view.hidden.size){const hidden=h('details',{},h('summary',{text:`Hidden memories (${view.hidden.size})`}));for(const id of view.hidden)if(byId.has(id))hidden.append(button(`Show ${byId.get(id).title}`,()=>{view.hidden.delete(id);draw();}));panel.append(hidden);}
      if(view.multiMode||view.multi.size){panel.append(h('h3',{text:`${view.multi.size} selected`}),h('p',{class:'hint',text:'Select memories on the map or in the list below it.'}),button('Create group',()=>groupDialog()),button('Hide selected',()=>{for(const id of view.multi)view.hidden.add(id);view.multi.clear();draw();}),button('Done selecting',()=>{view.multiMode=false;view.multi.clear();draw();}));}
      const n=byId.get(view.selected),r=edges.find(e=>e.id===view.edge);
      if(n){panel.append(h('h3',{text:n.title}),h('p',{class:'meta',text:`${n.type} · ${n.kind} · ${n.status}`}),h('p',{class:'gm-body',text:n.body||'No description'}),h('p',{text:(n.tags||[]).map(t=>'#'+t).join(' ')||'No tags'}));
        if(n.facts?.length)panel.append(h('p',{class:'meta',text:`${n.facts.length} facts · ${n.references?.length||0} references`}));
        if(conflict.has(n.id))panel.append(h('p',{class:'notice',text:'Conflicting versions: open the full memory to resolve before editing.'}));
        panel.append(button('Open full memory',()=>actions.open(n.id)),button('Focus here',()=>{view.focus=n.id;view.depth=1;draw();fit();}),button('Hide from map',()=>{view.hidden.add(n.id);view.selected=null;draw();}));
        if(!conflict.has(n.id))panel.append(button('Edit memory',()=>memoryDialog(n)),button(n.status==='archived'?'Restore memory':'Archive memory',()=>run(async()=>{if(!confirm(n.status==='archived'?'Restore this memory?':'Archive this memory? It stays in the vault and can be restored.'))return;await actions.save(`/api/nodes/${n.id}`,'PATCH',{status:n.status==='archived'?'active':'archived',expectedRevision:n.revisionId});await actions.refresh();})),button('Link to another memory',()=>relationDialog(null,n.id)));
        const related=edges.filter(e=>e.fromId===n.id||e.toId===n.id);panel.append(h('h4',{text:`Relations (${related.length})`}));for(const e of related){const other=byId.get(e.fromId===n.id?e.toId:e.fromId);panel.append(button(`${e.fromId===n.id?'→':'←'} ${label(e.type)} · ${other?.title||'Unavailable'}`,()=>{view.edge=e.id;view.selected=null;draw();}));}
      }else if(r){panel.append(h('h3',{text:'Relation'}),button(byId.get(r.fromId)?.title||'Source',()=>{view.hidden.delete(r.fromId);choose(r.fromId);}),h('p',{text:`→ ${label(r.type)} →`}),button(byId.get(r.toId)?.title||'Target',()=>{view.hidden.delete(r.toId);choose(r.toId);}),button('Edit relation',()=>relationDialog(r)),button('Remove relation',()=>run(async()=>{if(!confirm('Remove this relation? The memories will remain.'))return;await actions.unlink(r);view.edge=null;})));}
      else if(!view.multi.size)panel.append(h('p',{class:'hint',text:'Select a memory or relation to explore and edit it.'}));
      canvas.onpointerdown=e=>{if(e.button!==0||e.target.closest('.gm-edge'))return;const target=e.target.closest('[data-node]');drag={id:target?.dataset.node,startX:e.clientX,startY:e.clientY,x:view.x,y:view.y,moved:false,ctrl:e.ctrlKey||e.metaKey};if(drag.id)drag.position={...view.positions[drag.id]};canvas.setPointerCapture(e.pointerId);};
      canvas.onpointermove=e=>{if(!drag)return;const dx=e.clientX-drag.startX,dy=e.clientY-drag.startY;drag.moved ||=Math.hypot(dx,dy)>5;if(drag.id){const p=view.positions[drag.id];p.x=drag.position.x+dx/view.zoom;p.y=drag.position.y+dy/view.zoom;canvas.querySelector(`[data-node="${drag.id}"]`).setAttribute('transform',`translate(${p.x} ${p.y})`);}else{view.x=drag.x+dx;view.y=drag.y+dy;transform();}};
      canvas.onpointerup=()=>{if(!drag)return;const d=drag;drag=null;if(d.id&&!d.moved){if(d.ctrl||view.multiMode){view.multi.has(d.id)?view.multi.delete(d.id):view.multi.add(d.id);}else{view.selected=d.id;view.edge=null;}}draw();};canvas.onpointercancel=()=>{drag=null;draw();};
      canvas.onwheel=e=>{if(e.ctrlKey||e.metaKey){e.preventDefault();zoom(e.deltaY>0?.9:1.1);}};
    }
    function dialog(title,build,save){
      const d=h('dialog',{class:'gm-dialog'}),form=h('form'),error=h('p',{class:'form-error',role:'alert'});let busy=false;
      const close=()=>{if(!busy){d.close();d.remove();}};
      d.append(form);form.append(h('h2',{text:title}));const values=build(form);const submit=h('button',{type:'submit',class:'primary',text:'Save'}),cancel=button('Cancel',close);form.append(error,h('div',{class:'toolbar'},submit,cancel));
      form.onsubmit=async e=>{e.preventDefault();if(busy)return;busy=true;submit.disabled=true;cancel.disabled=true;try{await save(values);await actions.refresh();d.close();d.remove();}catch(e){error.textContent=e.message||String(e);}finally{busy=false;submit.disabled=false;cancel.disabled=false;}};
      d.oncancel=e=>{e.preventDefault();close();};document.body.append(d);const lock=()=>{d.remove();};window.addEventListener('alve-session-locked',lock,{once:true});d.addEventListener('close',()=>window.removeEventListener('alve-session-locked',lock),{once:true});d.showModal();
    }
    function memoryDialog(n){dialog(n?'Edit memory':'Add memory',form=>{const title=h('input',{required:'',maxlength:160,value:n?.title||''}),body=h('textarea',{rows:6,maxlength:4000,text:n?.body||''}),tags=h('input',{value:(n?.tags||[]).join(', ')}),type=select(['memory','project','person','event','document'].map(t=>[t,t]),n?.type||'memory'),kind=select(['decision','preference','insight','commitment','record'].map(t=>[t,t]),n?.kind||'record');form.append(field('Summary heading',title),field('Concise details',body),field('Tags (comma separated)',tags),field('Type',type),field('Kind',kind),h('p',{class:'hint',text:n?'Existing facts and references are preserved. Use the full memory editor to change them.':'Keep this concise and understandable to a person.'}));return{title,body,tags,type,kind};},async v=>{const data={title:v.title.value,body:v.body.value,tags:v.tags.value.split(',').map(t=>t.trim()).filter(Boolean),type:v.type.value,kind:v.kind.value};if(n)data.expectedRevision=n.revisionId;const saved=await actions.save(n?`/api/nodes/${n.id}`:'/api/nodes',n?'PATCH':'POST',data);view.selected=saved.id;});}
    function relationDialog(r,source){dialog(r?'Edit relation':'Add relation',form=>{const options=nodes.filter(n=>n.status!=='archived'&&!conflict.has(n.id)).map(n=>[n.id,n.title]),from=select(options,r?.fromId||source||view.selected||options[0]?.[0]),to=select(options,r?.toId||options.find(o=>o[0]!==from.value)?.[0]),type=select(types.map(t=>[t,label(t)]),r?.type||'related_to');form.append(field('From',from),field('Relation',type),field('To',to),h('p',{class:'hint',text:'Direction matters: From → relation → To.'}));return{from,to,type};},async v=>{if(!v.from.value||v.from.value===v.to.value)throw new Error('Choose two different memories.');const data={fromId:v.from.value,toId:v.to.value,type:v.type.value};if(r)data.expectedRelation=r;const saved=await actions.save(r?`/api/relations/${r.id}/replace`:'/api/relations','POST',data);view.edge=saved.id;view.selected=null;});}
    function groupDialog(){dialog('Create a group',form=>{const title=h('input',{required:'',maxlength:160});form.append(field('Group heading',title),h('p',{text:'Creates a project memory and belongs to relations for the selected memories.'}));return title;},async title=>{if(view.multi.size<2)throw new Error('Select at least two memories.');await actions.save('/api/nodes/group','POST',{nodeIds:[...view.multi],groupTitle:title.value});view.multi.clear();});}
    draw();if(!view.fitted){fit();view.fitted=true;}
  }
})();
