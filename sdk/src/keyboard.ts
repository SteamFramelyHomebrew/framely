type Editable=HTMLInputElement|HTMLTextAreaElement;
export function installKeyboard(open:(params:{existing:string;password:boolean;multiline:boolean})=>Promise<unknown>){
 let field:Editable|null=null,last:Editable|null=null,openedAt=0;
 const request=(target:EventTarget|null)=>{
  if(!(target instanceof HTMLInputElement||target instanceof HTMLTextAreaElement)||target.disabled||target.readOnly)return;
  if(target instanceof HTMLInputElement&&!['text','password','url','search','email','number','tel'].includes(target.type))return;
  field=target;const now=performance.now();if(last===target&&now-openedAt<300)return;last=target;openedAt=now;
  void open({existing:target.value,password:target instanceof HTMLInputElement&&target.type==='password',multiline:target instanceof HTMLTextAreaElement}).catch(console.error);
 };
 document.addEventListener('focusin',e=>request(e.target));
 document.addEventListener('pointerdown',e=>request(e.target));
 (window as any).__framelyCommitKeyboard=(value:string)=>{
  if(!field?.isConnected)return;
  const proto=field instanceof HTMLTextAreaElement?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto,'value')!.set!.call(field,value);
  field.dispatchEvent(new Event('input',{bubbles:true}));field.dispatchEvent(new Event('change',{bubbles:true}));
  // Refocusing here would reopen the keyboard as its close event is processed.
 };
 (window as any).__framelyEditKeyboard=(edit:string)=>{
  if(!field?.isConnected)return;
  const value=field.value;
  let start=field.selectionStart??value.length,end=field.selectionEnd??start;
  const segments=Array.from(new (Intl as any).Segmenter(undefined,{granularity:'grapheme'}).segment(value)) as {index:number;segment:string}[];
  const boundaries=[...segments.map(s=>s.index),value.length];
  const previous=()=>boundaries.filter(p=>p<start).at(-1)??0;
  const next=()=>boundaries.find(p=>p>end)??value.length;
  if(edit==='\x1b[D'||edit==='\x1b[C'){
   const position=edit==='\x1b[D'?(start!==end?start:previous()):(start!==end?end:next());
   try{field.setSelectionRange(position,position);}catch{}
   return;
  }
  if(edit==='\x1b[A'||edit==='\x1b[B'){
   const lineStart=value.lastIndexOf('\n',start-1)+1,column=start-lineStart;
   let position=edit==='\x1b[A'?0:value.length;
   if(field instanceof HTMLTextAreaElement){
    if(edit==='\x1b[A'&&lineStart>0){const previousStart=value.lastIndexOf('\n',lineStart-2)+1;position=Math.min(previousStart+column,lineStart-1);}
    if(edit==='\x1b[B'){const lineEnd=value.indexOf('\n',end);if(lineEnd>=0){const nextEnd=value.indexOf('\n',lineEnd+1);position=Math.min(lineEnd+1+column,nextEnd<0?value.length:nextEnd);}}
   }
   position=boundaries.filter(p=>p<=position).at(-1)??0;
   try{field.setSelectionRange(position,position);}catch{}
   return;
  }
  if(edit==='\b'){if(start===end)start=previous();edit='';}
  else if(edit==='\n'||edit==='\r'){if(!(field instanceof HTMLTextAreaElement))return;edit='\n';}
  else if(!edit||edit.charCodeAt(0)<32)return;
  const updated=value.slice(0,start)+edit+value.slice(end),position=start+edit.length;
  (window as any).__framelyCommitKeyboard(updated);
  try{field.setSelectionRange(position,position);}catch{}
 };
}
