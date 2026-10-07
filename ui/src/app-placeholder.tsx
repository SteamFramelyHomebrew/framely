import React from 'react';
import {IconApps,IconBrandAndroid,IconBrandSteam,IconDeviceDesktop,IconPuzzle} from '@tabler/icons-react';

// One neutral placeholder per application type, independent of its display name.
export function AppPlaceholder({kind='plugin',size=28}:{kind?:string;size?:number}){
 const Glyph=kind==='plugin'?IconPuzzle:kind==='steam'?IconBrandSteam:kind==='lepton'||kind==='apk'?IconBrandAndroid:kind==='desktop'?IconDeviceDesktop:IconApps;
 return <Glyph className="app-placeholder" data-app-kind={kind} size={size} stroke={1.5} aria-hidden="true" style={{color:'#b7bdc4',flexShrink:0,pointerEvents:'none'}}/>;
}
