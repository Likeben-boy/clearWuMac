import {useLayoutEffect,useRef,useState} from 'react';
import {bytes,colorFor,layout,valueOf,percentage} from '../domain';
import type {DirectoryMap,Entry,Metric,Settings} from '../types';
import type {Tile} from '../domain';
import Icon from './Icon';
export default function Treemap({data,metric,unit,onEnter,onGroup}:{data:DirectoryMap;metric:Metric;unit:Settings['unit'];onEnter:(entry:Entry)=>void;onGroup:()=>void}){
  const ref=useRef<HTMLDivElement>(null);const [width,setWidth]=useState(0);
  useLayoutEffect(()=>{const el=ref.current;if(!el)return;const observer=new ResizeObserver(()=>setWidth(el.clientWidth));observer.observe(el);setWidth(el.clientWidth);return()=>observer.disconnect();},[]);
  const tiles:Tile[]=data.items.map(entry=>({id:entry.id,name:entry.name,bytes:valueOf(entry,metric),entry}));
  if(BigInt(data.other_bytes)>0n)tiles.push({id:'group',name:`其他 ${data.other_count} 项`,bytes:data.other_bytes,group:true});
  const rects=layout(tiles,width,260);
  return <><div className="treemap" ref={ref} aria-label="当前目录占用分布">{rects.map(({tile,x,y,width:w,height:h})=>{const c=colorFor(tile.name);return <button key={tile.id} className={'tile '+(w<130||h<100?'compact ':'')+(w<50||h<36?'micro':'')} style={{left:x,top:y,width:w,height:h,background:c[0],color:c[1]}} onClick={()=>tile.group?onGroup():onEnter(tile.entry!)} title={`${tile.name} · ${bytes(tile.bytes,unit)}`} aria-label={`${tile.name}，${bytes(tile.bytes,unit)}`}><div><span className="tile-name"><Icon name={tile.entry?.kind==='file'?'file':'folder'}/><span>{tile.name}</span></span><div className="tile-size">{bytes(tile.bytes,unit)}</div></div><span className="tile-foot">{percentage(tile.bytes,data.total).toFixed(1)}% 的已知量<Icon name="arrow"/></span></button>;})}{!rects.length&&<div className="empty">没有可绘制的已知正数占用，请查看下方列表。</div>}</div><div className="map-caption"><span>面积表示当前口径的计量贡献</span><span>点击文件夹，继续探索 →</span></div><div className="legend">{tiles.slice(0,8).map(t=><span key={t.id}><i style={{background:colorFor(t.name)[2]}}/>{t.name}</span>)}</div>{(data.unknown_count!=='0'||data.zero_count!=='0')&&<p className="muted small">{data.unknown_count} 项大小未知、{data.zero_count} 项无正数贡献，均可在列表中查看。</p>}</>;
}
