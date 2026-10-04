type Transport={start:(size:number)=>Promise<{upload:string;chunkSize:number}>;chunk:(upload:string,offset:number,data:Blob,signal:AbortSignal)=>Promise<void>;abort:(upload:string)=>Promise<unknown>};
export async function uploadFile(file:Pick<File,'size'|'slice'>,signal:AbortSignal,progress:(percent:number)=>void,transport:Transport):Promise<string>{
 let ticket='';
 try{
  signal.throwIfAborted();const start=await transport.start(file.size);ticket=start.upload;
  if(!Number.isSafeInteger(start.chunkSize)||start.chunkSize<=0||start.chunkSize>512*1024)throw new Error('Invalid upload chunk size');
  for(let offset=0;offset<file.size;offset+=start.chunkSize){signal.throwIfAborted();await transport.chunk(ticket,offset,file.slice(offset,offset+start.chunkSize),signal);progress(Math.round(Math.min(offset+start.chunkSize,file.size)/file.size*100));}
  signal.throwIfAborted();return ticket;
 }catch(error){if(ticket)await transport.abort(ticket).catch(()=>{});throw error;}
}
