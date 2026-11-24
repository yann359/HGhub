import {readFile,writeFile,mkdir,cp,readdir} from 'node:fs/promises';
import {resolve,dirname,posix} from 'node:path';
import MarkdownIt from 'markdown-it';
import AdmZip from 'adm-zip';
const root=resolve('.'),out=resolve('dist-docs');
const base=process.env.HGHUB_DOCS_BASE||'/HGhub/';
if(!/^\/(?:[A-Za-z0-9_-]+\/)*$/.test(base))throw Error('Invalid HGHUB_DOCS_BASE');
const protocol=(await readdir('docs/plugin-protocol')).filter(n=>/^0[1-9]-.*\.md$/.test(n)||['README.md','IMPLEMENTATION.md'].includes(n)).sort();
const guides=(await readdir('docs/developers')).filter(n=>n.endsWith('.md')).sort();
const sources=[...guides.map(n=>'docs/developers/'+n),...protocol.map(n=>'docs/plugin-protocol/'+n)];
const dest=file=>file.replace(/\.md$/,'.html');
const escape=text=>text.replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const md=new MarkdownIt({html:false,linkify:true});
const rendered=new Map();
for(const source of sources){
 const text=await readFile(source,'utf8'),tokens=md.parse(text,{});
 const walk=items=>{for(const token of items){if(token.type==='link_open'){const href=token.attrGet('href');if(href&&!/^(?:[a-z]+:|#)/i.test(href)){const [path,fragment]=href.split('#');const target=posix.normalize(posix.join(posix.dirname(source),path));if(target.endsWith('.md')){if(!sources.includes(target))token.attrSet('href','https://github.com/Minireel/HGhub/blob/main/'+target+(fragment?'#'+fragment:''));else token.attrSet('href',base+dest(target)+(fragment?'#'+fragment:''));}else token.attrSet('href',base+target+(fragment?'#'+fragment:''));}}if(token.children)walk(token.children);}};
 walk(tokens);const ids=new Map();
 for(let i=0;i<tokens.length;i++)if(tokens[i].type==='heading_open'){const title=tokens[i+1].content;const stem=title.toLowerCase().replace(/[^\p{L}\p{N}\s_-]/gu,'').replace(/\s/g,'-');const count=ids.get(stem)||0;ids.set(stem,count+1);tokens[i].attrSet('id',stem+(count?'-'+count:''));}
 rendered.set(source,{title:text.match(/^# (.+)/m)?.[1]||source,body:md.renderer.render(tokens,md.options,{})});
}
const nav=[['开始','index'],['教程','quickstart'],['能力 / API','host-api'],['调试','testing'],['分发','distribution'],['支持状态','status'],['Agent 技能','agent'],['部署','deployment']];
const protocolNav=protocol.map(n=>'<a href="'+base+dest('docs/plugin-protocol/'+n)+'">'+escape(rendered.get('docs/plugin-protocol/'+n).title)+'</a>').join('');
for(const [source,{title,body}]of rendered){
 const page='<!doctype html><html lang="zh-CN"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><link rel="icon" href="data:,"><title>'+escape(title)+' · HGhub</title><link rel="stylesheet" href="'+base+'assets/site.css"></head><body><header><a class="brand" href="'+base+'">HGhub <span>开发者</span></a><span>HPP 1.0 草案</span><button id="menu" aria-label="切换导航" aria-expanded="false">目录</button></header><aside id="navigation"><nav>'+nav.map(([label,name])=>'<a '+(source==='docs/developers/'+name+'.md'?'aria-current="page" ':'')+'href="'+base+'docs/developers/'+name+'.html">'+label+'</a>').join('')+'<details><summary>协议参考 01–09</summary>'+protocolNav+'</details></nav></aside><main id="main">'+body+'<footer>HGhub · 本地优先 · 公开开发资料</footer></main><script src="'+base+'assets/site.js" defer></script></body></html>';
 await mkdir(dirname(resolve(out,dest(source))),{recursive:true});await writeFile(resolve(out,dest(source)),page);
 if(source==='docs/developers/index.md')await writeFile(resolve(out,'index.html'),page);
}
await mkdir(resolve(out,'assets'),{recursive:true});await cp('docs/site/site.css',resolve(out,'assets/site.css'));await cp('docs/site/site.js',resolve(out,'assets/site.js'));
await mkdir(resolve(out,'examples'),{recursive:true});await cp('examples/plugin-hello.js',resolve(out,'examples/plugin-hello.js'));
await mkdir(resolve(out,'downloads'),{recursive:true});
const zip=new AdmZip();const add=async(source,target)=>zip.addFile('hghub-plugin-dev/'+target,await readFile(source));
await add('tools/plugin-dev/SKILL.md','SKILL.md');await add('src/plugins/protocol.ts','assets/protocol.ts');await add('examples/plugin-hello.js','assets/plugin-hello.js');
for(const name of guides.filter(n=>!['index.md','agent.md','deployment.md'].includes(n))){
 const guide=(await readFile('docs/developers/'+name,'utf8')).replaceAll('../plugin-protocol/','protocol/').replaceAll('../../examples/','../assets/').replaceAll('../../downloads/protocol.ts','../assets/protocol.ts');
 zip.addFile('hghub-plugin-dev/references/'+name,Buffer.from(guide));
}
for(const name of protocol){
 const text=(await readFile('docs/plugin-protocol/'+name,'utf8')).replaceAll('../developers/','../');
 zip.addFile('hghub-plugin-dev/references/protocol/'+name,Buffer.from(text));
}
await writeFile(resolve(out,'downloads/hghub-plugin-dev.zip'),zip.toBuffer());
await cp('tools/plugin-dev/install.py',resolve(out,'downloads/install.py'));
await cp('src/plugins/protocol.ts',resolve(out,'downloads/protocol.ts'));
await writeFile(resolve(out,'.nojekyll'),'');await writeFile(resolve(out,'build.json'),JSON.stringify({base,pages:[...rendered.keys()].map(dest),builtAt:new Date().toISOString()},null,2));
console.log('Built '+rendered.size+' pages in '+out+' (base '+base+')');
