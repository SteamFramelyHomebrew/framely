async function(request) {
 const store=window.appStore, remote=window.RemotePlayStore_SteamUI;
 if(!store||!remote||!Array.isArray(store.allApps)||!Array.isArray(remote.devices)) {
  if(request.mode==='list')return [];
  throw Error('Steam remote library is unavailable');
 }
 const connected=new Map(remote.devices.filter(d=>d.status==='Connected').map(d=>[String(d.clientId),String(d.clientName||'').trim().slice(0,256)]));
 const rows=[];
 for(const a of store.allApps) {
  if(!Number.isInteger(a.appid)||a.appid<=0||a.appid>4294967295||a.visible_in_game_list===false||!(a.app_type&(1|2|8)))continue;
  for(const r of a.remote_per_client_data||[]) {
   const client=String(r.clientid);
   if(r.installed===true&&/^\d{1,20}$/.test(client)&&connected.has(client)&&a.BIsPerClientDataLocal?.(r)===false)
    rows.push({id:a.appid,name:String(a.display_name||''),client,deviceName:connected.get(client)});
  }
 }
 if(request.mode==='list')return rows.slice(0,10000);
 if(request.mode!=='launch'||!rows.some(r=>r.id===request.id&&r.client===request.client))return false;
 if(typeof SteamClient.Apps.StreamGame!=='function')throw Error('Steam streaming is unavailable');
 await SteamClient.Apps.StreamGame(request.id,request.client,-1);
 return true;
}
