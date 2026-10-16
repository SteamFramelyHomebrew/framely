import {EditorView,runScopeHandlers,type Panel,type ViewUpdate} from '@codemirror/view';
import {SearchQuery,getSearchQuery,setSearchQuery,findNext,findPrevious,replaceNext,replaceAll,closeSearchPanel} from '@codemirror/search';

// A compact editor find widget. Matching, replacement and undo remain owned by
// CodeMirror, so regex captures, word boundaries and document edits stay in sync.
export function createSearchPanel(view:EditorView):Panel {
 const dom=document.createElement('div');dom.className='cm-search file-code-search';
 const phrase=(key:string)=>view.state.phrase(key);
 const row=(name:string)=>{const el=document.createElement('div');el.className=name;dom.append(el);return el;};
 const main=row('file-search-row'),replacement=row('file-search-row file-search-replacement');
 const icons:Record<string,string>={previous:'M12 19V5m-6 6 6-6 6 6',next:'M12 5v14m-6-6 6 6 6-6',close:'m6 6 12 12M6 18 18 6','Toggle replace':'m9 5 7 7-7 7'};
 const button=(parent:HTMLElement,name:string,glyph:string,action:()=>void)=>{const el=document.createElement('button');el.type='button';el.name=name;
  if(icons[name]){const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');svg.setAttribute('viewBox','0 0 24 24');svg.setAttribute('fill','none');svg.setAttribute('stroke','currentColor');svg.setAttribute('stroke-width','1.7');svg.setAttribute('aria-hidden','true');const path=document.createElementNS(svg.namespaceURI,'path');path.setAttribute('d',icons[name]);svg.append(path);el.append(svg);}else el.textContent=glyph;
  el.title=phrase(name);el.setAttribute('aria-label',phrase(name));el.addEventListener('click',action);parent.append(el);return el;};
 const field=(parent:HTMLElement,name:string)=>{const el=document.createElement('input');el.type='text';el.name=name;el.placeholder=phrase(name);el.setAttribute('aria-label',phrase(name));el.autocomplete='off';el.spellcheck=false;parent.append(el);return el;};
 let expanded=false,query=getSearchQuery(view.state),matches:{from:number;to:number}[]=[],truncated=false;
 const expand=button(main,'Toggle replace','›',()=>{expanded=!expanded;refresh();view.requestMeasure();});expand.setAttribute('aria-expanded','false');
 const inputGroup=document.createElement('div');inputGroup.className='file-search-input';main.append(inputGroup);
 const input=field(inputGroup,'Find');input.setAttribute('main-field','true');
 const options=document.createElement('div');options.className='file-search-options';inputGroup.append(options);
 const toggles=(['caseSensitive','wholeWord','regexp'] as const).map((key,index)=>{const el=button(options,['match case','by word','regexp'][index],['Aa','ab','.*'][index],()=>{commit({[key]:!query[key]});input.focus();});return {key,el};});
 const count=document.createElement('output');count.className='file-search-count';count.setAttribute('aria-live','polite');main.append(count);
 const previous=button(main,'previous','↑',()=>findPrevious(view)),next=button(main,'next','↓',()=>findNext(view));
 button(main,'close','×',()=>{closeSearchPanel(view);view.focus();});
 const replaceInput=field(replacement,'Replace');
 const one=button(replacement,'replace',phrase('Replace'),()=>replaceNext(view)),all=button(replacement,'replace all',phrase('replace all'),()=>replaceAll(view));
 const message=row('file-search-message');message.setAttribute('role','status');
 function commit(change:Partial<{caseSensitive:boolean;wholeWord:boolean;regexp:boolean}>={}) {
  const nextQuery=new SearchQuery({...query,search:input.value,replace:replaceInput.value,...change});
  if(!nextQuery.eq(query))view.dispatch({effects:setSearchQuery.of(nextQuery)});
 }
 function scan(){matches=[];truncated=false;if(query.valid){const cursor=query.getCursor(view.state);for(let step=cursor.next();!step.done;step=cursor.next()){if(matches.length===10000){truncated=true;break;}matches.push(step.value);}}}
 function refresh(){
  input.value=query.search;replaceInput.value=query.replace;
  for(const {key,el} of toggles)el.setAttribute('aria-pressed',String(query[key]));
  expand.setAttribute('aria-expanded',String(expanded));
  replacement.hidden=!expanded;replaceInput.disabled=view.state.readOnly;
  const selection=view.state.selection.main,index=matches.findIndex(m=>m.from===selection.from&&m.to===selection.to);
  const invalid=!!query.search&&!query.valid;
  input.setAttribute('aria-invalid',String(invalid));
  count.textContent=query.search?(invalid?'—':`${index<0?'–':index+1} / ${matches.length}${truncated?'+':''}`):'';
  previous.disabled=next.disabled=!matches.length;
  one.disabled=all.disabled=view.state.readOnly||!matches.length;
  message.textContent=invalid?phrase('Invalid regular expression'):query.search&&!matches.length?phrase('No results'):expanded&&view.state.readOnly?phrase('Read-only document'):'';
  message.hidden=!message.textContent;
 }
 input.addEventListener('input',()=>commit());replaceInput.addEventListener('input',()=>commit());
 dom.addEventListener('keydown',event=>{
  if(event.key==='Escape'){event.preventDefault();event.stopPropagation();closeSearchPanel(view);view.focus();}
  else if(runScopeHandlers(view,event,'search-panel')){event.preventDefault();event.stopPropagation();}
  else if(event.key==='Enter'&&!event.isComposing){event.preventDefault();event.stopPropagation();if(event.target===replaceInput){if(!view.state.readOnly)(event.altKey?replaceAll:replaceNext)(view);}else(event.shiftKey?findPrevious:findNext)(view);}
 });
 scan();refresh();
 return {dom,top:true,update(update:ViewUpdate){const nextQuery=getSearchQuery(update.state),changed=!query.eq(nextQuery);query=nextQuery;if(changed||update.docChanged)scan();refresh();}};
}
