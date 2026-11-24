import assert from 'node:assert/strict';
import {readFile,readdir,stat,mkdtemp,rm,access} from 'node:fs/promises';
import {resolve,dirname,join,posix} from 'node:path';
import {tmpdir} from 'node:os';
import {spawnSync} from 'node:child_process';
import AdmZip from 'adm-zip';
const root=resolve('dist-docs'),config=JSON.parse(await readFile(join(root,'build.json'),'utf8'));
async function files(dir){const result=[];for(const entry of await readdir(dir,{withFileTypes:true})){const path=join(dir,entry.name);if(entry.isDirectory())result.push(...await files(path));else result.push(path);}return result;}
const {version:appVersion}=JSON.parse(await readFile('package.json','utf8'));
for(const [page,versionLine] of [
 ['index.html',`当前应用 <strong>${appVersion} / HPP `],
 ['docs/developers/index.html',`当前应用 <strong>${appVersion} / HPP `],
 ['docs/plugin-protocol/IMPLEMENTATION.html',`应用清单版本为 <strong>${appVersion}</strong>`],
]){
 const html=await readFile(join(root,page),'utf8');
 assert(html.includes(versionLine),`${page} must show the app version from package.json (${appVersion})`);
}
const pages=(await files(root)).filter(p=>p.endsWith('.html'));let links=0;
for(const page of pages){
 const html=await readFile(page,'utf8');
 for(const match of html.matchAll(/(?:href|src)="([^"]+)"/g)){
  const href=match[1].replaceAll('&amp;','&');if(/^[a-z]+:/i.test(href))continue;
  const [path,anchor]=href.split('#');const target=path?resolve(root,path.startsWith(config.base)?path.slice(config.base.length):path):page;
  const file=path===config.base?join(root,'index.html'):target;
  assert(file.startsWith(root),'Path escaped site');
  await access(file).catch(()=>{throw Error('Broken link '+href+' in '+page);});
  if(anchor&&file.endsWith('.html')){const text=await readFile(file,'utf8');assert(text.includes('id="'+decodeURIComponent(anchor)+'"'),'Missing anchor '+href+' in '+page);}
  links++;
 }
 assert(!/kanju|huangguohub\.js|signingConfig|client_secret/.test(html),'Private site content found');
}
const zip=new AdmZip(join(root,'downloads/hghub-plugin-dev.zip'));const entries=zip.getEntries();
for(const entry of entries)assert(entry.entryName.startsWith('hghub-plugin-dev/')&&!/(?:local-plugins|test-results|\.env|\.keystore)/.test(entry.entryName));
let archiveLinks=0;
for(const entry of entries.filter(e=>e.entryName.endsWith('.md'))){
 for(const match of entry.getData().toString('utf8').matchAll(/\]\(([^)]+)\)/g)){
  const href=match[1];if(/^(?:[a-z]+:|#)/i.test(href))continue;
  const target=posix.normalize(posix.join(posix.dirname(entry.entryName),href.split('#')[0]));
  assert(zip.getEntry(target),'Broken archive link '+href+' in '+entry.entryName);archiveLinks++;
 }
}
for(const name of ['SKILL.md','assets/protocol.ts','assets/plugin-hello.js','references/status.md','references/protocol/04-host-api.md'])assert(zip.getEntry('hghub-plugin-dev/'+name),'Missing skill resource '+name);
const stage=await mkdtemp(join(tmpdir(),'hghub-docs-install-'));
try{
 const python=process.env.PYTHON||'python';
 let result=spawnSync(python,['tools/plugin-dev/install.py','--archive',join(root,'downloads/hghub-plugin-dev.zip'),'--destination',stage],{encoding:'utf8'});
 assert.equal(result.status,0,result.stderr);assert((await stat(join(stage,'hghub-plugin-dev/SKILL.md'))).isFile());
 result=spawnSync(python,['tools/plugin-dev/install.py','--archive',join(root,'downloads/hghub-plugin-dev.zip'),'--destination',stage],{encoding:'utf8'});
 assert.notEqual(result.status,0,'Installer overwrote an existing skill');
}finally{await rm(stage,{recursive:true,force:true});}
console.log(JSON.stringify({pages:pages.length,localLinksChecked:links,archiveLinks,skillFiles:entries.length,installation:'passed; overwrite rejected'}));
