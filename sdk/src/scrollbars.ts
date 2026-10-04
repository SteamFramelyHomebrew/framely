// Shared by the manager and host bootstrap, including already-installed plugins.
export function installScrollbars(){
 if(document.getElementById('framely-scrollbars'))return;
 const style=document.createElement('style');style.id='framely-scrollbars';
 style.textContent=`
html{color-scheme:dark}
::-webkit-scrollbar{width:12px;height:12px}
::-webkit-scrollbar-track{background:transparent}
::-webkit-scrollbar-thumb{background:#46596b;border:3px solid transparent;border-radius:12px;background-clip:padding-box;min-height:36px;min-width:36px}
::-webkit-scrollbar-thumb:hover{background-color:#6a8299}
::-webkit-scrollbar-thumb:active{background-color:#84b9de}
::-webkit-scrollbar-corner{background:transparent}
::-webkit-scrollbar-button{display:none}
@supports not selector(::-webkit-scrollbar){*{scrollbar-width:thin;scrollbar-color:#46596b transparent}}
`;
 document.head.append(style);
}
