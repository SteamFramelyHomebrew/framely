// Separately bundled and loaded only when the text editor opens. No language
// servers, autocomplete, lint diagnostics or network requests are installed.
import {EditorState,Compartment,countColumn,Text} from '@codemirror/state';
import {EditorView,keymap,lineNumbers,highlightActiveLineGutter,highlightSpecialChars,drawSelection,highlightActiveLine} from '@codemirror/view';
import {history,historyKeymap,defaultKeymap,indentWithTab,undo,redo,deleteCharBackward,cursorCharLeft,cursorCharRight,cursorLineUp,cursorLineDown,insertNewlineAndIndent} from '@codemirror/commands';
import {foldGutter,foldKeymap,indentOnInput,indentUnit,bracketMatching,syntaxHighlighting,HighlightStyle} from '@codemirror/language';
import {tags} from '@lezer/highlight';
import {search,searchKeymap,highlightSelectionMatches,openSearchPanel,closeSearchPanel} from '@codemirror/search';
import {createSearchPanel} from './file-code-search';
import {javascript} from '@codemirror/lang-javascript';
import {json} from '@codemirror/lang-json';
import {css} from '@codemirror/lang-css';
import {html} from '@codemirror/lang-html';
import {python} from '@codemirror/lang-python';
import {rust} from '@codemirror/lang-rust';
export type CursorStatus={line:number;column:number;selected:number;lines:number};
export type CodeEditorOptions={parent:HTMLElement;value:string;path:string;readOnly:boolean;label:string;phrases:Record<string,string>;onChange:(text:string)=>void;onSave:()=>void;onCursor?:(status:CursorStatus)=>void};
function language(mode:string){switch(mode){case 'javascript':return javascript({jsx:true});case 'typescript':return javascript({typescript:true,jsx:true});case 'json':return json();case 'css':return css();case 'html':return html();case 'python':return python();case 'rust':return rust();default:return [];}}
export function languageForPath(path:string){const ext=path.split('.').pop()?.toLowerCase()??'';return ({js:'javascript',mjs:'javascript',cjs:'javascript',jsx:'javascript',ts:'typescript',tsx:'typescript',json:'json',css:'css',html:'html',htm:'html',py:'python',rs:'rust'} as Record<string,string>)[ext]??'text';}
const separator=(value:string)=>value.includes('\r\n')?'\r\n':value.includes('\r')&&!value.includes('\n')?'\r':'\n';
const highlighting=HighlightStyle.define([{tag:tags.keyword,color:'#c9a3e8'},{tag:tags.string,color:'#b6cea4'},{tag:[tags.number,tags.bool,tags.null],color:'#dfbe91'},{tag:tags.typeName,color:'#8ecbd1'},{tag:tags.function(tags.variableName),color:'#e0d0a0'},{tag:tags.comment,color:'#909f8c',fontStyle:'italic'},{tag:[tags.propertyName,tags.attributeName],color:'#a5c9eb'},{tag:tags.tagName,color:'#a5c9eb'},{tag:tags.operator,color:'#c5cbd5'}]);
export function mount(options:CodeEditorOptions){
 const editable=new Compartment(),wrapping=new Compartment(),syntax=new Compartment(),endings=new Compartment();let updating=false;
 const cursor=(state:EditorState)=>{const selection=state.selection.main,line=state.doc.lineAt(selection.head);options.onCursor?.({line:line.number,column:countColumn(state.sliceDoc(line.from,selection.head),state.tabSize)+1,selected:selection.to-selection.from,lines:state.doc.lines});};
 const view=new EditorView({parent:options.parent,state:EditorState.create({doc:options.value,extensions:[
  lineNumbers(),highlightActiveLineGutter(),highlightSpecialChars(),history(),foldGutter(),drawSelection(),indentOnInput(),bracketMatching(),syntaxHighlighting(highlighting),highlightActiveLine(),highlightSelectionMatches(),syntax.of(language(languageForPath(options.path))),indentUnit.of('  '),EditorState.tabSize.of(4),endings.of(EditorState.lineSeparator.of(separator(options.value))),
  editable.of([EditorState.readOnly.of(options.readOnly),EditorView.editable.of(!options.readOnly)]),wrapping.of([]),
  search({createPanel:createSearchPanel}),EditorState.phrases.of(options.phrases),EditorView.contentAttributes.of({'aria-label':options.label,'data-framely-editor-input':''}),
  keymap.of([{key:'Mod-s',scope:'editor search-panel',run:()=>{options.onSave();return true;}},...defaultKeymap,...historyKeymap,...searchKeymap,...foldKeymap,indentWithTab]),
  EditorView.updateListener.of(update=>{if(update.docChanged&&!updating)options.onChange(update.state.sliceDoc());if(update.docChanged||update.selectionSet)cursor(update.state);}),
  EditorView.theme({'&':{height:'100%',backgroundColor:'#1c1e22',color:'#dce1e8',fontSize:'14px'},'.cm-scroller':{fontFamily:'ui-monospace,SFMono-Regular,Consolas,monospace',overflow:'auto',lineHeight:'1.7'},'.cm-content':{padding:'18px 0'},'.cm-line':{padding:'0 20px'},'.cm-gutters':{backgroundColor:'#1c1e22',color:'#7f8998',borderRight:'none',padding:'0 4px 0 12px'},'.cm-activeLineGutter':{backgroundColor:'transparent',color:'#e0e6ef'},'.cm-activeLine':{backgroundColor:'#ffffff05'},'.cm-cursor':{borderLeftColor:'#b9d8f4',borderLeftWidth:'2px'},'&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection':{backgroundColor:'#42638480'},'.cm-panels':{backgroundColor:'#25282f',color:'#e0e2e6'},'.cm-textfield':{backgroundColor:'#1c1d20',color:'#e0e2e6',border:'1px solid #656b76'},'.cm-button':{backgroundImage:'none',backgroundColor:'#35383e',color:'#e0e2e6',border:'1px solid #656b76'}},{dark:true})
 ]})});
 const input=view.contentDOM;
 const open=(event:Event)=>{(event as CustomEvent).detail.existing=view.state.sliceDoc(view.state.selection.main.from,view.state.selection.main.to);};
 const commit=(event:Event)=>{event.preventDefault();if(!view.state.readOnly)view.dispatch(view.state.replaceSelection(String((event as CustomEvent).detail)));};
 const edit=(event:Event)=>{
  event.preventDefault();if(view.state.readOnly)return;const value=String((event as CustomEvent).detail);
  const commands:Record<string,(v:EditorView)=>boolean>={'\b':deleteCharBackward,'\x1b[D':cursorCharLeft,'\x1b[C':cursorCharRight,'\x1b[A':cursorLineUp,'\x1b[B':cursorLineDown,'\n':insertNewlineAndIndent,'\r':insertNewlineAndIndent};
  if(commands[value])commands[value](view);else if(value&&value.charCodeAt(0)>=32)view.dispatch(view.state.replaceSelection(value));
  view.scrollDOM.querySelector('.cm-cursor')?.scrollIntoView({block:'nearest'});
 };
 input.addEventListener('framely.keyboard.open',open);input.addEventListener('framely.keyboard.commit',commit);input.addEventListener('framely.keyboard.edit',edit);
 cursor(view.state);
 return {
  setValue(value:string){if(value!==view.state.sliceDoc()){updating=true;try{view.dispatch({changes:{from:0,to:view.state.doc.length,insert:Text.of(value.split(/\r\n|\r|\n/))},effects:endings.reconfigure(EditorState.lineSeparator.of(separator(value)))});}finally{updating=false;}}},
  setLanguage(mode:string){view.dispatch({effects:syntax.reconfigure(language(mode))});},
  setReadOnly(readOnly:boolean){view.dispatch({effects:editable.reconfigure([EditorState.readOnly.of(readOnly),EditorView.editable.of(!readOnly)])});},
  wrap(enabled:boolean){view.dispatch({effects:wrapping.reconfigure(enabled?EditorView.lineWrapping:[])});},
  undo(){undo(view);},redo(){redo(view);},find(){openSearchPanel(view);},closeSearch(){const closed=closeSearchPanel(view);if(closed)view.focus();return closed;},
  destroy(){input.removeEventListener('framely.keyboard.open',open);input.removeEventListener('framely.keyboard.commit',commit);input.removeEventListener('framely.keyboard.edit',edit);view.destroy();}
 };
}
export type MountedCodeEditor=ReturnType<typeof mount>;
