const {test}=require('node:test');
const assert=require('node:assert/strict');
const model=require('../app/static/agents.js');
const draft={name:'Project manager',mission:'Keep two projects moving.',method:'Check confirmed commitments.',escalation:'Ask when blocked; otherwise stay quiet.',firstAssignment:'Suggest three next steps.',phase:'introduced',understanding:'',contextIds:['a','b'],reviewDate:'2026-10-05'};
test('introduction and confirmed understanding round-trip; rebrief and extra facts survive',()=>{
  const node={...model.content(draft),id:'manager',revisionId:'r1'};
  assert.equal(model.isAgent(node),true);
  assert.deepEqual(model.read(node),draft);
  assert.equal(node.facts.find(f=>f.key==='agent_reviewDate').value.type,'date');
  node.facts.push({key:'budget',label:'Budget',value:{type:'money',amount:'1200.00',currency:'SEK'},precision:'exact'});
  node.tags.push('work');node.references.push({title:'Method',url:'https://example.com'});
  const ready=model.content({...draft,phase:'ready',understanding:'I follow both projects and ask before changes.'},node);
  assert.equal(model.read(ready).phase,'ready');
  assert.deepEqual(ready.facts.find(f=>f.key==='budget'),node.facts.at(-1));
  assert.deepEqual(ready.references,node.references);assert.ok(ready.tags.includes('work'));
  const rebrief=model.content({...model.read(ready),phase:'introduced',understanding:''},ready);
  assert.equal(model.read(rebrief).understanding,'');
});
test('briefing includes exactly selected context with exact facts, never connections or unrelated nodes',()=>{
  const node={...model.content(draft),id:'manager'};
  const graph={nodes:[{id:'a',title:'Project A',facts:[{value:{type:'money',amount:'12.50',currency:'SEK'}}]},{id:'b',title:'Project B'},{id:'private',title:'Private unselected memory'}],connections:[{token:'SECRET'}]};
  const packet=model.briefing(node,graph);
  assert.ok(packet.includes('12.50'));assert.ok(packet.includes('Project B'));
  assert.ok(!packet.includes('Private unselected memory'));assert.ok(!packet.includes('SECRET'));
  assert.ok(packet.includes('First respond with your understanding'));assert.ok(packet.includes('does not grant API access'));
  const ready={...model.content({...draft,phase:'ready',understanding:'Confirmed scope.'}),id:'manager'};
  assert.ok(model.briefing(ready,graph).includes('Owner-confirmed understanding: Confirmed scope.'));
});
test('missing, archived and conflicted context fail closed; incomplete confirmation rejected',()=>{
  const node={...model.content(draft),id:'manager'};
  for(const graph of [{nodes:[]},{nodes:[{id:'a',status:'archived'},{id:'b'}]},{nodes:[{id:'a'},{id:'b'}],conflicts:[{nodeId:'a'}]},{nodes:[{id:'a'},{id:'b'}],conflicts:[{nodeId:'manager'}]}]) assert.throws(()=>model.briefing(node,graph));
  assert.throws(()=>model.content({...draft,phase:'ready'}));
  assert.throws(()=>model.content({...draft,contextIds:Array.from({length:21},(_,i)=>String(i))}));
  assert.throws(()=>model.content({...draft,reviewDate:'2026-02-30'}));
  assert.throws(()=>model.content({...draft,mission:'x'.repeat(1001)}));
});
