type Transport={start:(size:number)=>Promise<{upload:string;chunkSize:number}>;chunk:(upload:string,offset:number,data:Blob,signal:AbortSignal)=>Promise<void>;abort:(upload:string)=>Promise<unknown>};
export async function uploadFile(file:Pick<File,'size'|'slice'>,signal:AbortSignal,progress:(percent:number,received:number)=>void,transport:Transport):Promise<string>{
 let ticket='',nextOffset=0,received=0;
 const controller=new AbortController();
 const cancel=()=>controller.abort(signal.reason);
 signal.addEventListener('abort',cancel,{once:true});
 try{
  signal.throwIfAborted();const start=await transport.start(file.size);ticket=start.upload;
  signal.throwIfAborted();
  if(!Number.isSafeInteger(start.chunkSize)||start.chunkSize<=0||start.chunkSize>4*1024*1024)throw new Error('Invalid upload chunk size');
  const worker=async()=>{
   try{
    while(nextOffset<file.size){
     controller.signal.throwIfAborted();
     const offset=nextOffset;nextOffset+=start.chunkSize;
     const chunk=file.slice(offset,Math.min(offset+start.chunkSize,file.size));
     await transport.chunk(ticket,offset,chunk,controller.signal);
     controller.signal.throwIfAborted();received+=chunk.size;
     progress(Math.round(received/file.size*100),received);
    }
   }catch(error){controller.abort(error);throw error;}
  };
  const results=await Promise.allSettled(Array.from({length:Math.min(4,Math.ceil(file.size/start.chunkSize))},worker));
  signal.throwIfAborted();
  const failed=results.find((result):result is PromiseRejectedResult=>result.status==='rejected');
  if(failed)throw controller.signal.reason??failed.reason;
  return ticket;
 }catch(error){controller.abort(error);if(ticket)await transport.abort(ticket).catch(()=>{});throw error;}
 finally{signal.removeEventListener('abort',cancel);}
}
