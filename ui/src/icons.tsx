import React from 'react';
export type IconName='star'|'plugins'|'settings'|'arrow'|'close'|'more'|'download'|'search'|'shield'|'link'|'refresh'|'info'|'power'|'connection'|'edit'|'trash'|'store';
const paths:Record<IconName,React.ReactNode>={
 store:<><path d="M4 10v11h16V10M3 10l2-7h14l2 7M3 10a3 3 0 0 0 6 0 3 3 0 0 0 6 0 3 3 0 0 0 6 0M9 21v-6h6v6"/></>,
 power:<><path d="M12 3v9M7 5a8 8 0 1 0 10 0"/></>,
 connection:<><path d="M7 3v4m6-4v4M5 7h10v3a5 5 0 0 1-5 5v5M17 16l2 2 3-4"/></>,
 edit:<><path d="m15 4 5 5M4 20l5-1L21 7a2 2 0 0 0-5-5L4 14Z"/></>,
 trash:<><path d="M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7m4-7v7"/></>,
 star:<path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9Z"/>,
 plugins:<path d="M7 5h3V3a2 2 0 0 1 4 0v2h5v5h2a2 2 0 0 1 0 4h-2v5h-5v-2a2 2 0 0 0-4 0v2H5v-5h2a2 2 0 0 0 0-4H5V7a2 2 0 0 1 2-2Z"/> ,
 settings:<><path d="m10 3-.6 2.2-2 .9-2-.6L3 9l1.5 1.7v2.5L3 15l2.4 3.5 2-.6 2 .9L10 21h4l.6-2.2 2-.9 2 .6L21 15l-1.5-1.8v-2.5L21 9l-2.4-3.5-2 .6-2-.9L14 3Z"/><circle cx="12" cy="12" r="3"/></>,
 arrow:<path d="m14 5-7 7 7 7"/>,close:<path d="m6 6 12 12M18 6 6 18"/>,more:<><circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/></>,
 download:<><path d="M12 3v12m-5-5 5 5 5-5M4 17v4h16v-4"/></>,search:<><circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5"/></>,
 shield:<><path d="m12 3 8 3v6c0 5-8 9-8 9s-8-4-8-9V6Z"/><path d="m8 12 3 3 5-6"/></>,
 link:<><path d="m9 15 6-6M8 17l-1 1a4 4 0 0 1-6-6l4-4a4 4 0 0 1 6 0m2-1 1-1a4 4 0 0 1 6 6l-4 4a4 4 0 0 1-6 0"/></>,
 info:<><circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7h.01"/></>,
 refresh:<><path d="M20 7v5h-5M4 17v-5h5"/><path d="M6 7a7 7 0 0 1 12-1l2 3M4 15l2 3a7 7 0 0 0 12-1"/></>,
};
export function Icon({name,size=24,filled=false}:{name:IconName;size?:number;filled?:boolean}){return <svg width={size} height={size} viewBox="0 0 24 24" fill={filled?'currentColor':'none'} stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name]}</svg>}
export function Brand({wordmark=false}:{wordmark?:boolean}={}){return <span className={`brand-mark${wordmark?' brand-wordmark':''}`}><img src={`/assets/branding/framely-${wordmark?'logo':'mark'}-light.svg`} alt={wordmark?'Framely':''} aria-hidden={wordmark?undefined:true}/></span>}
