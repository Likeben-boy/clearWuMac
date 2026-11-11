import type { Entry, Metric, Scan, View } from './types';
export function bytes(value:string|null|undefined, unit:'binary'|'decimal'='binary'):string {
  if(value==null) return '未知';
  const n=BigInt(value);const base=unit==='binary'?1024n:1000n;
  const units=unit==='binary'?['B','KiB','MiB','GiB','TiB','PiB','EiB']:['B','kB','MB','GB','TB','PB','EB'];
  let divisor=1n,i=0;while(n>=divisor*base&&i<units.length-1){divisor*=base;i++;}
  if(i===0)return `${n.toLocaleString()} B`;
  const hundredths=(n*100n+divisor/2n)/divisor;
  const whole=hundredths/100n, fraction=(hundredths%100n).toString().padStart(2,'0').replace(/0+$/,'');
  return `${whole.toLocaleString()}${fraction?'.'+fraction:''} ${units[i]}`;
}
export const valueOf=(e:Entry,m:Metric)=>m==='allocated'?e.known_allocated:e.known_logical;
export const unknownOf=(e:Entry,m:Metric)=>m==='allocated'?e.unknown_allocated:e.unknown_logical;
export function coverage(e:Entry,m:Metric):'known'|'partial'|'unknown'{
  const observed=m==='allocated'?e.observed_allocated:e.observed_logical;
  if(unknownOf(e,m)==='0')return 'known';return observed==='0'?'unknown':'partial';
}
export function contribution(e:Entry,m:Metric,unit:'binary'|'decimal'='binary'):string {
  if(e.excluded)return '按策略跳过';
  if(coverage(e,m)==='unknown')return '未知';
  const value=bytes(valueOf(e,m),unit);
  return coverage(e,m)==='partial'?`${value}（部分已知）`:value;
}
export const percentage=(part:string,total:string)=>BigInt(total)>0n?Number(BigInt(part)*10000n/BigInt(total))/100:0;
export const activeStates=['preparing','scanning','cancelling','finalizing'];
export const isActive=(scan:Scan|undefined)=>!!scan&&activeStates.includes(scan.state);
export const stateLabels:Record<string,string>={preparing:'正在准备',scanning:'正在扫描',cancelling:'正在取消',finalizing:'正在整理结果',completed:'扫描完成',completed_with_gaps:'部分完成',cancelled:'已取消',failed:'扫描失败',interrupted:'上次扫描中断'};
export function resetView(view:View,patch:Partial<View>):View{return {...view,...patch,cursor:null,previousCursors:[],groupToken:null};}
export function date(value:number|null|undefined):string{return value?new Date(value).toLocaleString('zh-CN',{month:'short',day:'numeric',hour:'2-digit',minute:'2-digit'}):'—';}
export function count(value:string|undefined):string{return BigInt(value??'0').toLocaleString();}
export const colors=[['#dfe6f8','#50649d','#8097ce'],['#e2ecd9','#617953','#9eb18d'],['#f4ead8','#96713b','#c6ad7d'],['#eae1f0','#866596','#b5a0c7'],['#dcece7','#527e70','#8bb6a6'],['#f2e1df','#99645e','#d2a09a']];
export function colorFor(name:string){let hash=0;for(const c of name)hash=(hash*31+c.charCodeAt(0))>>>0;return colors[hash%colors.length];}
export interface Tile {id:string;name:string;bytes:string;entry?:Entry;group?:boolean}
export interface Rect {tile:Tile;x:number;y:number;width:number;height:number}
export function layout(items:Tile[],width:number,height:number):Rect[]{
  const result:Rect[]=[];
  function split(list:Tile[],x:number,y:number,w:number,h:number){
    if(!list.length)return;
    if(list.length===1){result.push({tile:list[0],x,y,width:w,height:h});return;}
    const total=list.reduce((s,t)=>s+BigInt(t.bytes),0n);let sum=0n,index=0;
    for(let i=0;i<list.length-1;i++){sum+=BigInt(list[i].bytes);index=i+1;if(sum*2n>=total)break;}
    const ratio=Number(sum*1_000_000_000n/total)/1_000_000_000;
    if(w>=h){split(list.slice(0,index),x,y,w*ratio,h);split(list.slice(index),x+w*ratio,y,w*(1-ratio),h);}
    else{split(list.slice(0,index),x,y,w,h*ratio);split(list.slice(index),x,y+h*ratio,w,h*(1-ratio));}
  }
  split(items.filter(t=>BigInt(t.bytes)>0n),0,0,Math.max(0,width),Math.max(0,height));return result;
}
