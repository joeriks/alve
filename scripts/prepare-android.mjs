import {readFile,writeFile,mkdir,copyFile,readdir} from 'node:fs/promises';
import {resolve,dirname,join} from 'node:path';

const root=resolve(import.meta.dirname,'..');
const project=join(root,'src-tauri/gen/android');
const manifestPath=join(project,'app/src/main/AndroidManifest.xml');
let manifest=await readFile(manifestPath,'utf8').catch(()=>{throw new Error('Run tauri android init before preparing Android.');});
const attribute=(name,value)=>{
  const pattern=new RegExp(`android:${name}="[^"]*"`);
  if(pattern.test(manifest))manifest=manifest.replace(pattern,`android:${name}="${value}"`);
  else manifest=manifest.replace('<application',`<application android:${name}="${value}"`);
};
attribute('allowBackup','false');
attribute('fullBackupContent','false');
attribute('dataExtractionRules','@xml/alve_backup_rules');
await writeFile(manifestPath,manifest);
const resource=join(project,'app/src/main/res/xml/alve_backup_rules.xml');
await mkdir(dirname(resource),{recursive:true});
const exclusions=['root','file','database','sharedpref','external'].map(domain=>`    <exclude domain="${domain}" path="." />`).join('\n');
await writeFile(resource,`<?xml version="1.0" encoding="utf-8"?>\n<data-extraction-rules>\n  <cloud-backup>\n${exclusions}\n  </cloud-backup>\n  <device-transfer>\n${exclusions}\n  </device-transfer>\n</data-extraction-rules>\n`);
const overlays=join(root,'src-tauri/android-overlay');
async function files(dir){const entries=await readdir(dir,{withFileTypes:true});return (await Promise.all(entries.map(entry=>entry.isDirectory()?files(join(dir,entry.name)):[join(dir,entry.name)]))).flat();}
for(const file of await files(overlays)){
  const target=join(project,file.slice(overlays.length+1));
  await mkdir(dirname(target),{recursive:true});await copyFile(file,target);
}
console.log('Android native overlays and explicit backup policy applied.');
