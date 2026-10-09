export type UploadEntry={name:string;directory:boolean;file?:File;error?:string};
type BrowserEntry={name:string;isDirectory:boolean;isFile:boolean;file:(success:(file:File)=>void,failure:(error:DOMException)=>void)=>void;createReader:()=>{readEntries:(success:(entries:BrowserEntry[])=>void,failure:(error:DOMException)=>void)=>void}};
function validate(name:string){if(!name||name.startsWith('/')||name.split('/').some(part=>!part||part==='.'||part==='..'||part.includes('\0')))throw new Error('Invalid upload path');}
function add(entries:Map<string,UploadEntry>,entry:UploadEntry){validate(entry.name);const previous=entries.get(entry.name);if(previous&&(previous.directory!==entry.directory||!entry.directory))throw new Error('Duplicate upload path');if(!previous&&entries.size>=100000)throw new Error('Too many upload entries');entries.set(entry.name,entry);}
function parents(entries:Map<string,UploadEntry>,name:string){const parts=name.split('/');for(let i=1;i<parts.length;i++)add(entries,{name:parts.slice(0,i).join('/'),directory:true});}
export async function selectedUploadEntries(files:File[],signal?:AbortSignal):Promise<UploadEntry[]>{const entries=new Map<string,UploadEntry>();let visited=0;for(const file of files){signal?.throwIfAborted();if(++visited%100===0)await new Promise(resolve=>setTimeout(resolve,0));const name=file.webkitRelativePath||file.name;parents(entries,name);add(entries,{name,directory:false,file});}return [...entries.values()];}
/** Capture drag data synchronously: browsers revoke DataTransfer after drop. */
export function droppedUploadEntries(data:DataTransfer,signal:AbortSignal):Promise<UploadEntry[]>{
 const items=Array.from(data.items).filter(item=>item.kind==='file');
 const roots=items.map(item=>(item as unknown as {webkitGetAsEntry?:()=>BrowserEntry|null}).webkitGetAsEntry?.()??null);
 const fallback=Array.from(data.files);
 return (async()=>{
  const entries=new Map<string,UploadEntry>();let visited=0;
  async function walk(entry:BrowserEntry,prefix=''){
   signal.throwIfAborted();const name=prefix+entry.name;
   if(++visited>100000)throw new Error('Too many upload entries');
   if(entry.isDirectory){
    add(entries,{name,directory:true});const reader=entry.createReader();
    for(;;){signal.throwIfAborted();const page=await new Promise<BrowserEntry[]>((resolve,reject)=>reader.readEntries(resolve,reject));if(!page.length)break;for(const child of page)await walk(child,name+'/');}
   }else if(entry.isFile){const file=await new Promise<File>((resolve,reject)=>entry.file(resolve,reject));add(entries,{name,directory:false,file});}
   if(visited%100===0)await new Promise(resolve=>setTimeout(resolve,0));
  }
  if(roots.length&&roots.every(Boolean)){for(const root of roots)await walk(root!);return [...entries.values()];}
  if(roots.some(Boolean)||fallback.length!==items.length||!fallback.length)throw new Error('Folder drag-and-drop is unavailable; use Upload folder');
  signal.throwIfAborted();return selectedUploadEntries(fallback);
 })();
}
type DirectoryHandle={name:string;kind:'directory';values:()=>AsyncIterable<FileHandle|DirectoryHandle>};
type FileHandle={name:string;kind:'file';getFile:()=>Promise<File>};
export function canPickDirectory(){return typeof (window as Window&{showDirectoryPicker?:unknown}).showDirectoryPicker==='function';}
export async function pickUploadDirectory(signal:AbortSignal):Promise<UploadEntry[]>{
 const picker=(window as unknown as {showDirectoryPicker:()=>Promise<DirectoryHandle>}).showDirectoryPicker;
 const root=await picker.call(window),entries=new Map<string,UploadEntry>();let visited=0;
 async function walk(handle:FileHandle|DirectoryHandle,prefix=''){
  signal.throwIfAborted();if(++visited>100000)throw new Error('Too many upload entries');const name=prefix+handle.name;
  if(handle.kind==='file')add(entries,{name,directory:false,file:await handle.getFile()});
  else{add(entries,{name,directory:true});for await(const child of handle.values())await walk(child,name+'/');}
  if(visited%100===0)await new Promise(resolve=>setTimeout(resolve,0));
 }
 await walk(root);return [...entries.values()];
}
