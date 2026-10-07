import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const lookup=eval(`(${await readFile('src/apk/steam_shortcut_lookup.js','utf8')})`);
const id=0xf1234567, exe='/home/steamos/devkit-game/owned/base.apk';
let targets=new Map([[id,{strShortcutExe:`"${exe}"`}]]),unregistered=0;
globalThis.appStore={m_mapApps:new Map([[id,{appid:id,display_name:'GamePad Tester'}]])};
globalThis.SteamClient={Apps:{RegisterForAppDetails:(id,cb)=>{queueMicrotask(()=>cb(targets.get(id)??{}));return {unregister(){unregistered++}}}}};
assert.deepEqual(await lookup({game:'owned',exe}),{id});
assert.ok(unregistered>0);
assert.deepEqual(await lookup({game:'owned',exe,previous:id}),{id});
// Never accept an identically named entry belonging to a different executable.
appStore.m_mapApps.set(id,{appid:id,display_name:'Devkit Game: owned'});
targets.set(id,{strShortcutExe:'/another/app.apk'});
await assert.rejects(lookup({game:'owned',exe}),/target does not match/);
appStore.m_mapApps.set(id,{appid:id,display_name:'GamePad Tester'});
await assert.rejects(lookup({game:'owned',exe}),/Missing or ambiguous/);
targets.set(id,{strShortcutExe:exe});
appStore.m_mapApps.set(id+1,{appid:id+1,display_name:'Another copy'});targets.set(id+1,{strShortcutExe:exe});
await assert.rejects(lookup({game:'owned',exe}),/ambiguous/);
// Cached details can call back before the subscription is returned.
appStore.m_mapApps.delete(id+1);
SteamClient.Apps.RegisterForAppDetails=(id,cb)=>{cb(targets.get(id));return {unregister(){unregistered++}}};
assert.deepEqual(await lookup({game:'owned',exe,previous:id}),{id});
console.log('Renamed registration recovery, executable ownership, ambiguity and cached callback checks passed.');
