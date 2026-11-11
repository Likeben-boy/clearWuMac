import { invoke, isTauri } from '@tauri-apps/api/core';
import { useEffect, useState } from 'react';
export const native = isTauri();
export const errorText = (error:unknown):string => typeof error==='object'&&error&&'message' in error?String(error.message):String(error);
export async function call<T>(command:string,args:Record<string,unknown>={}):Promise<T>{
  if(!native)throw new Error('当前为浏览器预览。真实磁盘分析仅在 SpaceLens 桌面应用中可用。');
  return invoke<T>(command,args);
}
export function useQuery<T>(command:string,args:Record<string,unknown>,enabled:boolean,refresh=0){
  const key=JSON.stringify(args);
  const [result,setResult]=useState<{data:T|null;error:string|null;loading:boolean}>({data:null,error:null,loading:false});
  useEffect(()=>{
    let current=true;
    if(!enabled){setResult({data:null,error:null,loading:false});return;}
    setResult({data:null,error:null,loading:true});
    call<T>(command,JSON.parse(key)).then(data=>{if(current)setResult({data,error:null,loading:false});}).catch(e=>{if(current)setResult({data:null,error:errorText(e),loading:false});});
    return()=>{current=false;};
  },[command,key,enabled,refresh]);
  return result;
}
